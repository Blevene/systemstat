//! Optional configuration loaded from `~/.config/systemstat/config.toml`.
//!
//! A missing or partial file falls back to defaults, so the dashboard runs the
//! same with no config at all. Thresholds feed the pure derivations in
//! `metrics.rs`, removing the need to recompile to tune them.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Refresh interval in seconds (>= 1).
    pub refresh_secs: u64,
    pub thresholds: Thresholds,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            refresh_secs: 1,
            thresholds: Thresholds::default(),
        }
    }
}

/// Tunable thresholds for the health/flag/advisory derivations. Defaults match
/// the values previously hard-coded in `metrics.rs`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct Thresholds {
    /// CPU temperature (°C) at/above which `thermal_warn` trips.
    pub temp_warn_c: f64,
    /// Current freq below this fraction of max trips `freq_scaled`.
    pub freq_scaled_ratio: f64,
    /// Load average per core above which `cpu_pressure` trips.
    pub load_per_core_high: f64,
    /// RAM % above which `mem_pressure` trips.
    pub mem_pressure_pct: f64,
    /// Swap % above which `mem_pressure` trips.
    pub swap_pressure_pct: f64,
    /// CPU load % considered "high" for health/alerts.
    pub cpu_load_high_pct: f64,
    /// Disk % considered "nearly full" for alerts/advisories.
    pub disk_full_pct: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            temp_warn_c: 80.0,
            freq_scaled_ratio: 0.9,
            load_per_core_high: 1.0,
            mem_pressure_pct: 85.0,
            swap_pressure_pct: 50.0,
            cpu_load_high_pct: 90.0,
            disk_full_pct: 90.0,
        }
    }
}

impl Config {
    /// Load from the standard path, falling back to defaults on any problem.
    pub fn load() -> Self {
        match Self::path().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => Self::parse(&text).unwrap_or_default(),
            None => Self::default(),
        }
    }

    /// Parse TOML into a `Config` (separated for testing).
    pub fn parse(text: &str) -> Option<Self> {
        let mut cfg: Config = toml::from_str(text).ok()?;
        cfg.refresh_secs = cfg.refresh_secs.max(1);
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
}
