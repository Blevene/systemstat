//! SystemStat — a single-binary terminal system-monitor dashboard.
//!
//! Sets up the alternate-screen terminal, then loops: redraw, poll input,
//! and refresh metrics once per second. Quits on `q`, `Esc`, or `Ctrl-C`,
//! and restores the terminal on SIGTERM/SIGINT/SIGHUP.

mod config;
mod metrics;
mod ui;

use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

use config::Config;
use metrics::Collector;

const POLL: Duration = Duration::from_millis(250);

type Term = Terminal<CrosstermBackend<Stdout>>;

fn setup() -> io::Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

/// Best-effort restore; safe to call more than once and from a panic hook.
fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

/// Rows scrolled per PageUp/PageDown — roughly one dashboard section.
const PAGE: u16 = 10;

fn run(
    terminal: &mut Term,
    shutdown: &Arc<AtomicBool>,
    config: &Config,
    kiosk: bool,
) -> io::Result<()> {
    let refresh = Duration::from_secs(config.refresh_secs);
    let mut collector = Collector::with_thresholds(config.thresholds);
    let mut last_refresh = Instant::now();
    collector.refresh();
    let mut scroll: u16 = 0;
    let mut max_scroll: u16 = 0;
    let mut view = ui::View::Dashboard;

    loop {
        // A SIGTERM/SIGINT/SIGHUP sets this; exit so main() can restore the terminal.
        if shutdown.load(Ordering::Relaxed) {
            return Ok(());
        }

        terminal.draw(|f| {
            max_scroll = ui::render(f, &collector.metrics, &collector.history, scroll, view)
        })?;
        scroll = scroll.min(max_scroll);

        // Block at most POLL so the UI stays responsive between refreshes.
        match event::poll(POLL) {
            Ok(true) => {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        let ctrl_c = key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL);
                        // Kiosk mode ignores q/Esc; exit only via Ctrl-C or a signal.
                        let quit_key = matches!(key.code, KeyCode::Char('q') | KeyCode::Esc);
                        if ctrl_c || (!kiosk && quit_key) {
                            return Ok(());
                        }
                        match key.code {
                            // Tab cycles dashboard -> processes -> detail; reset scroll.
                            KeyCode::Tab => {
                                view = match view {
                                    ui::View::Dashboard => ui::View::Processes(ui::Sort::Cpu),
                                    ui::View::Processes(_) => ui::View::Detail,
                                    ui::View::Detail => ui::View::Dashboard,
                                };
                                scroll = 0;
                            }
                            // 's' flips the process-view sort key.
                            KeyCode::Char('s') => {
                                if let ui::View::Processes(sort) = view {
                                    let next = match sort {
                                        ui::Sort::Cpu => ui::Sort::Mem,
                                        ui::Sort::Mem => ui::Sort::Cpu,
                                    };
                                    view = ui::View::Processes(next);
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') => scroll = scroll.saturating_sub(1),
                            KeyCode::Down | KeyCode::Char('j') => {
                                scroll = scroll.saturating_add(1).min(max_scroll)
                            }
                            KeyCode::PageUp => scroll = scroll.saturating_sub(PAGE),
                            KeyCode::PageDown => {
                                scroll = scroll.saturating_add(PAGE).min(max_scroll)
                            }
                            KeyCode::Home => scroll = 0,
                            KeyCode::End => scroll = max_scroll,
                            _ => {}
                        }
                    }
                }
            }
            Ok(false) => {}
            // Defensive: crossterm 0.27 retries on EINTR internally and reports a
            // Ok(false) timeout rather than an error when a signal fires, so the
            // top-of-loop flag check is the real exit path (≤POLL latency). This arm
            // only matters if a future crossterm surfaces EINTR as an error.
            Err(_) if shutdown.load(Ordering::Relaxed) => return Ok(()),
            Err(e) => return Err(e),
        }

        if last_refresh.elapsed() >= refresh {
            collector.refresh();
            last_refresh = Instant::now();
        }
    }
}

const USAGE: &str = "\
systemstat — terminal system-monitor dashboard

USAGE:
    systemstat [OPTIONS]

OPTIONS:
    --once, --snapshot   Print one plain-text frame and exit
    --json               Print the current metrics as JSON and exit
    --kiosk              Always-on mode: ignore q/Esc (exit only via Ctrl-C or a
                         signal). Intended for a dedicated always-on display.
    -h, --help           Show this help and exit
    -V, --version        Show version and exit

With no options, runs the interactive dashboard.

KEYS (interactive):
    q, Esc, Ctrl-C       Quit
    Tab                  Toggle dashboard / process list
    s                    (process list) sort by CPU / memory
    Up/Down, j/k         Scroll
    PageUp/PageDown      Scroll a page
    Home/End             Top / bottom";

enum Mode {
    Interactive,
    Once,
    Json,
}

struct Args {
    mode: Mode,
    kiosk: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        mode: Mode::Interactive,
        kiosk: false,
    };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--once" | "--snapshot" => args.mode = Mode::Once,
            "--json" => args.mode = Mode::Json,
            "--kiosk" => args.kiosk = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("systemstat {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => {
                eprintln!("error: unknown argument '{other}'\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }
    args
}

/// Non-interactive output: sample twice (so rates are populated) then print.
fn snapshot(json: bool, config: &Config) -> io::Result<()> {
    let mut c = Collector::with_thresholds(config.thresholds);
    c.refresh();
    std::thread::sleep(Duration::from_secs(config.refresh_secs));
    c.refresh();
    if json {
        // Propagate rather than panic: a NaN/Infinity float would make
        // serde_json fail, and --json is meant for scripts.
        let out = serde_json::to_string_pretty(&c.metrics).map_err(io::Error::other)?;
        println!("{out}");
    } else {
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(100);
        println!("{}", ui::snapshot(&c.metrics, &c.history, width));
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let args = parse_args();
    let config = Config::load();
    match args.mode {
        Mode::Once => return snapshot(false, &config),
        Mode::Json => return snapshot(true, &config),
        Mode::Interactive => {}
    }

    // Restore the terminal even if rendering or collection panics.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        default_hook(info);
    }));

    // Restore the terminal on SIGTERM/SIGINT/SIGHUP (e.g. `systemctl stop`, `kill`).
    // The handler only flips a flag; the run loop polls it and exits cleanly.
    let shutdown = Arc::new(AtomicBool::new(false));
    for sig in [SIGTERM, SIGINT, SIGHUP] {
        signal_hook::flag::register(sig, Arc::clone(&shutdown))?;
    }

    let mut terminal = setup()?;
    let result = run(&mut terminal, &shutdown, &config, args.kiosk);
    restore();
    result
}
