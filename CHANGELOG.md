# Changelog

All notable changes to this project are documented here. The format loosely
follows [Keep a Changelog](https://keepachangelog.com/); versions follow
[semantic versioning](https://semver.org/).

## [0.1.0] - 2026-06-04

First feature release since the initial `v0.0.1`: non-interactive output,
runtime configuration, two new interactive views, GPU monitoring, kiosk mode,
packaging, and macOS support.

### Added
- **Non-interactive output:** `--once` prints a single plain-text frame and
  exits; `--json` emits the current metrics as JSON for scripting (`--help`
  lists all flags).
- **Configuration:** tune health thresholds and the refresh interval via
  `~/.config/systemstat/config.toml` (see `config.example.toml`). Unknown keys
  and unreadable/malformed files warn on stderr instead of failing silently;
  values are clamped to sane ranges.
- **Process view:** press `Tab` for a sortable top-process table; `s` toggles
  sorting by CPU or memory.
- **Detail view:** per-mount filesystems and per-interface cumulative
  throughput.
- **GPU panel:** shown automatically when a GPU is detected (NVIDIA via
  `nvidia-smi`, others via DRM/`hwmon`).
- **Kiosk mode:** `--kiosk` ignores `q`/`Esc` for always-on displays; ships a
  sample `systemd` unit (`docs/systemstat.service`).
- **Small-terminal handling:** a minimum-size notice plus scrolling
  (arrows / `k`,`j`, PageUp/PageDown, Home/End).
- **Battery / AC power** and a portable CPU-temperature fallback (Linux).
- **macOS support:** default-route detection, CI on macOS, and darwin release
  builds.
- **Packaging:** Debian `.deb` (via `cargo deb`) and a ready-to-use Homebrew
  formula (`docs/homebrew/systemstat.rb`).
- Render-layer tests via ratatui's `TestBackend`.

### Changed / Fixed
- Restore the terminal cleanly on `SIGTERM`/`SIGINT`/`SIGHUP`.
- `--json` propagates serialization errors instead of panicking.

### Platforms
Prebuilt tarballs for Linux (`x86_64` gnu/musl, `aarch64`, `armv7`) and macOS
(`aarch64`, `x86_64`), plus an `amd64` `.deb`.

## [0.0.1] - 2026-06-02

- Initial release: single-binary terminal system-monitor dashboard (CPU/thermal,
  memory/storage, network, power/health, and a derived insights summary).

[0.1.0]: https://github.com/Blevene/systemstat/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/Blevene/systemstat/releases/tag/v0.0.1
