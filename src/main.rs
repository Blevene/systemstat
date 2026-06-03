//! SystemStat — a single-binary terminal system-monitor dashboard.
//!
//! Sets up the alternate-screen terminal, then loops: redraw, poll input,
//! and refresh metrics once per second. Quits on `q`, `Esc`, or `Ctrl-C`.

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

fn run(terminal: &mut Term, shutdown: &Arc<AtomicBool>) -> io::Result<()> {
    let mut collector = Collector::new();
    let mut last_refresh = Instant::now();
    collector.refresh();

    loop {
        // A SIGTERM/SIGINT/SIGHUP sets this; exit so main() can restore the terminal.
        if shutdown.load(Ordering::Relaxed) {
            return Ok(());
        }

        terminal.draw(|f| ui::render(f, &collector))?;

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
                    }
                }
            }
            Ok(false) => {}
            // A signal interrupts the poll syscall (EINTR); treat as a clean exit.
            Err(_) if shutdown.load(Ordering::Relaxed) => return Ok(()),
            Err(e) => return Err(e),
        }

        if last_refresh.elapsed() >= REFRESH {
            collector.refresh();
            last_refresh = Instant::now();
        }
    }
}

fn main() -> io::Result<()> {
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
