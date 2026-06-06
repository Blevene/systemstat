# SystemStat

[![CI](https://github.com/Blevene/systemstat/actions/workflows/ci.yml/badge.svg)](https://github.com/Blevene/systemstat/actions/workflows/ci.yml)

A single-binary terminal system-monitor dashboard: CPU and thermal, memory and
storage, network throughput, a derived power/health panel, an optional GPU panel
(shown when a GPU is detected), and an "insights" summary (overall status,
advisories, top processes) — refreshed once a second by default. Press `Tab` to
cycle to a sortable process list and a per-mount / per-interface detail view.

Cross-platform metrics come from `sysinfo` (plus a portable CPU-temperature
fallback and, on Linux, battery / AC state from `systemstat`); richer Linux
signals (thermal zone, cpufreq
current/max, `/proc/diskstats`) are read directly when present. On macOS the
default route comes from `route`, and the POWER/HEALTH flags are derived
generically per platform — anything unavailable degrades gracefully. CI builds
and tests on both Linux and macOS.

## What it looks like

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│SystemStat                                            Interface: eno1 | Tab: processes / detail│
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│CPU / THERMAL                                                                                     │
│CPU Load       22.1 % ████████████████████████████████████████████████████████████  OK            │
│CPU Cores    1:  25%  2:  22%  3:  33%  4:  30%  5:  23%  6:  29%  7:  15%  8:  12%  9:  20%  10: │
│CPU Temp      51.0 °C   OK                                                                        │
│CPU Trend    █▅▄▃                                                                                 │
│RAM Trend    ████                                                                                 │
│CPU Freq     2140 MHz                                                                             │
│Uptime       7h 58m 41s                                                                           │
│Load Avg     4.95 / 3.61 / 3.32 1/5/15                                                            │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│MEMORY / STORAGE                                                                                  │
│RAM Usage      42.2 % ████████████████████████████████████████████████████████████  OK            │
│Swap Usage      0.0 % ████████████████████████████████████████████████████████████  OK            │
│Disk Usage     82.2 % ████████████████████████████████████████████████████████████  OK            │
│Disk Read    0.00 KiB/s                                                                           │
│Disk Write   0.00 KiB/s                                                                           │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│NETWORK                                                                                           │
│Sent         909.57 KiB/s                                                                         │
│Received     17.01 KiB/s                                                                          │
│Net Total    926.58 KiB/s                                                                         │
│Net Trend    ▁█▆▆                                                                                 │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│POWER / HEALTH                                                                                    │
│Thermal Warn     NO   (51 °C)                                                                     │
│Freq Scaled      YES  (2.14/3.20 GHz)                                                             │
│CPU Pressure     NO   (0.41 /core)                                                                │
│Mem Pressure     NO   (42% RAM)                                                                   │
│Health Trend ████                                                                                 │
│Temp Trend   ████                                                                                 │
│System Health   100 % ████████████████████████████████████████████████████████████  OK            │
│Storage Health    81 % ████████████████████████████████████████████████████████████  OK           │
│Health Why:  nominal                                                                              │
│Stability Avg 100.0 % ████████████████████████████████████████████████████████████  OK            │
│──────────────────────────────────────────────────────────────────────────────────────────────────│
│INSIGHTS                                                                                          │
│Status       Healthy                                                                              │
│Cooling    Good                           Power    Nominal                                        │
│Workload   Light                          Storage  Filling                                        │
│System     Intel Xeon E5-2620 v3          Arch     x86_64                                         │
│Total RAM  62.7 GiB                       Alerts   0                                              │
│Top CPU Proc claude 40.0%                                                                         │
│Top RAM Proc opensearch[438d 1.5%                                                                 │
│Advisories   nominal                                                                              │
│                                                                                                  │
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

### Keys

| Key | Action |
|-----|--------|
| `q` / `Esc` / `Ctrl-C` | Quit |
| `Tab` | Cycle views: dashboard → processes → detail |
| `s` | In the process view, toggle sort by CPU / memory |
| `↑` `↓` (or `k` `j`), `PageUp` `PageDown`, `Home` `End` | Scroll the current view |

- **Processes** — the top processes, sortable by CPU or memory.
- **Detail** — every mounted filesystem and every (non-loopback) interface with
  cumulative throughput.

Non-interactive output is also available: `systemstat --once` prints one
plain-text frame, and `systemstat --json` prints the current metrics as JSON
(handy for scripting). `--help` lists all flags.

## Prebuilt binaries

Each tagged [release](https://github.com/Blevene/systemstat/releases) ships
cross-compiled tarballs for desktop and Raspberry Pi:

| Target | Use |
|--------|-----|
| `x86_64-unknown-linux-gnu` / `-musl` | desktop / server (dynamic / static) |
| `aarch64-unknown-linux-gnu` | 64-bit Raspberry Pi OS |
| `armv7-unknown-linux-gnueabihf` | 32-bit Raspberry Pi OS |
| `aarch64-apple-darwin` | Apple Silicon macOS |
| `x86_64-apple-darwin` | Intel macOS |

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

## Packaging

- **Debian/Ubuntu (`.deb`):** `cargo install cargo-deb` then `cargo deb` builds a
  package that installs the `systemstat` binary plus docs (the example config and
  systemd unit). Tagged releases also attach a prebuilt `.deb`.
- **Homebrew (macOS):** `docs/homebrew/systemstat.rb` is a ready-to-use formula
  pinned to the latest darwin release (URLs + checksums filled in). Drop it into a
  tap (e.g. `Blevene/homebrew-tap`) and install with
  `brew install blevene/tap/systemstat`.

## License

MIT — see `LICENSE`. (Defaulted; change if you prefer another license.)

## Always-on / kiosk mode

`systemstat --kiosk` runs the dashboard but ignores `q`/`Esc`, so a dedicated
display won't exit on a stray keypress (exit via `Ctrl-C` or `SIGTERM`). A sample
systemd unit that drives a console (e.g. `tty1`) is in `docs/systemstat.service`;
stopping the service sends `SIGTERM`, which the dashboard handles to restore the
console.

## Notes

- INSIGHTS (overall status + advisories), the POWER/HEALTH flags (thermal /
  freq-scaled / CPU pressure / mem pressure), and the health/stability figures are
  derived heuristics (temperature, cpufreq, load, memory, disk).
- Thresholds and the refresh interval are configurable via
  `~/.config/systemstat/config.toml` — see `config.example.toml`. With no file
  the dashboard runs on defaults, exactly as before.
- CPU and network rates need two samples, so the first second after launch may read
  low/zero before stabilizing.
- Truecolor terminals render the olive bar background best; on a 256-color terminal
  it falls back to the nearest color.
- `docs/preview.html` is a static, browser-rendered mock of the dashboard layout —
  handy as a design reference without a terminal.
