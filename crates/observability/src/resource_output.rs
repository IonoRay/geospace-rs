use super::{
    resource_probe::{Snapshot, metric, micros, optional},
    resources::Samples,
};
use serde_json::{Value, json};
use std::time::Duration;

pub(crate) fn delta(
    before: Option<u64>,
    after: Option<u64>,
    identity: bool,
) -> (Option<u64>, &'static str) {
    if !identity {
        return (None, "identity_changed");
    }
    match before.zip(after) {
        Some((a, b)) => match b.checked_sub(a) {
            Some(v) => (Some(v), "available"),
            None => (None, "counter_reset"),
        },
        None => (None, "unavailable"),
    }
}

#[allow(clippy::too_many_arguments, clippy::similar_names)]
pub(crate) fn emit(
    parent: &tracing::Id,
    query_id: u64,
    start: Option<&Snapshot>,
    end: Option<&Snapshot>,
    samples: &Samples,
    interval: Duration,
    sampled: bool,
    sampler_failed: bool,
    filesystems: Option<Value>,
) {
    let (Some(start), Some(end)) = (start, end) else {
        tracing::info!(target:"ionoray_observability",parent:parent,event="resource.window",scope="process",query_id,status="unavailable");
        return;
    };
    let wall = micros(end.at.saturating_duration_since(start.at));
    let identity = start.identity.is_some() && start.identity == end.identity;
    let mut process = json!({});
    let mut cpu = None;
    let mut cpu_status = "unavailable";
    for (name, after) in &end.counters {
        let before = start.counters.get(name).copied().flatten();
        let (change, status) = delta(before, *after, identity);
        let unit = if name.ends_with("_us") {
            "us"
        } else if name.ends_with("_bytes") {
            "bytes"
        } else {
            "count"
        };
        let backend = if name.ends_with("_bytes") {
            if cfg!(target_os = "macos") {
                "proc_pid_rusage:RUSAGE_INFO_V2"
            } else {
                "proc-self-io"
            }
        } else {
            "getrusage:RUSAGE_SELF"
        };
        process[name] = json!({"start":optional(before,unit,backend),"end":optional(*after,unit,backend),
            "delta":metric(json!(change),unit,status,backend)});
        if name == "cpu_time_us" {
            cpu = change;
            cpu_status = status;
        }
        if name.ends_with("_bytes") {
            #[allow(clippy::cast_precision_loss)]
            let rate = change
                .filter(|_| wall > 0)
                .map(|v| v as f64 * 1e6 / wall as f64);
            process[name]["rate"] =
                metric(json!(rate), "bytes/s", rate_status(status, wall), backend);
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let percent = cpu
        .filter(|_| wall > 0)
        .map(|cpu| cpu as f64 * 100.0 / wall as f64);
    process["cpu_pct_one_core"] = metric(
        json!(percent),
        "percent_one_core",
        rate_status(cpu_status, wall),
        "delta_cpu/delta_wall",
    );
    process["rss"] = json!({"start":optional(start.rss,"bytes","sysinfo-0.37.2"),
        "end":optional(end.rss,"bytes","sysinfo-0.37.2"),
        "delta":metric(json!(end.rss.zip(start.rss).map(|(end,start)|i128::from(end)-i128::from(start))),"bytes_signed",if end.rss.is_some()&&start.rss.is_some(){"available"}else{"unavailable"},"sysinfo-0.37.2")});
    process["virtual_memory"] = optional(end.virtual_memory, "bytes", "sysinfo-0.37.2");
    process["sampled_rss_max"] = metric(
        json!(samples.rss_max),
        "bytes",
        sample_status(sampled, sampler_failed, samples.count, samples.rss_max),
        "sysinfo-0.37.2 sampled",
    );
    process["optional_metrics"] = end.extra.clone();
    process["backend_error"] = json!(end.error);
    let measurement_us = start.measurement_us + end.measurement_us + samples.measurement_us;
    tracing::info!(target:"ionoray_observability",parent:parent,event="resource.window",scope="process",query_id,
        pid=std::process::id(),elapsed_us=wall,measurement_elapsed_us=measurement_us,
        sample_count=samples.count,interval_ms=micros(interval)/1000,overlapping_window=samples.overlaps,
        profile_mode=if sampled{"sampled"}else{"basic"},metrics_json=%process,
        "point resource observation complete");
    emit_background(parent, query_id, start, end, sampled, filesystems);
}

pub(crate) fn rate_status(counter_status: &'static str, wall: u64) -> &'static str {
    if counter_status == "available" && wall == 0 {
        "insufficient_window"
    } else {
        counter_status
    }
}

pub(crate) fn sample_status(
    enabled: bool,
    failed: bool,
    count: u64,
    value: Option<u64>,
) -> &'static str {
    if !enabled {
        "disabled"
    } else if failed {
        "unavailable"
    } else if count == 0 {
        "insufficient_window"
    } else if value.is_none() {
        "unavailable"
    } else {
        "available"
    }
}

fn emit_background(
    parent: &tracing::Id,
    query_id: u64,
    start: &Snapshot,
    end: &Snapshot,
    sampled: bool,
    filesystems: Option<Value>,
) {
    let mut system = end.system.clone();
    if sampled {
        let interfaces: Vec<_> = end
            .interfaces
            .iter()
            .map(|(name, (rx, tx))| {
                let previous = start.interfaces.get(name);
                let (dr, sr) = background_delta(
                    previous.map(|v| v.0),
                    Some(*rx),
                    start.background_at != end.background_at,
                );
                let (dt, st) = background_delta(
                    previous.map(|v| v.1),
                    Some(*tx),
                    start.background_at != end.background_at,
                );
                json!({"name":name,"received":optional(Some(*rx),"bytes","sysinfo-interface"),
                "transmitted":optional(Some(*tx),"bytes","sysinfo-interface"),
                "received_delta":metric(json!(dr),"bytes",sr,"sysinfo-interface"),
                "transmitted_delta":metric(json!(dt),"bytes",st,"sysinfo-interface")})
            })
            .collect();
        system["interfaces"] = json!(interfaces);
    }
    tracing::info!(target:"ionoray_observability",parent:parent,event="resource.background",scope="system",query_id,metrics_json=%system);
    tracing::info!(target:"ionoray_observability",parent:parent,event="resource.filesystems",scope="filesystem",query_id,
        metrics_json=%filesystems.unwrap_or(serde_json::Value::Null));
}

pub(crate) fn background_delta(
    before: Option<u64>,
    after: Option<u64>,
    refreshed: bool,
) -> (Option<u64>, &'static str) {
    if refreshed {
        delta(before, after, true)
    } else {
        (None, "insufficient_window")
    }
}
