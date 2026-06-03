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
use std::process::Command;
use std::time::Instant;

use serde::Serialize;
use sysinfo::{Components, Disks, Networks, System};
use systemstat::{Platform, System as StatSystem};

use crate::config::Thresholds;

const HISTORY_LEN: usize = 120;
/// How many processes to keep for the process view / JSON output.
pub const PROC_LIMIT: usize = 15;

/// A single process for the process view (and JSON output).
#[derive(Clone, Serialize)]
pub struct Proc {
    pub name: String,
    pub pid: u32,
    pub cpu: f64,
    pub mem_pct: f64,
}

/// GPU snapshot, shown only when a GPU is detected. All metric fields are
/// optional so partial data (e.g. name + temp but no utilization) still renders.
#[derive(Clone, Serialize)]
pub struct GpuInfo {
    pub name: String,
    pub util_pct: Option<f64>,
    pub temp_c: Option<f64>,
    pub mem_used_mib: Option<f64>,
    pub mem_total_mib: Option<f64>,
}

/// A mounted filesystem for the detail view.
#[derive(Clone, Serialize)]
pub struct MountInfo {
    pub mount: String,
    pub fs: String,
    pub total_gib: f64,
    pub used_pct: f64,
}

/// A network interface's cumulative totals for the detail view.
#[derive(Clone, Serialize)]
pub struct IfaceInfo {
    pub name: String,
    pub rx_mib: f64,
    pub tx_mib: f64,
}

/// Cross-platform health flags surfaced under POWER / HEALTH. All are derived
/// from generally-available data (temperature, cpufreq, load, memory) so they
/// work on any architecture and degrade to `false` where a signal is missing.
#[derive(Default, Clone, Serialize)]
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
/// A process and its usage of a single resource (CPU or RAM), as a percentage.
/// A named struct (rather than a tuple) so `--json` emits stable
/// `{"name": ..., "pct": ...}` objects instead of positional arrays.
#[derive(Clone, Serialize)]
pub struct ProcUsage {
    pub name: String,
    pub pct: f64,
}

#[derive(Clone, Serialize)]
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
    /// Battery charge %, or `None` when the host has no battery (desktops, Pis).
    /// Currently Linux-only: systemstat's macOS/Windows backends return `Err`
    /// for `battery_life`, so this stays `None` there until a native source is added.
    pub battery: Option<f64>,
    /// On AC power (true when unknown / no battery, e.g. all non-Linux hosts).
    pub on_ac: bool,
    pub system_health: f64,
    pub storage_health: f64,
    pub stability_avg: f64,
    pub health_why: String,
    pub alerts: u32,
    // insights
    pub status: String,
    pub advisories: Vec<String>,
    pub cooling: String,
    pub power: String,
    pub workload: String,
    pub storage_note: String,
    pub top_cpu: ProcUsage,
    pub top_ram: ProcUsage,
    /// Top processes (by CPU or memory), for the process view.
    pub procs: Vec<Proc>,
    /// All real mounts, for the detail view.
    pub mounts: Vec<MountInfo>,
    /// All non-loopback interfaces (cumulative totals), for the detail view.
    pub ifaces: Vec<IfaceInfo>,
    /// GPU info when one is detected; `None` otherwise (no GPU section renders).
    pub gpu: Option<GpuInfo>,
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
            battery: None,
            on_ac: true,
            system_health: 100.0,
            storage_health: 100.0,
            stability_avg: 100.0,
            health_why: "nominal".into(),
            alerts: 0,
            status: "Healthy".into(),
            advisories: Vec::new(),
            cooling: "—".into(),
            power: "—".into(),
            workload: "—".into(),
            storage_note: "—".into(),
            top_cpu: ProcUsage {
                name: "—".into(),
                pct: 0.0,
            },
            top_ram: ProcUsage {
                name: "—".into(),
                pct: 0.0,
            },
            procs: Vec::new(),
            mounts: Vec::new(),
            ifaces: Vec::new(),
            gpu: None,
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
    /// Supplementary cross-platform backend (battery, AC power, portable temp).
    stat: StatSystem,
    networks: Networks,
    disks: Disks,
    components: Components,
    /// (cumulative sectors read, cumulative sectors written, sampled at).
    prev_diskstats: Option<(u64, u64, Instant)>,
    last_net: Instant,
    /// Highest CPU frequency seen so far — a max-freq fallback off Linux/cpufreq.
    peak_freq_mhz: u64,
    /// While true, probe for a GPU each tick; cleared after the first miss so
    /// GPU-less hosts pay only one probe.
    probe_gpu: bool,
    thresholds: Thresholds,
    pub metrics: Metrics,
    pub history: History,
}

impl Collector {
    pub fn new() -> Self {
        Self::with_thresholds(Thresholds::default())
    }

    pub fn with_thresholds(thresholds: Thresholds) -> Self {
        let sys = System::new_all();
        let networks = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();
        let mut c = Collector {
            sys,
            stat: StatSystem::new(),
            networks,
            disks,
            components,
            prev_diskstats: read_diskstats().map(|(r, w)| (r, w, Instant::now())),
            last_net: Instant::now(),
            peak_freq_mhz: 0,
            probe_gpu: true,
            thresholds,
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

        let t = self.thresholds; // Copy; lets us borrow self.metrics mutably below.
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
            .or_else(|| self.stat.cpu_temp().ok().map(|t| t as f64))
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

        // ---- disk space (root mount, else largest) + per-mount detail ----
        m.disk_pct = root_disk_usage(&self.disks);
        m.mounts = self
            .disks
            .iter()
            .filter(|d| d.total_space() > 0)
            .map(|d| {
                let total = d.total_space();
                MountInfo {
                    mount: d.mount_point().to_string_lossy().into_owned(),
                    fs: d.file_system().to_string_lossy().into_owned(),
                    total_gib: total as f64 / 1024.0 / 1024.0 / 1024.0,
                    used_pct: pct(total.saturating_sub(d.available_space()), total),
                }
            })
            .collect();

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
        m.ifaces = self
            .networks
            .iter()
            .filter(|(n, _)| !is_loopback(n.as_str()))
            .map(|(n, d)| IfaceInfo {
                name: n.clone(),
                rx_mib: d.total_received() as f64 / 1024.0 / 1024.0,
                tx_mib: d.total_transmitted() as f64 / 1024.0 / 1024.0,
            })
            .collect();

        // ---- battery / power source (systemstat; None on desktops/Pis) ----
        m.battery = self
            .stat
            .battery_life()
            .ok()
            .map(|b| (b.remaining_capacity as f64 * 100.0).clamp(0.0, 100.0));
        m.on_ac = self.stat.on_ac_power().unwrap_or(true);

        // ---- derived cross-platform health flags ----
        m.health = derive_health_flags(
            m.cpu_temp,
            m.cpu_freq_mhz,
            m.cpu_max_freq_mhz,
            m.load_per_core,
            m.ram_pct,
            m.swap_pct,
            &t,
        );

        // ---- processes ----
        let mut procs: Vec<Proc> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, p)| Proc {
                name: p.name().to_string(),
                pid: pid.as_u32(),
                cpu: p.cpu_usage() as f64,
                mem_pct: pct(p.memory(), total),
            })
            .collect();
        // Single top-CPU / top-RAM for INSIGHTS (computed over all processes).
        m.top_cpu = procs
            .iter()
            .max_by(|a, b| a.cpu.total_cmp(&b.cpu))
            .map(|p| ProcUsage {
                name: p.name.clone(),
                pct: p.cpu,
            })
            .unwrap_or(ProcUsage {
                name: "—".into(),
                pct: 0.0,
            });
        m.top_ram = procs
            .iter()
            .max_by(|a, b| a.mem_pct.total_cmp(&b.mem_pct))
            .map(|p| ProcUsage {
                name: p.name.clone(),
                pct: p.mem_pct,
            })
            .unwrap_or(ProcUsage {
                name: "—".into(),
                pct: 0.0,
            });
        // Keep the union of the top PROC_LIMIT by CPU and by memory, so the
        // process view shows a correct top-N under either sort key. Truncating
        // by a single combined metric could drop a genuinely high-CPU,
        // low-memory process (or vice versa).
        let mut keep: std::collections::HashSet<u32> = std::collections::HashSet::new();
        procs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        keep.extend(procs.iter().take(PROC_LIMIT).map(|p| p.pid));
        procs.sort_by(|a, b| b.mem_pct.total_cmp(&a.mem_pct));
        keep.extend(procs.iter().take(PROC_LIMIT).map(|p| p.pid));
        procs.retain(|p| keep.contains(&p.pid));
        // Default order: most notable by either metric (the UI re-sorts on demand).
        procs.sort_by(|a, b| b.cpu.max(b.mem_pct).total_cmp(&a.cpu.max(a.mem_pct)));
        m.procs = procs;

        // ---- GPU (optional; stop probing after the first miss) ----
        if self.probe_gpu {
            let gpu = detect_gpu();
            if gpu.is_none() {
                self.probe_gpu = false;
            }
            m.gpu = gpu;
        }

        // ---- derived heuristics (thresholds come from config) ----
        let (health, why) = derive_system_health(m, &t);
        m.system_health = health;
        m.health_why = why;
        m.storage_health = derive_storage_health(m.disk_pct);
        m.alerts = count_alerts(m, &t);

        // insight strings.
        m.cooling = cooling_label(m.cpu_temp).into();
        m.power = power_label(&m.health).into();
        m.workload = workload_label(m.cpu_load).into();
        m.storage_note = storage_note_label(m.disk_pct).into();
        m.status = overall_status(m.system_health, m.storage_health, m.alerts).into();
        let adv = advisories(m, &t);
        m.advisories = adv;

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

/// Detect a GPU: NVIDIA via `nvidia-smi`, else an AMD/Intel DRM card via sysfs.
/// `None` when nothing usable is found (no GPU section then renders).
fn detect_gpu() -> Option<GpuInfo> {
    read_nvidia_gpu().or_else(read_drm_gpu)
}

/// Query `nvidia-smi` and parse the first GPU's line.
fn read_nvidia_gpu() -> Option<GpuInfo> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,temperature.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_nvidia_smi(text.lines().next().unwrap_or(""))
}

/// Parse one `nvidia-smi` CSV line (`name, util, temp, mem_used, mem_total`).
/// Non-numeric fields (e.g. `[N/A]`) become `None`.
fn parse_nvidia_smi(line: &str) -> Option<GpuInfo> {
    let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if f.len() < 5 || f[0].is_empty() {
        return None;
    }
    let num = |s: &str| s.parse::<f64>().ok();
    Some(GpuInfo {
        name: f[0].to_string(),
        util_pct: num(f[1]),
        temp_c: num(f[2]),
        mem_used_mib: num(f[3]),
        mem_total_mib: num(f[4]),
    })
}

/// AMD/Intel GPU via `/sys/class/drm/card*/device`: requires `gpu_busy_percent`
/// (so we skip dumb framebuffer/BMC chips like `ast`).
fn read_drm_gpu() -> Option<GpuInfo> {
    for n in 0..8 {
        let dev = format!("/sys/class/drm/card{n}/device");
        let busy = read_first_line(&format!("{dev}/gpu_busy_percent"));
        let Some(busy) = busy else { continue };
        let mib = |p: String| {
            read_first_line(&p)
                .and_then(|s| s.parse::<f64>().ok())
                .map(|b| b / 1024.0 / 1024.0)
        };
        let temp = read_gpu_hwmon_temp(&dev);
        let name = drm_driver(n).unwrap_or_else(|| "GPU".to_string());
        return Some(GpuInfo {
            name,
            util_pct: busy.parse::<f64>().ok(),
            temp_c: temp,
            mem_used_mib: mib(format!("{dev}/mem_info_vram_used")),
            mem_total_mib: mib(format!("{dev}/mem_info_vram_total")),
        });
    }
    None
}

/// GPU edge temperature (°C) from any hwmon node under the card. The hwmon
/// index isn't stable across boots, so scan `{dev}/hwmon/*/temp1_input` rather
/// than hard-coding hwmon0/hwmon1.
fn read_gpu_hwmon_temp(dev: &str) -> Option<f64> {
    for entry in fs::read_dir(format!("{dev}/hwmon")).ok()?.flatten() {
        let path = entry.path().join("temp1_input");
        if let Some(milli) = path
            .to_str()
            .and_then(read_first_line)
            .and_then(|s| s.parse::<f64>().ok())
        {
            return Some(milli / 1000.0);
        }
    }
    None
}

/// Read the DRM card's driver name from its `uevent` (`DRIVER=...`).
fn drm_driver(n: u32) -> Option<String> {
    let content = fs::read_to_string(format!("/sys/class/drm/card{n}/device/uevent")).ok()?;
    content
        .lines()
        .find_map(|l| l.strip_prefix("DRIVER="))
        .map(|d| d.to_string())
}

/// Choose the interface to display and rate: the default-route link when known,
/// otherwise the busiest non-loopback link (avoids latching onto a virtual
/// True for loopback interfaces: `lo` (Linux) and `lo0`/`lo1`… (BSD/macOS).
/// Deliberately narrow so real interfaces like `lobby0` aren't hidden.
fn is_loopback(name: &str) -> bool {
    name == "lo"
        || (name.len() > 2
            && name.starts_with("lo")
            && name[2..].bytes().all(|b| b.is_ascii_digit()))
}

/// bridge). Returns (name, bytes received this tick, bytes transmitted this tick).
fn primary_iface(networks: &Networks) -> Option<(String, u64, u64)> {
    // (name, recv_this_tick, sent_this_tick, cumulative_bytes) for real links.
    let mut links: Vec<(String, u64, u64, u64)> = networks
        .iter()
        .filter(|(n, _)| !is_loopback(n.as_str()))
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

/// The interface backing the default route: Linux `/proc/net/route`, macOS
/// `route -n get default`. `None` elsewhere or when there is no default route
/// (the caller then falls back to the busiest interface).
fn default_route_iface() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let content = fs::read_to_string("/proc/net/route").ok()?;
        parse_default_route(&content)
    }
    #[cfg(target_os = "macos")]
    {
        let out = Command::new("route")
            .args(["-n", "get", "default"])
            .output()
            .ok()?;
        parse_macos_route(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Find the iface whose route Destination is `00000000` (0.0.0.0, the default
/// route). Columns are: Iface  Destination  Gateway  Flags ...
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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

/// Parse `route -n get default` (macOS): the line `  interface: en0`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_macos_route(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|l| l.trim().strip_prefix("interface:"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---- pure derivations (unit-tested; tune dashboard thresholds here) ----

/// Cross-platform POWER/HEALTH flags from generally-available signals.
#[allow(clippy::too_many_arguments)]
fn derive_health_flags(
    cpu_temp: f64,
    cpu_freq_mhz: u64,
    cpu_max_freq_mhz: u64,
    load_per_core: f64,
    ram_pct: f64,
    swap_pct: f64,
    t: &Thresholds,
) -> HealthFlags {
    HealthFlags {
        thermal_warn: cpu_temp >= t.temp_warn_c,
        freq_scaled: cpu_max_freq_mhz > 0
            && (cpu_freq_mhz as f64) < t.freq_scaled_ratio * (cpu_max_freq_mhz as f64),
        cpu_pressure: load_per_core > t.load_per_core_high,
        mem_pressure: ram_pct > t.mem_pressure_pct || swap_pct > t.swap_pressure_pct,
    }
}

/// System health score (0..=100) plus the dominant reason string.
/// Reads the already-populated temp/load/cpu fields and `health.mem_pressure`.
fn derive_system_health(m: &Metrics, t: &Thresholds) -> (f64, String) {
    let mut health = 100.0_f64;
    let mut why = "nominal".to_string();
    // Two caution tiers below the configured warning temperature.
    if m.cpu_temp >= t.temp_warn_c {
        health -= 30.0;
        why = format!("temp {:.0}°C", m.cpu_temp);
    } else if m.cpu_temp > t.temp_warn_c - 10.0 {
        health -= 15.0;
        why = format!("temp {:.0}°C", m.cpu_temp);
    } else if m.cpu_temp > t.temp_warn_c - 20.0 {
        health -= 5.0;
    }
    if m.load_per_core > 2.0 * t.load_per_core_high {
        health -= 20.0;
        if why == "nominal" {
            why = format!("load {:.2}", m.load1);
        }
    } else if m.load_per_core > t.load_per_core_high {
        health -= 10.0;
    }
    if m.cpu_load > t.cpu_load_high_pct {
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

/// Count of tripped alert conditions shown in INSIGHTS.
fn count_alerts(m: &Metrics, t: &Thresholds) -> u32 {
    let conds = [
        m.health.thermal_warn,
        m.health.cpu_pressure,
        m.health.mem_pressure,
        m.cpu_load > t.cpu_load_high_pct,
        m.disk_pct > t.disk_full_pct,
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

/// One-word overall verdict from the worst health score and the alert count.
fn overall_status(system_health: f64, storage_health: f64, alerts: u32) -> &'static str {
    let worst = system_health.min(storage_health);
    if worst < 50.0 || alerts >= 3 {
        "Critical"
    } else if worst < 80.0 || alerts >= 1 {
        "Degraded"
    } else {
        "Healthy"
    }
}

/// Human-readable, actionable notes for whatever is currently amiss. Empty when
/// everything is nominal. Reads already-populated `Metrics` fields.
fn advisories(m: &Metrics, t: &Thresholds) -> Vec<String> {
    let mut v = Vec::new();
    if m.health.thermal_warn {
        v.push(format!(
            "Running hot ({:.0}°C) — improve cooling",
            m.cpu_temp
        ));
    }
    if m.cpu_load > t.cpu_load_high_pct || m.health.cpu_pressure {
        v.push(format!("High CPU load ({:.2}/core)", m.load_per_core));
    }
    if m.health.mem_pressure {
        v.push(format!("Memory pressure ({:.0}% RAM)", m.ram_pct));
    }
    if m.swap_pct > 5.0 {
        v.push(format!("Swapping in use ({:.0}%)", m.swap_pct));
    }
    if m.disk_pct > t.disk_full_pct {
        v.push(format!(
            "Disk nearly full ({:.0}%) — free space",
            m.disk_pct
        ));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_loopback_matches_only_real_loopbacks() {
        assert!(is_loopback("lo")); // Linux
        assert!(is_loopback("lo0")); // BSD/macOS
        assert!(is_loopback("lo1"));
        assert!(!is_loopback("lobby0")); // real iface, must not be hidden
        assert!(!is_loopback("local0"));
        assert!(!is_loopback("eth0"));
    }

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
    fn macos_route_parses_interface_line() {
        let sample = "   route to: default\ndestination: default\n       mask: default\n    gateway: 192.168.1.1\n  interface: en0\n      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING>\n";
        assert_eq!(parse_macos_route(sample), Some("en0".to_string()));
        assert_eq!(parse_macos_route(""), None);
        assert_eq!(parse_macos_route("gateway: 1.2.3.4\n"), None);
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
        let t = &Thresholds::default();
        // nominal: cool, freq near max, light load, ample memory.
        let ok = derive_health_flags(50.0, 3000, 3200, 0.3, 40.0, 0.0, t);
        assert!(!ok.thermal_warn && !ok.cpu_pressure && !ok.mem_pressure);
        // 3000/3200 = 93.75% -> not scaled
        assert!(!ok.freq_scaled);

        // thermal warning is inclusive at 80.
        assert!(derive_health_flags(80.0, 3000, 3200, 0.3, 40.0, 0.0, t).thermal_warn);
        assert!(!derive_health_flags(79.9, 3000, 3200, 0.3, 40.0, 0.0, t).thermal_warn);

        // freq scaled when current < 90% of max; unknown max (0) => never scaled.
        assert!(derive_health_flags(50.0, 1000, 3200, 0.3, 40.0, 0.0, t).freq_scaled);
        assert!(!derive_health_flags(50.0, 1000, 0, 0.3, 40.0, 0.0, t).freq_scaled);

        // pressure thresholds.
        assert!(derive_health_flags(50.0, 3000, 3200, 1.01, 40.0, 0.0, t).cpu_pressure);
        assert!(!derive_health_flags(50.0, 3000, 3200, 1.0, 40.0, 0.0, t).cpu_pressure);
        assert!(derive_health_flags(50.0, 3000, 3200, 0.3, 90.0, 0.0, t).mem_pressure);
        assert!(derive_health_flags(50.0, 3000, 3200, 0.3, 40.0, 60.0, t).mem_pressure);
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
        let t = &Thresholds::default();
        let (h, why) = derive_system_health(&metrics_with(45.0, 0.3, 10.0, false), t);
        assert_eq!(h, 100.0);
        assert_eq!(why, "nominal");
    }

    #[test]
    fn system_health_penalises_and_names_dominant_cause() {
        let t = &Thresholds::default();
        // hot CPU dominates the reason string.
        let (h, why) = derive_system_health(&metrics_with(85.0, 0.3, 10.0, false), t);
        assert_eq!(h, 70.0);
        assert!(why.starts_with("temp"));

        // memory pressure names itself when nothing hotter trips.
        let (_h, why) = derive_system_health(&metrics_with(45.0, 0.3, 10.0, true), t);
        assert_eq!(why, "memory pressure");

        // stacked penalties clamp at 0, never negative.
        let (h, _why) = derive_system_health(&metrics_with(95.0, 3.0, 95.0, true), t);
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
        let t = &Thresholds::default();
        let mut m = Metrics::default();
        assert_eq!(count_alerts(&m, t), 0);
        m.health.thermal_warn = true;
        m.cpu_load = 95.0; // > 90
        m.disk_pct = 95.0; // > 90
        assert_eq!(count_alerts(&m, t), 3);
    }

    #[test]
    fn overall_status_tiers() {
        assert_eq!(overall_status(100.0, 100.0, 0), "Healthy");
        // one alert (or a sub-80 score) drops to Degraded
        assert_eq!(overall_status(100.0, 100.0, 1), "Degraded");
        assert_eq!(overall_status(75.0, 100.0, 0), "Degraded");
        // a very low score or 3+ alerts is Critical
        assert_eq!(overall_status(40.0, 100.0, 0), "Critical");
        assert_eq!(overall_status(100.0, 100.0, 3), "Critical");
        // worst-of the two health scores drives it
        assert_eq!(overall_status(100.0, 45.0, 0), "Critical");
    }

    #[test]
    fn advisories_are_actionable_and_empty_when_nominal() {
        let t = &Thresholds::default();
        // nominal system -> no advisories
        let ok = metrics_with(45.0, 0.3, 10.0, false);
        assert!(advisories(&ok, t).is_empty());

        // hot + memory pressure + swapping + full disk -> four specific notes
        let m = Metrics {
            cpu_temp: 85.0,
            ram_pct: 92.0,
            disk_pct: 95.0,
            swap_pct: 12.0,
            health: HealthFlags {
                thermal_warn: true,
                mem_pressure: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let adv = advisories(&m, t);
        assert!(adv.iter().any(|a| a.contains("hot")));
        assert!(adv.iter().any(|a| a.contains("Memory pressure")));
        assert!(adv.iter().any(|a| a.contains("Swapping")));
        assert!(adv.iter().any(|a| a.contains("Disk nearly full")));
    }

    #[test]
    fn nvidia_smi_parses_and_tolerates_na() {
        let g = parse_nvidia_smi("NVIDIA GeForce RTX 3080, 37, 52, 1234, 10240").unwrap();
        assert_eq!(g.name, "NVIDIA GeForce RTX 3080");
        assert_eq!(g.util_pct, Some(37.0));
        assert_eq!(g.temp_c, Some(52.0));
        assert_eq!(g.mem_used_mib, Some(1234.0));
        assert_eq!(g.mem_total_mib, Some(10240.0));

        // [N/A] fields parse to None but the GPU is still reported.
        let g = parse_nvidia_smi("Tesla T4, [N/A], 40, 100, 16000").unwrap();
        assert_eq!(g.util_pct, None);
        assert_eq!(g.temp_c, Some(40.0));

        // garbage / empty -> None
        assert!(parse_nvidia_smi("").is_none());
        assert!(parse_nvidia_smi("only,three,fields").is_none());
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
