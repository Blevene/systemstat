//! SystemStat — a single-binary terminal system-monitor dashboard.
//!
//! Sets up the alternate-screen terminal, then loops: redraw, poll input,
//! and refresh metrics once per second. Quits on `q`, `Esc`, or `Ctrl-C`,
//! and restores the terminal on SIGTERM/SIGINT/SIGHUP.

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

use metrics::Collector;

const REFRESH: Duration = Duration::from_secs(1);
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

fn run(terminal: &mut Term, shutdown: &Arc<AtomicBool>) -> io::Result<()> {
    let mut collector = Collector::new();
    let mut last_refresh = Instant::now();
    collector.refresh();
    let mut scroll: u16 = 0;
    let mut max_scroll: u16 = 0;

    loop {
        // A SIGTERM/SIGINT/SIGHUP sets this; exit so main() can restore the terminal.
        if shutdown.load(Ordering::Relaxed) {
            return Ok(());
        }

        terminal
            .draw(|f| max_scroll = ui::render(f, &collector.metrics, &collector.history, scroll))?;
        scroll = scroll.min(max_scroll);

        // Block at most POLL so the UI stays responsive between refreshes.
        match event::poll(POLL) {
            Ok(true) => {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        let ctrl_c = key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL);
                        if ctrl_c || matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                            return Ok(());
                        }
                        match key.code {
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

        if last_refresh.elapsed() >= REFRESH {
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
    -h, --help           Show this help and exit
    -V, --version        Show version and exit

With no options, runs the interactive dashboard (quit with q/Esc/Ctrl-C).";

enum Mode {
    Interactive,
    Once,
    Json,
}

fn parse_args() -> Mode {
    let mut mode = Mode::Interactive;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--once" | "--snapshot" => mode = Mode::Once,
            "--json" => mode = Mode::Json,
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
    mode
}

/// Non-interactive output: sample twice (so rates are populated) then print.
fn snapshot(json: bool) -> io::Result<()> {
    let mut c = Collector::new();
    c.refresh();
    std::thread::sleep(REFRESH);
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
    match parse_args() {
        Mode::Once => return snapshot(false),
        Mode::Json => return snapshot(true),
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
    let result = run(&mut terminal, &shutdown);
    restore();
    result
}
