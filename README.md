# SystemStat

[![CI](https://github.com/Blevene/systemstat/actions/workflows/ci.yml/badge.svg)](https://github.com/Blevene/systemstat/actions/workflows/ci.yml)

A single-binary terminal system-monitor dashboard: CPU and thermal, memory and
storage, network throughput, a derived power/health panel, and a "doctor insight"
summary — all refreshed once a second.

Cross-platform metrics come from `sysinfo`; richer Linux signals (thermal zone,
cpufreq current/max, `/proc/diskstats`, default route) are read directly when
present, and the POWER/HEALTH flags are derived generically per platform and
degrade gracefully when a signal is unavailable.

## What it looks like

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│SystemStat v2.1 Dashboard                                            Interface: eno1 | Refresh: 1s│
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│CPU / THERMAL                                                                                     │
│CPU Load       90.0 % ████████████████████████████████████████████████████████████  OK            │
│CPU Cores    1:  81%  2:  85%  3:  99%  4:  90%  5:  97%  6:  89%  7:  85%  8:  91%  9:  90%  10: │
│CPU Temp      59.0 °C   OK                                                                        │
│CPU Trend    ▇▆▆█                                                                                 │
│RAM Trend    ████                                                                                 │
│CPU Freq     2599 MHz                                                                             │
│Uptime       6h 50m 51s                                                                           │
│Load Avg     10.90 / 10.21 / 8.54 1/5/15                                                          │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│MEMORY / STORAGE                                                                                  │
│RAM Usage      42.4 % ████████████████████████████████████████████████████████████  OK            │
│Swap Usage      0.0 % ████████████████████████████████████████████████████████████  OK            │
│Disk Usage     79.3 % ████████████████████████████████████████████████████████████  OK            │
│Disk Read    0.00 KiB/s                                                                           │
│Disk Write   5135.95 KiB/s                                                                        │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│NETWORK                                                                                           │
│Sent         59.70 KiB/s                                                                          │
│Received     12.26 KiB/s                                                                          │
│Net Total    71.95 KiB/s                                                                          │
│Net Trend    █▂▂▂                                                                                 │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│POWER / HEALTH                                                                                    │
│Thermal Warn     NO   (59 °C)                                                                     │
│Freq Scaled      YES  (2.60/3.20 GHz)                                                             │
│CPU Pressure     NO   (0.91 /core)                                                                │
│Mem Pressure     NO   (42% RAM)                                                                   │
│Health Trend ████                                                                                 │
│Temp Trend   ████                                                                                 │
│System Health   100 % ████████████████████████████████████████████████████████████  OK            │
│Storage Health    85 % ████████████████████████████████████████████████████████████  OK           │
│Health Why:  nominal                                                                              │
│Stability Avg 100.0 % ████████████████████████████████████████████████████████████  OK            │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│DOCTOR INSIGHT                                                                                    │
│Cooling    Adequate                       Power    Nominal                                        │
│Workload   Heavy                          Storage  Filling                                        │
│System     Intel Xeon E5-2620 v3          Arch     x86_64                                         │
│Total RAM  62.7 GiB                       Alerts   0                                              │
│Top CPU Proc python3 602.8%                                                                       │
│Top RAM Proc python3 1.8%                                                                         │
│Active Alerts none                                                                                │
│                                                                                                  │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

(Live capture on a 12-core x86_64 box; bars and sparklines are colored in a real
terminal. On a Raspberry Pi the model/temperature/frequency figures come from the
Pi's own sensors.)

## Run / build

```sh
cargo run --release      # run locally
cargo build --release    # binary at target/release/systemstat
```

Quit with `q`, `Esc`, or `Ctrl-C`.

## Prebuilt binaries

Each tagged [release](https://github.com/Blevene/systemstat/releases) ships
cross-compiled tarballs for desktop and Raspberry Pi:

| Target | Use |
|--------|-----|
| `x86_64-unknown-linux-gnu` / `-musl` | desktop / server (dynamic / static) |
| `aarch64-unknown-linux-gnu` | 64-bit Raspberry Pi OS |
| `armv7-unknown-linux-gnueabihf` | 32-bit Raspberry Pi OS |

Download the tarball for your target, extract, and run the `systemstat` binary.

## Cross-compile for a Raspberry Pi (64-bit OS)

To build it yourself, the easiest path is `cross` (uses Docker, no toolchain fuss):

```sh
cargo install cross
cross build --release --target aarch64-unknown-linux-gnu
# binary: target/aarch64-unknown-linux-gnu/release/systemstat
scp target/aarch64-unknown-linux-gnu/release/systemstat pi@raspberrypi.local:~
```

For a 32-bit Pi OS use `armv7-unknown-linux-gnueabihf` instead. For a fully static
binary, target the `musl` variants (e.g. `aarch64-unknown-linux-musl`).

## Notes

- The POWER/HEALTH flags (thermal / freq-scaled / CPU pressure / mem pressure)
  and the "health"/"stability" figures and doctor strings are derived heuristics
  (temperature, cpufreq, load, memory, disk). Tune the thresholds in
  `src/metrics.rs::refresh`.
- CPU and network rates need two samples, so the first second after launch may read
  low/zero before stabilizing.
- Truecolor terminals render the olive bar background best; on a 256-color terminal
  it falls back to the nearest color.
- `docs/preview.html` is a static, browser-rendered mock of the dashboard layout —
  handy as a design reference without a terminal.
