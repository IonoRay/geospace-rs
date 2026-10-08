//! Narrow, checked OS calls; no native calls leak into scientific crates.
#![allow(unsafe_code)]
use super::resource_probe::{metric, optional};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) fn read() -> (BTreeMap<String, Option<u64>>, Value, Option<String>) {
    let mut counters = BTreeMap::new();
    let mut extra = json!({});
    let mut error = None;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        // SAFETY: writable, correctly sized/aligned output and RUSAGE_SELF selector.
        let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
        if result == 0 {
            // SAFETY: getrusage succeeded and initialized the output structure.
            let usage = unsafe { usage.assume_init() };
            let time = |value: libc::timeval| {
                u64::try_from(value.tv_sec)
                    .ok()
                    .and_then(|v| v.checked_mul(1_000_000))
                    .and_then(|v| {
                        u64::try_from(value.tv_usec)
                            .ok()
                            .and_then(|us| v.checked_add(us))
                    })
            };
            let user = time(usage.ru_utime);
            let system = time(usage.ru_stime);
            counters.insert("cpu_user_us".into(), user);
            counters.insert("cpu_system_us".into(), system);
            counters.insert(
                "cpu_time_us".into(),
                user.zip(system).and_then(|(a, b)| a.checked_add(b)),
            );
            for (name, value) in [
                ("minor_faults", usage.ru_minflt),
                ("major_faults", usage.ru_majflt),
                ("voluntary_context_switches", usage.ru_nvcsw),
                ("involuntary_context_switches", usage.ru_nivcsw),
            ] {
                counters.insert(name.into(), u64::try_from(value).ok());
            }
            // Darwin reports bytes, Linux reports KiB. This is a process lifetime HWM.
            let maxrss = u64::try_from(usage.ru_maxrss).ok();
            #[cfg(target_os = "linux")]
            let maxrss = maxrss.and_then(|v| v.checked_mul(1024));
            extra["os_lifetime_rss_high_water"] = optional(maxrss, "bytes", "getrusage");
            extra["cpu_counter_resolution"] = metric(
                json!(1),
                "us_representation",
                "os_accounting_resolution_not_guaranteed",
                "getrusage",
            );
        } else {
            error = Some(std::io::Error::last_os_error().to_string());
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage_info_v2>::uninit();
        // SAFETY: current PID, RUSAGE_INFO_V2 matches the sized output buffer.
        let result = unsafe {
            libc::proc_pid_rusage(
                libc::getpid(),
                libc::RUSAGE_INFO_V2,
                usage.as_mut_ptr().cast(),
            )
        };
        if result == 0 {
            // SAFETY: proc_pid_rusage returned success for this output flavor.
            let usage = unsafe { usage.assume_init() };
            counters.insert("read_bytes".into(), Some(usage.ri_diskio_bytesread));
            counters.insert("written_bytes".into(), Some(usage.ri_diskio_byteswritten));
        } else {
            error = Some(std::io::Error::last_os_error().to_string());
        }
    }
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_to_string("/proc/self/io") {
            Ok(text) => {
                for line in text.lines() {
                    if let Some((key, value)) = line.split_once(':') {
                        let name = match key {
                            "read_bytes" => "read_bytes",
                            "write_bytes" => "written_bytes",
                            _ => continue,
                        };
                        counters.insert(name.into(), value.trim().parse().ok());
                    }
                }
            }
            Err(e) => error = Some(e.to_string()),
        }
    }
    for name in [
        "cpu_time_us",
        "cpu_user_us",
        "cpu_system_us",
        "read_bytes",
        "written_bytes",
        "minor_faults",
        "major_faults",
        "voluntary_context_switches",
        "involuntary_context_switches",
    ] {
        counters.entry(name.into()).or_insert(None);
    }
    unsupported(&mut extra);
    (counters, extra, error)
}

fn unsupported(extra: &mut Value) {
    for (name, unit) in [
        ("threads", "count"),
        ("open_handles", "count"),
        ("io_operations", "count"),
        ("hardware_counters", "count"),
        ("device_latency", "us"),
        ("gpu", "percent"),
        ("energy", "joules"),
    ] {
        extra[name] = metric(Value::Null, unit, "unsupported", "none");
    }
}
