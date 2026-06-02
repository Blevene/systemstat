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
│SystemStat                                                           Interface: eno1 | Refresh: 1s│
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│CPU / THERMAL                                                                                     │
│CPU Load       26.9 % ████████████████████████████████████████████████████████████  OK            │
│CPU Cores    1:  35%  2:  30%  3:  28%  4:  33%  5:  35%  6:  24%  7:  24%  8:  20%  9:  26%  10: │
│CPU Temp      50.0 °C   OK                                                                        │
│CPU Trend    █▇▇▇                                                                                 │
│RAM Trend    ████                                                                                 │
│CPU Freq     1769 MHz                                                                             │
│Uptime       7h 6m 26s                                                                            │
│Load Avg     2.93 / 5.25 / 7.25 1/5/15                                                            │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│MEMORY / STORAGE                                                                                  │
│RAM Usage      41.2 % ████████████████████████████████████████████████████████████  OK            │
│Swap Usage      0.0 % ████████████████████████████████████████████████████████████  OK            │
│Disk Usage     79.3 % ████████████████████████████████████████████████████████████  OK            │
│Disk Read    0.00 KiB/s                                                                           │
│Disk Write   30.77 KiB/s                                                                          │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│NETWORK                                                                                           │
│Sent         22.66 KiB/s                                                                          │
│Received     12.44 KiB/s                                                                          │
│Net Total    35.10 KiB/s                                                                          │
│Net Trend    █▇▆▆                                                                                 │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│POWER / HEALTH                                                                                    │
│Thermal Warn     NO   (50 °C)                                                                     │
│Freq Scaled      YES  (1.77/3.20 GHz)                                                             │
│CPU Pressure     NO   (0.24 /core)                                                                │
│Mem Pressure     NO   (41% RAM)                                                                   │
│Health Trend ████                                                                                 │
│Temp Trend   ████                                                                                 │
│System Health   100 % ████████████████████████████████████████████████████████████  OK            │
│Storage Health    85 % ████████████████████████████████████████████████████████████  OK           │
│Health Why:  nominal                                                                              │
│Stability Avg 100.0 % ████████████████████████████████████████████████████████████  OK            │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│DOCTOR INSIGHT                                                                                    │
│Cooling    Good                           Power    Nominal                                        │
│Workload   Light                          Storage  Filling                                        │
│System     Intel Xeon E5-2620 v3          Arch     x86_64                                         │
│Total RAM  62.7 GiB                       Alerts   0                                              │
│Top CPU Proc claude 39.9%                                                                         │
│Top RAM Proc JNA Cleaner 1.5%                                                                     │
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
