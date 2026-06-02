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
            .map(|cpu| clean_brand(cpu.brand()))
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
        m.cores = self
            .sys
            .cpus()
            .iter()
            .map(|c| c.cpu_usage() as f64)
            .collect();
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

        // ---- network (bytes since last refresh -> KiB/s, for the primary iface) ----
        let now = Instant::now();
        let dt = now.duration_since(self.last_net).as_secs_f64().max(1e-3);
        self.last_net = now;
        // Primary interface: the default route when known, else the busiest
        // non-loopback link (so we don't latch onto a docker/virtual bridge).
        let primary = primary_iface(&self.networks);
        if let Some((name, rx, tx)) = primary {
            m.iface = name;
            m.net_recv_kib = rx as f64 / 1024.0 / dt;
            m.net_sent_kib = tx as f64 / 1024.0 / dt;
        } else {
            m.net_recv_kib = 0.0;
            m.net_sent_kib = 0.0;
        }

        // ---- derived cross-platform health flags ----
        m.health = derive_health_flags(
            m.cpu_temp,
            m.cpu_freq_mhz,
            m.cpu_max_freq_mhz,
            m.load_per_core,
            m.ram_pct,
            m.swap_pct,
        );

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

        // ---- derived heuristics (tune the thresholds in the helpers below) ----
        let (health, why) = derive_system_health(m);
        m.system_health = health;
        m.health_why = why;
        m.storage_health = derive_storage_health(m.disk_pct);
        m.alerts = count_alerts(m);

        // doctor strings.
        m.cooling = cooling_label(m.cpu_temp).into();
        m.power = power_label(&m.health).into();
        m.workload = workload_label(m.cpu_load).into();
        m.storage_note = storage_note_label(m.disk_pct).into();

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

/// Tidy a CPU brand for display: drop `(R)`/`(TM)` marks, the redundant "CPU"
/// filler, and the trailing "@ x.yGHz" clock, then collapse whitespace. Turns
/// e.g. "Intel(R) Xeon(R) CPU E5-2620 v3 @ 2.40GHz" into "Intel Xeon E5-2620 v3".
fn clean_brand(brand: &str) -> String {
    let mut s = brand.to_string();
    for mark in ["(R)", "(r)", "(TM)", "(tm)"] {
        s = s.replace(mark, "");
    }
    if let Some(at) = s.find('@') {
        s.truncate(at);
    }
    s.split_whitespace()
        .filter(|w| *w != "CPU")
        .collect::<Vec<_>>()
        .join(" ")
}

/// Disk usage % for the `/` mount, falling back to the largest disk.
fn root_disk_usage(disks: &Disks) -> f64 {
    let root = disks
        .iter()
        .find(|d| d.mount_point() == std::path::Path::new("/"));
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
        !name
            .chars()
            .last()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
    } else {
        false
    }
}

fn read_diskstats() -> Option<(u64, u64)> {
    let content = fs::read_to_string("/proc/diskstats").ok()?;
    Some(parse_diskstats(&content))
}

/// Sum (sectors_read, sectors_written) across whole-disk devices in /proc/diskstats.
/// Fields (whitespace split): [2]=name [5]=sectors read [9]=sectors written.
fn parse_diskstats(content: &str) -> (u64, u64) {
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
    (read, written)
}

/// Choose the interface to display and rate: the default-route link when known,
/// otherwise the busiest non-loopback link (avoids latching onto a virtual
/// bridge). Returns (name, bytes received this tick, bytes transmitted this tick).
fn primary_iface(networks: &Networks) -> Option<(String, u64, u64)> {
    // (name, recv_this_tick, sent_this_tick, cumulative_bytes) for real links.
    let mut links: Vec<(String, u64, u64, u64)> = networks
        .iter()
        .filter(|(n, _)| {
            let n = n.as_str();
            n != "lo" && !n.starts_with("lo")
        })
        .map(|(n, d)| {
            (
                n.clone(),
                d.received(),
                d.transmitted(),
                d.total_received() + d.total_transmitted(),
            )
        })
        .collect();
    if links.is_empty() {
        return None;
    }
    let pick = default_route_iface()
        .and_then(|name| links.iter().position(|l| l.0 == name))
        .unwrap_or_else(|| {
            links
                .iter()
                .enumerate()
                .max_by_key(|(_, l)| l.3)
                .map(|(i, _)| i)
                .unwrap_or(0)
        });
    let (name, rx, tx, _) = links.swap_remove(pick);
    Some((name, rx, tx))
}

/// The interface backing the IPv4 default route (Linux `/proc/net/route`).
/// `None` off Linux or when there is no default route.
fn default_route_iface() -> Option<String> {
    let content = fs::read_to_string("/proc/net/route").ok()?;
    parse_default_route(&content)
}

/// Find the iface whose route Destination is `00000000` (0.0.0.0, the default
/// route). Columns are: Iface  Destination  Gateway  Flags ...
fn parse_default_route(content: &str) -> Option<String> {
    for line in content.lines().skip(1) {
        let mut f = line.split_whitespace();
        if let (Some(iface), Some(dest)) = (f.next(), f.next()) {
            if dest == "00000000" {
                return Some(iface.to_string());
            }
        }
    }
    None
}

// ---- pure derivations (unit-tested; tune dashboard thresholds here) ----

/// Cross-platform POWER/HEALTH flags from generally-available signals.
fn derive_health_flags(
    cpu_temp: f64,
    cpu_freq_mhz: u64,
    cpu_max_freq_mhz: u64,
    load_per_core: f64,
    ram_pct: f64,
    swap_pct: f64,
) -> HealthFlags {
    HealthFlags {
        thermal_warn: cpu_temp >= 80.0,
        freq_scaled: cpu_max_freq_mhz > 0
            && (cpu_freq_mhz as f64) < 0.9 * (cpu_max_freq_mhz as f64),
        cpu_pressure: load_per_core > 1.0,
        mem_pressure: ram_pct > 85.0 || swap_pct > 50.0,
    }
}

/// System health score (0..=100) plus the dominant reason string.
/// Reads the already-populated temp/load/cpu fields and `health.mem_pressure`.
fn derive_system_health(m: &Metrics) -> (f64, String) {
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
    (health.clamp(0.0, 100.0), why)
}

/// Storage health: flat until ~70% full, then degrades linearly.
fn derive_storage_health(disk_pct: f64) -> f64 {
    (100.0 - (disk_pct - 70.0).max(0.0) * 1.6).clamp(0.0, 100.0)
}

/// Count of tripped alert conditions shown in DOCTOR INSIGHT.
fn count_alerts(m: &Metrics) -> u32 {
    let conds = [
        m.health.thermal_warn,
        m.health.cpu_pressure,
        m.health.mem_pressure,
        m.cpu_load > 90.0,
        m.disk_pct > 90.0,
        m.swap_pct > 80.0,
    ];
    conds.iter().filter(|&&c| c).count() as u32
}

fn cooling_label(cpu_temp: f64) -> &'static str {
    if cpu_temp < 55.0 {
        "Good"
    } else if cpu_temp < 70.0 {
        "Adequate"
    } else {
        "Insufficient"
    }
}

fn power_label(h: &HealthFlags) -> &'static str {
    if h.thermal_warn {
        "Thermal-limited"
    } else if h.cpu_pressure {
        "Under load"
    } else {
        "Nominal"
    }
}

fn workload_label(cpu_load: f64) -> &'static str {
    if cpu_load < 30.0 {
        "Light"
    } else if cpu_load < 70.0 {
        "Moderate"
    } else {
        "Heavy"
    }
}

fn storage_note_label(disk_pct: f64) -> &'static str {
    if disk_pct < 70.0 {
        "Healthy"
    } else if disk_pct < 90.0 {
        "Filling"
    } else {
        "Critical"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pct_handles_zero_total() {
        assert_eq!(pct(5, 0), 0.0);
        assert_eq!(pct(0, 100), 0.0);
        assert_eq!(pct(50, 200), 25.0);
        assert_eq!(pct(200, 200), 100.0);
    }

    #[test]
    fn whole_disk_excludes_partitions_and_virtual() {
        // whole disks
        assert!(is_whole_disk("sda"));
        assert!(is_whole_disk("vdb"));
        assert!(is_whole_disk("hda"));
        assert!(is_whole_disk("nvme0n1"));
        assert!(is_whole_disk("mmcblk0"));
        // partitions
        assert!(!is_whole_disk("sda1"));
        assert!(!is_whole_disk("nvme0n1p2"));
        assert!(!is_whole_disk("mmcblk0p1"));
        // virtual / unrelated devices
        assert!(!is_whole_disk("loop0"));
        assert!(!is_whole_disk("ram0"));
        assert!(!is_whole_disk("dm-0"));
    }

    #[test]
    fn diskstats_sums_whole_disks_only() {
        // Real-ish lines: [2]=name [5]=sectors read [9]=sectors written.
        let sample = "\
   8       0 sda 100 0 2000 50 200 0 4000 60 0 0 0
   8       1 sda1 10 0 999 5 20 0 999 6 0 0 0
 259       0 nvme0n1 1 0 1000 1 2 0 8000 2 0 0 0
   7       0 loop0 1 0 12345 1 0 0 0 0 0 0 0";
        // sda: read 2000, write 4000; nvme0n1: read 1000, write 8000.
        // sda1 (partition) and loop0 (virtual) excluded.
        assert_eq!(parse_diskstats(sample), (3000, 12000));
    }

    #[test]
    fn diskstats_tolerates_short_and_garbage_lines() {
        assert_eq!(parse_diskstats(""), (0, 0));
        assert_eq!(parse_diskstats("garbage\n8 0 sda x y\n"), (0, 0));
    }

    #[test]
    fn default_route_picks_zero_destination_iface() {
        // Real /proc/net/route shape: header then one row per route.
        let sample = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask
eth0\t0000FEA9\t00000000\t0001\t0\t0\t1000\t0000FFFF
wlan0\t00000000\t0102A8C0\t0003\t0\t0\t600\t00000000
eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000";
        // First row has a non-default destination; wlan0 is the real default route.
        assert_eq!(parse_default_route(sample), Some("wlan0".to_string()));
    }

    #[test]
    fn default_route_none_when_absent_or_empty() {
        assert_eq!(parse_default_route(""), None);
        assert_eq!(parse_default_route("Iface\tDestination\n"), None);
        let no_default = "Iface\tDestination\neth0\t0000FEA9\n";
        assert_eq!(parse_default_route(no_default), None);
    }

    #[test]
    fn health_flags_boundaries() {
        // nominal: cool, freq near max, light load, ample memory.
        let ok = derive_health_flags(50.0, 3000, 3200, 0.3, 40.0, 0.0);
        assert!(!ok.thermal_warn && !ok.cpu_pressure && !ok.mem_pressure);
        // 3000/3200 = 93.75% -> not scaled
        assert!(!ok.freq_scaled);

        // thermal warning is inclusive at 80.
        assert!(derive_health_flags(80.0, 3000, 3200, 0.3, 40.0, 0.0).thermal_warn);
        assert!(!derive_health_flags(79.9, 3000, 3200, 0.3, 40.0, 0.0).thermal_warn);

        // freq scaled when current < 90% of max; unknown max (0) => never scaled.
        assert!(derive_health_flags(50.0, 1000, 3200, 0.3, 40.0, 0.0).freq_scaled);
        assert!(!derive_health_flags(50.0, 1000, 0, 0.3, 40.0, 0.0).freq_scaled);

        // pressure thresholds.
        assert!(derive_health_flags(50.0, 3000, 3200, 1.01, 40.0, 0.0).cpu_pressure);
        assert!(!derive_health_flags(50.0, 3000, 3200, 1.0, 40.0, 0.0).cpu_pressure);
        assert!(derive_health_flags(50.0, 3000, 3200, 0.3, 90.0, 0.0).mem_pressure);
        assert!(derive_health_flags(50.0, 3000, 3200, 0.3, 40.0, 60.0).mem_pressure);
    }

    fn metrics_with(
        cpu_temp: f64,
        load_per_core: f64,
        cpu_load: f64,
        mem_pressure: bool,
    ) -> Metrics {
        Metrics {
            cpu_temp,
            load_per_core,
            cpu_load,
            health: HealthFlags {
                mem_pressure,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn system_health_nominal_is_full() {
        let (h, why) = derive_system_health(&metrics_with(45.0, 0.3, 10.0, false));
        assert_eq!(h, 100.0);
        assert_eq!(why, "nominal");
    }

    #[test]
    fn system_health_penalises_and_names_dominant_cause() {
        // hot CPU dominates the reason string.
        let (h, why) = derive_system_health(&metrics_with(85.0, 0.3, 10.0, false));
        assert_eq!(h, 70.0);
        assert!(why.starts_with("temp"));

        // memory pressure names itself when nothing hotter trips.
        let (_h, why) = derive_system_health(&metrics_with(45.0, 0.3, 10.0, true));
        assert_eq!(why, "memory pressure");

        // stacked penalties clamp at 0, never negative.
        let (h, _why) = derive_system_health(&metrics_with(95.0, 3.0, 95.0, true));
        assert!(h >= 0.0);
    }

    #[test]
    fn storage_health_curve() {
        assert_eq!(derive_storage_health(50.0), 100.0); // flat below 70%
        assert_eq!(derive_storage_health(70.0), 100.0);
        assert_eq!(derive_storage_health(80.0), 84.0); // 100 - 10*1.6
        assert_eq!(derive_storage_health(100.0), 52.0); // 100 - 30*1.6
        assert!(derive_storage_health(100.0) >= 0.0);
    }

    #[test]
    fn alerts_count_distinct_conditions() {
        let mut m = Metrics::default();
        assert_eq!(count_alerts(&m), 0);
        m.health.thermal_warn = true;
        m.cpu_load = 95.0; // > 90
        m.disk_pct = 95.0; // > 90
        assert_eq!(count_alerts(&m), 3);
    }

    #[test]
    fn clean_brand_strips_marks_filler_and_clock() {
        assert_eq!(
            clean_brand("Intel(R) Xeon(R) CPU E5-2620 v3 @ 2.40GHz"),
            "Intel Xeon E5-2620 v3"
        );
        assert_eq!(
            clean_brand("Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz"),
            "Intel Core i7-9750H"
        );
        // ARM brands carry none of that cruft and pass through unchanged.
        assert_eq!(clean_brand("Cortex-A76"), "Cortex-A76");
        assert_eq!(clean_brand("  AMD Ryzen 9 5900X  "), "AMD Ryzen 9 5900X");
    }

    #[test]
    fn doctor_labels() {
        assert_eq!(cooling_label(40.0), "Good");
        assert_eq!(cooling_label(60.0), "Adequate");
        assert_eq!(cooling_label(85.0), "Insufficient");
        assert_eq!(workload_label(10.0), "Light");
        assert_eq!(workload_label(50.0), "Moderate");
        assert_eq!(workload_label(95.0), "Heavy");
        assert_eq!(storage_note_label(50.0), "Healthy");
        assert_eq!(storage_note_label(80.0), "Filling");
        assert_eq!(storage_note_label(95.0), "Critical");

        let hot = HealthFlags {
            thermal_warn: true,
            ..Default::default()
        };
        assert_eq!(power_label(&hot), "Thermal-limited");
        let busy = HealthFlags {
            cpu_pressure: true,
            ..Default::default()
        };
        assert_eq!(power_label(&busy), "Under load");
        assert_eq!(power_label(&HealthFlags::default()), "Nominal");
    }
}
