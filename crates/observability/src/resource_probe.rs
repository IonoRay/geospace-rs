use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use sysinfo::{
    DiskRefreshKind, Disks, Networks, Pid, ProcessRefreshKind, ProcessesToUpdate, System,
};

use super::resource_native;

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub at: Instant,
    pub background_at: Option<Instant>,
    pub identity: Option<u64>,
    pub counters: BTreeMap<String, Option<u64>>,
    pub rss: Option<u64>,
    pub virtual_memory: Option<u64>,
    pub extra: Value,
    pub system: Value,
    pub interfaces: BTreeMap<String, (u64, u64)>,
    pub measurement_us: u64,
    pub error: Option<String>,
}

pub(crate) fn micros(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}

pub(crate) fn metric(value: impl Into<Value>, unit: &str, status: &str, backend: &str) -> Value {
    json!({"value":value.into(),"unit":unit,"status":status,"backend":backend})
}

pub(crate) fn optional(value: Option<u64>, unit: &str, backend: &str) -> Value {
    metric(
        json!(value),
        unit,
        if value.is_some() {
            "available"
        } else {
            "unavailable"
        },
        backend,
    )
}

pub(crate) struct Probe {
    system: System,
    disks: Disks,
    networks: Networks,
    background_at: Option<Instant>,
    cpu_at: Option<Instant>,
    background: Value,
    interfaces: BTreeMap<String, (u64, u64)>,
    log_path: Option<PathBuf>,
}

impl Probe {
    pub fn new(log_path: Option<PathBuf>) -> Self {
        Self {
            system: System::new(),
            disks: Disks::new(),
            networks: Networks::new(),
            background_at: None,
            cpu_at: None,
            background: Value::Null,
            interfaces: BTreeMap::new(),
            log_path,
        }
    }

    pub fn snapshot(&mut self, sampled: bool) -> Snapshot {
        let started = Instant::now();
        let pid = Pid::from_u32(std::process::id());
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_memory(),
        );
        if self
            .background_at
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
        {
            self.system.refresh_memory();
            self.system.refresh_cpu_usage();
            let cpu_ready = self
                .cpu_at
                .is_some_and(|at| at.elapsed() >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            self.cpu_at = Some(Instant::now());
            self.disks
                .refresh_specifics(true, DiskRefreshKind::nothing().with_storage());
            if sampled {
                self.networks.refresh(true);
                self.interfaces = self
                    .networks
                    .iter()
                    .map(|(name, data)| {
                        (
                            name.clone(),
                            (data.total_received(), data.total_transmitted()),
                        )
                    })
                    .collect();
            }
            let load = System::load_average();
            self.background = json!({
                "cpu_pct":metric(if cpu_ready {json!(self.system.global_cpu_usage())} else {Value::Null},"percent_all_cores",if cpu_ready {"available"} else {"insufficient_window"},"sysinfo-0.37.2"),
                "logical_cpus":metric(json!(self.system.cpus().len()),"count","available","sysinfo-0.37.2"),
                "total_memory":optional(Some(self.system.total_memory()),"bytes","sysinfo-0.37.2"),
                "available_memory":optional(Some(self.system.available_memory()),"bytes","sysinfo-0.37.2"),
                "used_swap":optional(Some(self.system.used_swap()),"bytes","sysinfo-0.37.2"),
                "load_average":metric(json!([load.one,load.five,load.fifteen]),"runnable_tasks","available","sysinfo-0.37.2"),
            });
            self.background_at = Some(Instant::now());
        }
        let process = self.system.process(pid);
        let (counters, extra, error) = resource_native::read();
        let mut system = self.background.clone();
        system["snapshot_age_us"] = json!(self.background_at.map(|at| micros(at.elapsed())));
        Snapshot {
            at: Instant::now(),
            background_at: self.background_at,
            identity: process.map(sysinfo::Process::start_time),
            counters,
            rss: process.map(sysinfo::Process::memory),
            virtual_memory: process.map(sysinfo::Process::virtual_memory),
            extra,
            system,
            interfaces: self.interfaces.clone(),
            measurement_us: micros(started.elapsed()),
            error,
        }
    }

    pub fn filesystems(&self, home: Option<&Path>) -> Value {
        let mut mounts: BTreeMap<PathBuf, Value> = BTreeMap::new();
        let temporary = std::env::temp_dir();
        let paths = [
            ("data_home", home),
            ("temporary", Some(temporary.as_path())),
            ("log", self.log_path.as_deref()),
        ];
        let mut missing = Vec::new();
        for (role, path) in paths {
            let Some(path) = path else { continue };
            let mut existing = path;
            while !existing.exists() {
                let Some(parent) = existing.parent() else {
                    break;
                };
                existing = parent;
            }
            let resolved = existing
                .canonicalize()
                .unwrap_or_else(|_| existing.to_path_buf());
            let disk = self
                .disks
                .iter()
                .filter(|disk| resolved.starts_with(disk.mount_point()))
                .max_by_key(|disk| disk.mount_point().components().count());
            if let Some(disk) = disk {
                let entry=mounts.entry(disk.mount_point().to_path_buf()).or_insert_with(|| json!({
                    "mount":disk.mount_point(),"roles":[],
                    "total":optional(Some(disk.total_space()),"bytes","sysinfo-0.37.2"),
                    "available":optional(Some(disk.available_space()),"bytes","sysinfo-0.37.2"),
                }));
                entry["roles"]
                    .as_array_mut()
                    .expect("roles array")
                    .push(json!(role));
            } else {
                missing.push(json!({"role":role,"status":"unavailable"}));
            }
        }
        json!({"mounts":mounts.into_values().collect::<Vec<_>>(),"unavailable":missing,
            "snapshot_age_us":self.background_at.map(|at|micros(at.elapsed()))})
    }
}
