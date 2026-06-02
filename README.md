# SystemStat

[![CI](https://github.com/Blevene/systemstat/actions/workflows/ci.yml/badge.svg)](https://github.com/Blevene/systemstat/actions/workflows/ci.yml)

A single-binary terminal system-monitor dashboard, à la the SystemStat v2.1 layout:
CPU/thermal, memory/storage, network, power/health, and a "doctor insight" summary.
Cross-platform metrics via `sysinfo`; richer Linux signals (thermal zone, cpufreq
current/max, `/proc/diskstats`) are read directly when present. The POWER/HEALTH
flags are derived generically per platform and degrade gracefully when a signal
is unavailable.

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
