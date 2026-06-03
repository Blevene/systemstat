//! Optional configuration loaded from `~/.config/systemstat/config.toml`.
//!
//! A missing or partial file falls back to defaults, so the dashboard runs the
//! same with no config at all. Thresholds feed the pure derivations in
//! `metrics.rs`, removing the need to recompile to tune them.

use std::path::PathBuf;

use serde::Deserialize;

// Default values, kept in one place: both the `Default` impls and serde's
// per-field `default = "..."` reference these. Per-field (rather than
// container-level `#[serde(default)]`) defaults are required so the structs can
// also use `deny_unknown_fields`, which rejects typo'd/unknown keys instead of
// silently ignoring them.
fn default_refresh_secs() -> u64 {
    1
}
fn default_temp_warn_c() -> f64 {
    80.0
}
fn default_freq_scaled_ratio() -> f64 {
    0.9
}
fn default_load_per_core_high() -> f64 {
    1.0
}
fn default_mem_pressure_pct() -> f64 {
    85.0
}
fn default_swap_pressure_pct() -> f64 {
    50.0
}
fn default_cpu_load_high_pct() -> f64 {
    90.0
}
fn default_disk_full_pct() -> f64 {
    90.0
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Refresh interval in seconds (>= 1).
    #[serde(default = "default_refresh_secs")]
    pub refresh_secs: u64,
    #[serde(default)]
    pub thresholds: Thresholds,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            refresh_secs: default_refresh_secs(),
            thresholds: Thresholds::default(),
        }
    }
}

/// Tunable thresholds for the health/flag/advisory derivations. Defaults match
/// the values previously hard-coded in `metrics.rs`. Unknown keys are rejected
/// (see `deny_unknown_fields`) so a typo surfaces as a warning rather than being
/// silently dropped.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    /// CPU temperature (°C) at/above which `thermal_warn` trips.
    #[serde(default = "default_temp_warn_c")]
    pub temp_warn_c: f64,
    /// Current freq below this fraction of max trips `freq_scaled`.
    #[serde(default = "default_freq_scaled_ratio")]
    pub freq_scaled_ratio: f64,
    /// Load average per core above which `cpu_pressure` trips.
    #[serde(default = "default_load_per_core_high")]
    pub load_per_core_high: f64,
    /// RAM % above which `mem_pressure` trips.
    #[serde(default = "default_mem_pressure_pct")]
    pub mem_pressure_pct: f64,
    /// Swap % above which `mem_pressure` trips.
    #[serde(default = "default_swap_pressure_pct")]
    pub swap_pressure_pct: f64,
    /// CPU load % considered "high" for health/alerts.
    #[serde(default = "default_cpu_load_high_pct")]
    pub cpu_load_high_pct: f64,
    /// Disk % considered "nearly full" for alerts/advisories.
    #[serde(default = "default_disk_full_pct")]
    pub disk_full_pct: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            temp_warn_c: default_temp_warn_c(),
            freq_scaled_ratio: default_freq_scaled_ratio(),
            load_per_core_high: default_load_per_core_high(),
            mem_pressure_pct: default_mem_pressure_pct(),
            swap_pressure_pct: default_swap_pressure_pct(),
            cpu_load_high_pct: default_cpu_load_high_pct(),
            disk_full_pct: default_disk_full_pct(),
        }
    }
}

impl Thresholds {
    /// Clamp values into sane ranges: percentages to 0..=100, the freq ratio to
    /// 0..=1, and the rest to non-negative. Keeps a malformed config from
    /// silently producing flags that always (or never) trip. `temp_warn_c` has a
    /// 30°C floor so the derived caution tiers (`temp_warn_c - 10/-20`) stay
    /// non-negative.
    fn sanitise(&mut self) {
        self.temp_warn_c = self.temp_warn_c.clamp(30.0, 150.0);
        self.freq_scaled_ratio = self.freq_scaled_ratio.clamp(0.0, 1.0);
        self.load_per_core_high = self.load_per_core_high.max(0.0);
        self.mem_pressure_pct = self.mem_pressure_pct.clamp(0.0, 100.0);
        self.swap_pressure_pct = self.swap_pressure_pct.clamp(0.0, 100.0);
        self.cpu_load_high_pct = self.cpu_load_high_pct.clamp(0.0, 100.0);
        self.disk_full_pct = self.disk_full_pct.clamp(0.0, 100.0);
    }
}

impl Config {
    /// Load from the standard path, falling back to defaults. A missing file is
    /// silent (the no-config case); a file that exists but can't be read or
    /// parsed warns on stderr so a typo isn't silently ignored. Called before
    /// terminal setup, so the warning is visible.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            // Not found is the normal "no config" case; anything else is worth a warning.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!(
                    "systemstat: cannot read {} ({e}); using defaults",
                    path.display()
                );
                return Self::default();
            }
        };
        Self::parse(&text).unwrap_or_else(|| {
            eprintln!(
                "systemstat: ignoring malformed config at {}; using defaults",
                path.display()
            );
            Self::default()
        })
    }

    /// Parse TOML into a `Config` (separated for testing). Values are sanitised
    /// into sensible ranges so a stray config can't produce nonsense flags.
    pub fn parse(text: &str) -> Option<Self> {
        let mut cfg: Config = toml::from_str(text).ok()?;
        cfg.refresh_secs = cfg.refresh_secs.max(1);
        cfg.thresholds.sanitise();
        Some(cfg)
    }

    fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("systemstat").join("config.toml"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_legacy_constants() {
        let t = Thresholds::default();
        assert_eq!(t.temp_warn_c, 80.0);
        assert_eq!(t.mem_pressure_pct, 85.0);
        assert_eq!(Config::default().refresh_secs, 1);
    }

    #[test]
    fn partial_config_overrides_only_named_fields() {
        let cfg = Config::parse("refresh_secs = 3\n[thresholds]\ntemp_warn_c = 70.0\n").unwrap();
        assert_eq!(cfg.refresh_secs, 3);
        assert_eq!(cfg.thresholds.temp_warn_c, 70.0); // overridden
        assert_eq!(cfg.thresholds.mem_pressure_pct, 85.0); // still default
    }

    #[test]
    fn refresh_secs_floored_at_one() {
        let cfg = Config::parse("refresh_secs = 0\n").unwrap();
        assert_eq!(cfg.refresh_secs, 1);
    }

    #[test]
    fn empty_config_is_all_defaults() {
        let cfg = Config::parse("").unwrap();
        assert_eq!(cfg.refresh_secs, 1);
        assert_eq!(cfg.thresholds.disk_full_pct, 90.0);
    }

    #[test]
    fn garbage_config_returns_none() {
        assert!(Config::parse("this is not = = valid toml [[[").is_none());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        // A typo'd threshold key (real one is `temp_warn_c`) must error, not be
        // silently dropped — that's what triggers the "malformed config" warning.
        assert!(Config::parse("[thresholds]\ntemp_warn = 80\n").is_none());
        // An unknown top-level key is also rejected.
        assert!(Config::parse("refresh_sec = 5\n").is_none());
    }

    #[test]
    fn out_of_range_thresholds_are_clamped() {
        let cfg = Config::parse(
            "[thresholds]\nfreq_scaled_ratio = 5.0\nmem_pressure_pct = 250.0\nswap_pressure_pct = -10.0\n",
        )
        .unwrap();
        assert_eq!(cfg.thresholds.freq_scaled_ratio, 1.0);
        assert_eq!(cfg.thresholds.mem_pressure_pct, 100.0);
        assert_eq!(cfg.thresholds.swap_pressure_pct, 0.0);
    }
}
