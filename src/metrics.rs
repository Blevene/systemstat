//! Metric model and collection. `Collector` owns the persistent `sysinfo`
//! handles (CPU%, network and disk I/O all need two samples), refreshes them
//! once per tick, and derives the `Metrics`/`History` the UI renders.
//!
//! Most numbers come from `sysinfo` (portable across Linux/macOS/Windows).
//! A few richer signals are read straight from Linux `sysfs`/`procfs` when
//! present — thermal zone, cpufreq current/max, `/proc/diskstats` — and the
//! POWER/HEALTH flags are derived from whatever data is available, so the whole
//! thing degrades to sensible defaults on any platform/architecture.

use std::collections::VecDeque;
use std::fs;
use std::time::Instant;

use sysinfo::{Components, Disks, Networks, System};

const HISTORY_LEN: usize = 120;

/// Cross-platform health flags surfaced under POWER / HEALTH. All are derived
/// from generally-available data (temperature, cpufreq, load, memory) so they
/// work on any architecture and degrade to `false` where a signal is missing.
#[derive(Default, Clone)]
pub struct HealthFlags {
    /// CPU temperature at or above the warning threshold.
    pub thermal_warn: bool,
    /// Current frequency is running well below the hardware ceiling.
    pub freq_scaled: bool,
    /// Load average per core exceeds 1.0 (run queue backing up).
    pub cpu_pressure: bool,
    /// RAM or swap utilisation is high.
    pub mem_pressure: bool,
}

/// A single snapshot of everything the dashboard shows.
#[derive(Clone)]
pub struct Metrics {
    // identity / system
    pub iface: String,
    pub model: String,
    pub arch: String,
    pub uptime: u64,
    pub total_ram_gib: f64,
    // cpu / thermal
    pub cpu_load: f64,
    pub cores: Vec<f64>,
    pub cpu_temp: f64,
    pub cpu_freq_mhz: u64,
    pub cpu_max_freq_mhz: u64,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub load_per_core: f64,
    // memory / storage
    pub ram_pct: f64,
    pub swap_pct: f64,
    pub disk_pct: f64,
    pub disk_read_kib: f64,
    pub disk_write_kib: f64,
    // network
    pub net_sent_kib: f64,
    pub net_recv_kib: f64,
    // power / health
    pub health: HealthFlags,
    pub system_health: f64,
    pub storage_health: f64,
    pub stability_avg: f64,
    pub health_why: String,
    pub alerts: u32,
    // doctor insight
    pub cooling: String,
    pub power: String,
    pub workload: String,
    pub storage_note: String,
    pub top_cpu: (String, f64),
    pub top_ram: (String, f64),
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics {
            iface: "—".into(),
            model: "Unknown".into(),
            arch: std::env::consts::ARCH.into(),
            uptime: 0,
            total_ram_gib: 0.0,
            cpu_load: 0.0,
            cores: Vec::new(),
            cpu_temp: 0.0,
            cpu_freq_mhz: 0,
            cpu_max_freq_mhz: 0,
            load1: 0.0,
            load5: 0.0,
            load15: 0.0,
            load_per_core: 0.0,
            ram_pct: 0.0,
            swap_pct: 0.0,
            disk_pct: 0.0,
            disk_read_kib: 0.0,
            disk_write_kib: 0.0,
            net_sent_kib: 0.0,
            net_recv_kib: 0.0,
            health: HealthFlags::default(),
            system_health: 100.0,
            storage_health: 100.0,
            stability_avg: 100.0,
            health_why: "nominal".into(),
            alerts: 0,
            cooling: "—".into(),
            power: "—".into(),
            workload: "—".into(),
            storage_note: "—".into(),
            top_cpu: ("—".into(), 0.0),
            top_ram: ("—".into(), 0.0),
        }
    }
}

/// Rolling trend buffers for the sparklines (newest pushed to the back).
#[derive(Default)]
pub struct History {
    pub cpu: VecDeque<f64>,
    pub ram: VecDeque<f64>,
    pub net: VecDeque<f64>,
    pub health: VecDeque<f64>,
    pub temp: VecDeque<f64>,
}

fn push_capped(buf: &mut VecDeque<f64>, v: f64) {
    if buf.len() >= HISTORY_LEN {
        buf.pop_front();
    }
    buf.push_back(v);
}

/// Owns the live `sysinfo` state and the derived snapshot/history.
pub struct Collector {
    sys: System,
    networks: Networks,
    disks: Disks,
    components: Components,
    /// (cumulative sectors read, cumulative sectors written, sampled at).
    prev_diskstats: Option<(u64, u64, Instant)>,
    last_net: Instant,
    /// Highest CPU frequency seen so far — a max-freq fallback off Linux/cpufreq.
    peak_freq_mhz: u64,
    pub metrics: Metrics,
    pub history: History,
}

impl Collector {
    pub fn new() -> Self {
        let sys = System::new_all();
        let networks = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();
        let mut c = Collector {
            sys,
            networks,
            disks,
            components,
            prev_diskstats: read_diskstats().map(|(r, w)| (r, w, Instant::now())),
            last_net: Instant::now(),
            peak_freq_mhz: 0,
            metrics: Metrics::default(),
            history: History::default(),
        };
        // Prime static, architecture-derived fields so the first frame isn't blank.
        c.metrics.arch = std::env::consts::ARCH.into();
        c.metrics.model = c
            .sys
            .cpus()
            .first()
            .map(|cpu| cpu.brand().trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(System::name)
            .unwrap_or_else(|| "Unknown".into());
        c
    }

    /// One per-tick update. README points here for tuning the health thresholds.
    pub fn refresh(&mut self) {
        self.sys.refresh_cpu();
        self.sys.refresh_memory();
        self.sys.refresh_processes();
        self.networks.refresh();
        self.disks.refresh();
        self.components.refresh();

        let m = &mut self.metrics;

        // ---- CPU / thermal ----
        m.cpu_load = self.sys.global_cpu_info().cpu_usage() as f64;
        m.cores = self.sys.cpus().iter().map(|c| c.cpu_usage() as f64).collect();
        m.cpu_freq_mhz = read_cpu_freq_mhz()
            .or_else(|| self.sys.cpus().first().map(|c| c.frequency()))
            .unwrap_or(0);
        // Max freq: the cpufreq ceiling when available, else the peak we've observed.
        self.peak_freq_mhz = self.peak_freq_mhz.max(m.cpu_freq_mhz);
        m.cpu_max_freq_mhz = read_cpu_max_freq_mhz().unwrap_or(0).max(self.peak_freq_mhz);
        m.cpu_temp = read_thermal_zone()
            .or_else(|| component_cpu_temp(&self.components))
            .unwrap_or(0.0);

        // ---- load / uptime ----
        let la = System::load_average();
        m.load1 = la.one;
        m.load5 = la.five;
        m.load15 = la.fifteen;
        m.load_per_core = m.load1 / m.cores.len().max(1) as f64;
        m.uptime = System::uptime();

        // ---- memory ----
        let total = self.sys.total_memory();
        m.total_ram_gib = total as f64 / 1024.0 / 1024.0 / 1024.0;
        m.ram_pct = pct(self.sys.used_memory(), total);
        m.swap_pct = pct(self.sys.used_swap(), self.sys.total_swap());

        // ---- disk space (root mount, else largest) ----
        m.disk_pct = root_disk_usage(&self.disks);

        // ---- disk I/O (sectors -> KiB/s via /proc/diskstats deltas) ----
        if let Some((r, w)) = read_diskstats() {
            let now = Instant::now();
            if let Some((pr, pw, pt)) = self.prev_diskstats {
                let dt = now.duration_since(pt).as_secs_f64().max(1e-3);
                // 512 bytes/sector -> KiB == *512/1024 == *0.5
                m.disk_read_kib = (r.saturating_sub(pr) as f64) * 0.5 / dt;
                m.disk_write_kib = (w.saturating_sub(pw) as f64) * 0.5 / dt;
            }
            self.prev_diskstats = Some((r, w, now));
        }

        // ---- network (bytes since last refresh -> KiB/s) ----
        let now = Instant::now();
        let dt = now.duration_since(self.last_net).as_secs_f64().max(1e-3);
        self.last_net = now;
        let (mut rx, mut tx) = (0u64, 0u64);
        let mut iface = String::new();
        for (name, data) in &self.networks {
            if name == "lo" || name.starts_with("lo") {
                continue;
            }
            rx += data.received();
            tx += data.transmitted();
            if iface.is_empty() {
                iface = name.clone();
            }
        }
        if !iface.is_empty() {
            m.iface = iface;
        }
        m.net_recv_kib = rx as f64 / 1024.0 / dt;
        m.net_sent_kib = tx as f64 / 1024.0 / dt;

        // ---- derived cross-platform health flags ----
        m.health = HealthFlags {
            thermal_warn: m.cpu_temp >= 80.0,
            freq_scaled: m.cpu_max_freq_mhz > 0
                && (m.cpu_freq_mhz as f64) < 0.9 * (m.cpu_max_freq_mhz as f64),
            cpu_pressure: m.load_per_core > 1.0,
            mem_pressure: m.ram_pct > 85.0 || m.swap_pct > 50.0,
        };

        // ---- top processes ----
        let (mut top_cpu, mut top_ram) = (("—".to_string(), 0.0_f64), ("—".to_string(), 0.0_f64));
        for proc in self.sys.processes().values() {
            let cpu = proc.cpu_usage() as f64;
            if cpu > top_cpu.1 {
                top_cpu = (proc.name().to_string(), cpu);
            }
            let ram = pct(proc.memory(), total);
            if ram > top_ram.1 {
                top_ram = (proc.name().to_string(), ram);
            }
        }
        m.top_cpu = top_cpu;
        m.top_ram = top_ram;

        // ---- derived heuristics (tune thresholds here) ----
        let mut health = 100.0_f64;
        let mut why = "nominal".to_string();
        if m.cpu_temp > 80.0 {
            health -= 30.0;
            why = format!("temp {:.0}°C", m.cpu_temp);
        } else if m.cpu_temp > 70.0 {
            health -= 15.0;
            why = format!("temp {:.0}°C", m.cpu_temp);
        } else if m.cpu_temp > 60.0 {
            health -= 5.0;
        }
        if m.load_per_core > 2.0 {
            health -= 20.0;
            if why == "nominal" {
                why = format!("load {:.2}", m.load1);
            }
        } else if m.load_per_core > 1.0 {
            health -= 10.0;
        }
        if m.cpu_load > 90.0 {
            health -= 10.0;
        }
        if m.health.mem_pressure {
            health -= 10.0;
            if why == "nominal" {
                why = "memory pressure".into();
            }
        }
        m.system_health = health.clamp(0.0, 100.0);
        m.health_why = why;

        // storage health: flat until ~70% full, then degrades.
        m.storage_health = (100.0 - (m.disk_pct - 70.0).max(0.0) * 1.6).clamp(0.0, 100.0);

        // alerts: count of tripped conditions.
        let mut alerts = 0u32;
        for cond in [
            m.health.thermal_warn,
            m.health.cpu_pressure,
            m.health.mem_pressure,
            m.cpu_load > 90.0,
            m.disk_pct > 90.0,
            m.swap_pct > 80.0,
        ] {
            alerts += cond as u32;
        }
        m.alerts = alerts;

        // doctor strings.
        m.cooling = if m.cpu_temp < 55.0 {
            "Good"
        } else if m.cpu_temp < 70.0 {
            "Adequate"
        } else {
            "Insufficient"
        }
        .into();
        m.power = if m.health.thermal_warn {
            "Thermal-limited"
        } else if m.health.cpu_pressure {
            "Under load"
        } else {
            "Nominal"
        }
        .into();
        m.workload = if m.cpu_load < 30.0 {
            "Light"
        } else if m.cpu_load < 70.0 {
            "Moderate"
        } else {
            "Heavy"
        }
        .into();
        m.storage_note = if m.disk_pct < 70.0 {
            "Healthy"
        } else if m.disk_pct < 90.0 {
            "Filling"
        } else {
            "Critical"
        }
        .into();

        // ---- history (push after this tick's values are settled) ----
        push_capped(&mut self.history.cpu, m.cpu_load);
        push_capped(&mut self.history.ram, m.ram_pct);
        push_capped(&mut self.history.net, m.net_sent_kib + m.net_recv_kib);
        push_capped(&mut self.history.health, m.system_health);
        push_capped(&mut self.history.temp, m.cpu_temp);

        // stability = mean of recent system-health samples.
        if !self.history.health.is_empty() {
            let sum: f64 = self.history.health.iter().sum();
            self.metrics.stability_avg = sum / self.history.health.len() as f64;
        }
    }
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

// ---- helpers ----

fn pct(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        used as f64 / total as f64 * 100.0
    }
}

fn read_first_line(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.lines().next().unwrap_or("").trim().to_string())
}

/// `/sys/class/thermal/thermal_zone0/temp` is milli-°C.
fn read_thermal_zone() -> Option<f64> {
    let raw = read_first_line("/sys/class/thermal/thermal_zone0/temp")?;
    raw.parse::<f64>().ok().map(|v| v / 1000.0)
}

/// Pick a `Components` sensor that looks like a CPU/package temperature.
fn component_cpu_temp(components: &Components) -> Option<f64> {
    let pick = |needle: &str| {
        components
            .iter()
            .find(|c| c.label().to_lowercase().contains(needle))
            .map(|c| c.temperature() as f64)
    };
    pick("package")
        .or_else(|| pick("cpu"))
        .or_else(|| pick("core"))
        .or_else(|| components.iter().next().map(|c| c.temperature() as f64))
}

/// `scaling_cur_freq` is kHz; convert to MHz. (Linux/cpufreq; absent elsewhere.)
fn read_cpu_freq_mhz() -> Option<u64> {
    let raw = read_first_line("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")?;
    raw.parse::<u64>().ok().map(|khz| khz / 1000)
}

/// `cpuinfo_max_freq` is the cpufreq hardware ceiling in kHz; convert to MHz.
fn read_cpu_max_freq_mhz() -> Option<u64> {
    let raw = read_first_line("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")?;
    raw.parse::<u64>().ok().map(|khz| khz / 1000)
}

/// Disk usage % for the `/` mount, falling back to the largest disk.
fn root_disk_usage(disks: &Disks) -> f64 {
    let root = disks.iter().find(|d| d.mount_point() == std::path::Path::new("/"));
    let disk = root.or_else(|| disks.iter().max_by_key(|d| d.total_space()));
    match disk {
        Some(d) => {
            let total = d.total_space();
            pct(total.saturating_sub(d.available_space()), total)
        }
        None => 0.0,
    }
}

/// True for whole-disk device names (so partitions aren't double-counted).
fn is_whole_disk(name: &str) -> bool {
    // mmcblk0 / nvme0n1 are whole disks; their partitions carry a trailing `pN`.
    if name.starts_with("mmcblk") || name.starts_with("nvme") {
        !name.contains('p')
    } else if name.starts_with("sd") || name.starts_with("vd") || name.starts_with("hd") {
        // sda is a disk, sda1 is a partition.
        !name.chars().last().map(|c| c.is_ascii_digit()).unwrap_or(false)
    } else {
        false
    }
}

/// Sum (sectors_read, sectors_written) across whole-disk devices in /proc/diskstats.
/// Fields (whitespace split): [2]=name [5]=sectors read [9]=sectors written.
fn read_diskstats() -> Option<(u64, u64)> {
    let content = fs::read_to_string("/proc/diskstats").ok()?;
    let mut read = 0u64;
    let mut written = 0u64;
    for line in content.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 10 {
            continue;
        }
        if !is_whole_disk(f[2]) {
            continue;
        }
        read += f[5].parse::<u64>().unwrap_or(0);
        written += f[9].parse::<u64>().unwrap_or(0);
    }
    Some((read, written))
}
