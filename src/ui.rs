//! Rendering. The whole dashboard is built as a single list of styled `Line`s
//! (bars and sparklines drawn as colored block characters) to match the
//! original text-dashboard look and keep layout simple.

use std::collections::VecDeque;

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::metrics::{Collector, Metrics};

const CYAN: Color = Color::Cyan;
const YELLOW: Color = Color::Yellow;
const OLIVE: Color = Color::Rgb(70, 70, 0);
const RED: Color = Color::Red;
const GREEN: Color = Color::Green;
const WHITE: Color = Color::White;
const GRAY: Color = Color::Gray;

const LBL: usize = 13;

pub fn render(f: &mut Frame, c: &Collector) {
    let area = f.size();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GRAY));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let width = inner.width as usize;
    let lines = build(&c.metrics, &c.history, width);
    f.render_widget(Paragraph::new(lines), inner);
}

fn build(m: &Metrics, h: &crate::metrics::History, width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line> = Vec::new();

    // Title bar
    out.push(two_sided(
        "SystemStat",
        &format!("Interface: {} | Refresh: 1s", m.iface),
        width,
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        Style::default().fg(GRAY),
    ));
    out.push(sep(width));

    // CPU / THERMAL
    out.push(header("CPU / THERMAL"));
    out.push(bar_row(
        "CPU Load",
        format!("{:.1} %", m.cpu_load),
        m.cpu_load,
        m.cpu_load < 90.0,
        width,
    ));
    out.push(cores_row(&m.cores));
    out.push(Line::from(vec![
        lbl("CPU Temp"),
        Span::styled(
            format!("{:>8}   ", format!("{:.1} °C", m.cpu_temp)),
            Style::default().fg(WHITE),
        ),
        status(m.cpu_temp < 70.0),
    ]));
    out.push(spark_row("CPU Trend", &h.cpu, width));
    out.push(spark_row("RAM Trend", &h.ram, width));
    out.push(Line::from(vec![
        lbl("CPU Freq"),
        Span::styled(format!("{} MHz", m.cpu_freq_mhz), Style::default().fg(CYAN)),
    ]));
    out.push(Line::from(vec![
        lbl("Uptime"),
        Span::styled(fmt_uptime(m.uptime), Style::default().fg(WHITE)),
    ]));
    out.push(Line::from(vec![
        lbl("Load Avg"),
        Span::styled(
            format!("{:.2} / {:.2} / {:.2} ", m.load1, m.load5, m.load15),
            Style::default().fg(WHITE),
        ),
        Span::styled("1/5/15", Style::default().fg(GRAY)),
    ]));
    out.push(sep(width));

    // MEMORY / STORAGE
    out.push(header("MEMORY / STORAGE"));
    out.push(bar_row(
        "RAM Usage",
        format!("{:.1} %", m.ram_pct),
        m.ram_pct,
        m.ram_pct < 90.0,
        width,
    ));
    out.push(bar_row(
        "Swap Usage",
        format!("{:.1} %", m.swap_pct),
        m.swap_pct,
        m.swap_pct < 80.0,
        width,
    ));
    out.push(bar_row(
        "Disk Usage",
        format!("{:.1} %", m.disk_pct),
        m.disk_pct,
        m.disk_pct < 90.0,
        width,
    ));
    out.push(kv("Disk Read", format!("{:.2} KiB/s", m.disk_read_kib)));
    out.push(kv("Disk Write", format!("{:.2} KiB/s", m.disk_write_kib)));
    out.push(sep(width));

    // NETWORK
    out.push(header("NETWORK"));
    out.push(kv("Sent", format!("{:.2} KiB/s", m.net_sent_kib)));
    out.push(kv("Received", format!("{:.2} KiB/s", m.net_recv_kib)));
    out.push(kv(
        "Net Total",
        format!("{:.2} KiB/s", m.net_sent_kib + m.net_recv_kib),
    ));
    out.push(spark_row("Net Trend", &h.net, width));
    out.push(sep(width));

    // POWER / HEALTH
    out.push(header("POWER / HEALTH"));
    out.push(flag_ctx(
        "Thermal Warn",
        m.health.thermal_warn,
        true,
        format!("{:.0} °C", m.cpu_temp),
    ));
    out.push(flag_ctx(
        "Freq Scaled",
        m.health.freq_scaled,
        false,
        format!(
            "{:.2}/{:.2} GHz",
            m.cpu_freq_mhz as f64 / 1000.0,
            m.cpu_max_freq_mhz as f64 / 1000.0
        ),
    ));
    out.push(flag_ctx(
        "CPU Pressure",
        m.health.cpu_pressure,
        true,
        format!("{:.2} /core", m.load_per_core),
    ));
    out.push(flag_ctx(
        "Mem Pressure",
        m.health.mem_pressure,
        true,
        format!("{:.0}% RAM", m.ram_pct),
    ));
    if let Some(batt) = m.battery {
        out.push(battery_row(batt, m.on_ac));
    }
    out.push(spark_row("Health Trend", &h.health, width));
    out.push(spark_row("Temp Trend", &h.temp, width));
    out.push(bar_row(
        "System Health",
        format!("{:.0} %", m.system_health),
        m.system_health,
        m.system_health >= 80.0,
        width,
    ));
    out.push(bar_row(
        "Storage Health",
        format!("{:.0} %", m.storage_health),
        m.storage_health,
        m.storage_health >= 80.0,
        width,
    ));
    out.push(Line::from(vec![
        Span::styled(
            format!("{:<w$}", "Health Why:", w = LBL),
            Style::default().fg(CYAN),
        ),
        Span::styled(m.health_why.clone(), Style::default().fg(YELLOW)),
    ]));
    out.push(bar_row(
        "Stability Avg",
        format!("{:.1} %", m.stability_avg),
        m.stability_avg,
        m.stability_avg >= 80.0,
        width,
    ));
    out.push(sep(width));

    // INSIGHTS
    out.push(header("INSIGHTS"));
    out.push(status_row(&m.status));
    out.push(doc_row("Cooling", &m.cooling, "Power", &m.power));
    out.push(doc_row("Workload", &m.workload, "Storage", &m.storage_note));
    out.push(doc_row("System", &m.model, "Arch", &m.arch));
    out.push(doc_row(
        "Total RAM",
        &format!("{:.1} GiB", m.total_ram_gib),
        "Alerts",
        &m.alerts.to_string(),
    ));
    out.push(kv(
        "Top CPU Proc",
        format!("{} {:.1}%", m.top_cpu.0, m.top_cpu.1),
    ));
    out.push(kv(
        "Top RAM Proc",
        format!("{} {:.1}%", m.top_ram.0, m.top_ram.1),
    ));
    out.extend(advisory_rows(&m.advisories));

    out
}

// ---- helpers ----

fn lbl(text: &str) -> Span<'static> {
    Span::styled(format!("{:<w$}", text, w = LBL), Style::default().fg(GRAY))
}

fn header(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
    ))
}

fn sep(width: usize) -> Line<'static> {
    Line::from(Span::styled("─".repeat(width), Style::default().fg(GRAY)))
}

fn kv(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        lbl(label),
        Span::styled(value, Style::default().fg(WHITE)),
    ])
}

fn status(ok: bool) -> Span<'static> {
    if ok {
        Span::styled("OK", Style::default().fg(GREEN))
    } else {
        Span::styled(
            "WARN",
            Style::default().fg(RED).add_modifier(Modifier::BOLD),
        )
    }
}

/// A YES/NO health flag with a gray context note. `warn` selects whether the
/// "on" state is alarming (red) or merely informational (cyan, e.g. freq scaling).
fn flag_ctx(label: &str, on: bool, warn: bool, ctx: String) -> Line<'static> {
    let (txt, col) = if on {
        ("YES", if warn { RED } else { CYAN })
    } else {
        ("NO", WHITE)
    };
    Line::from(vec![
        lbl(label),
        Span::styled(
            format!("    {:<5}", txt),
            Style::default().fg(col).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("({})", ctx), Style::default().fg(GRAY)),
    ])
}

fn cores_row(cores: &[f64]) -> Line<'static> {
    let mut spans = vec![lbl("CPU Cores")];
    for (i, c) in cores.iter().enumerate() {
        spans.push(Span::styled(
            format!("{}: ", i + 1),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!("{:>3.0}%  ", c),
            Style::default().fg(WHITE),
        ));
    }
    Line::from(spans)
}

fn bar_cells(pct: f64, width: usize) -> Vec<Span<'static>> {
    let pct = pct.clamp(0.0, 100.0);
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    vec![
        Span::styled("█".repeat(filled), Style::default().fg(YELLOW)),
        Span::styled("█".repeat(width - filled), Style::default().fg(OLIVE)),
    ]
}

fn bar_row(label: &str, value: String, pct: f64, ok: bool, width: usize) -> Line<'static> {
    let bar_w = (width as isize - 32).clamp(10, 60) as usize;
    let mut spans = vec![
        lbl(label),
        Span::styled(format!("{:>8} ", value), Style::default().fg(WHITE)),
    ];
    spans.extend(bar_cells(pct, bar_w));
    spans.push(Span::raw("  "));
    spans.push(status(ok));
    Line::from(spans)
}

fn spark(data: &VecDeque<f64>, width: usize) -> Span<'static> {
    const T: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if data.is_empty() {
        return Span::raw("");
    }
    let slice: Vec<f64> = data.iter().rev().take(width).rev().cloned().collect();
    let maxv = slice.iter().cloned().fold(0.0_f64, f64::max).max(1.0);
    let s: String = slice
        .iter()
        .map(|v| {
            let idx = ((v / maxv) * 7.0).round() as usize;
            T[idx.min(7)]
        })
        .collect();
    Span::styled(s, Style::default().fg(RED))
}

fn spark_row(label: &str, data: &VecDeque<f64>, width: usize) -> Line<'static> {
    let sw = (width as isize - (LBL as isize) - 2).clamp(10, 90) as usize;
    Line::from(vec![lbl(label), spark(data, sw)])
}

/// Truncate to at most `max` display chars, marking elision with `…`.
fn truncate_fit(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max == 0 {
        String::new()
    } else {
        let kept: String = s.chars().take(max - 1).collect();
        format!("{kept}…")
    }
}

fn doc_row(llabel: &str, lval: &str, rlabel: &str, rval: &str) -> Line<'static> {
    const COL: usize = 42;
    // Keep at least one space before the right label, whatever the value length.
    let lval = truncate_fit(lval, COL - 11 - 1);
    let left_len = 11 + lval.chars().count();
    let pad = COL.saturating_sub(left_len);
    Line::from(vec![
        Span::styled(format!("{:<11}", llabel), Style::default().fg(GRAY)),
        Span::styled(lval, Style::default().fg(WHITE)),
        Span::raw(" ".repeat(pad)),
        Span::styled(format!("{:<9}", rlabel), Style::default().fg(GRAY)),
        Span::styled(rval.to_string(), Style::default().fg(WHITE)),
    ])
}

/// Battery charge + power source. Color reflects charge level; only shown when
/// the host actually has a battery.
fn battery_row(pct: f64, on_ac: bool) -> Line<'static> {
    let col = if pct < 20.0 {
        RED
    } else if pct < 50.0 {
        YELLOW
    } else {
        GREEN
    };
    let source = if on_ac { "AC" } else { "battery" };
    Line::from(vec![
        lbl("Battery"),
        Span::styled(
            format!("{:>5} ", format!("{pct:.0}%")),
            Style::default().fg(col).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("on {source}"), Style::default().fg(GRAY)),
    ])
}

/// Overall verdict, colored by severity.
fn status_row(status: &str) -> Line<'static> {
    let col = match status {
        "Healthy" => GREEN,
        "Degraded" => YELLOW,
        _ => RED,
    };
    Line::from(vec![
        lbl("Status"),
        Span::styled(
            status.to_string(),
            Style::default().fg(col).add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Render the advisories list — a "nominal" line when clear, else one bullet each.
fn advisory_rows(advisories: &[String]) -> Vec<Line<'static>> {
    if advisories.is_empty() {
        return vec![Line::from(vec![
            lbl("Advisories"),
            Span::styled("nominal", Style::default().fg(GREEN)),
        ])];
    }
    let mut rows = vec![Line::from(Span::styled(
        "Advisories",
        Style::default().fg(GRAY),
    ))];
    for a in advisories {
        rows.push(Line::from(vec![
            Span::styled("  • ", Style::default().fg(RED)),
            Span::styled(a.clone(), Style::default().fg(YELLOW)),
        ]));
    }
    rows
}

fn two_sided(left: &str, right: &str, width: usize, ls: Style, rs: Style) -> Line<'static> {
    let used = left.chars().count() + right.chars().count();
    let pad = width.saturating_sub(used);
    Line::from(vec![
        Span::styled(left.to_string(), ls),
        Span::raw(" ".repeat(pad)),
        Span::styled(right.to_string(), rs),
    ])
}

fn fmt_uptime(s: u64) -> String {
    let d = s / 86400;
    let h = (s % 86400) / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if d > 0 {
        format!("{}d {}h {}m {}s", d, h, m, sec)
    } else {
        format!("{}h {}m {}s", h, m, sec)
    }
}
