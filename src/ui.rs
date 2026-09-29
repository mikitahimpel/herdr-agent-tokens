use std::collections::HashMap;

use chrono::Local;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Bar, BarChart, BarGroup, Block, BorderType, Cell, Chart, Dataset, GraphType, LineGauge, List, ListItem,
    Paragraph, Row, Table, Tabs,
};
use ratatui::Frame;

use crate::usage::{ratio, Report, Totals};
use crate::worker::{Usage, REFRESH};
use crate::{App, TABS};

pub(crate) const TEXT: Color = Color::Rgb(205, 214, 244);
pub(crate) const SUBTEXT: Color = Color::Rgb(166, 173, 200);
pub(crate) const MUTED: Color = Color::Rgb(108, 112, 134);
pub(crate) const SURFACE: Color = Color::Rgb(49, 50, 68);
pub(crate) const SELECTED: Color = Color::Rgb(40, 42, 58);
pub(crate) const MAUVE: Color = Color::Rgb(203, 166, 247);
pub(crate) const BLUE: Color = Color::Rgb(137, 180, 250);
pub(crate) const SAPPHIRE: Color = Color::Rgb(116, 199, 236);
pub(crate) const TEAL: Color = Color::Rgb(148, 226, 213);
pub(crate) const GREEN: Color = Color::Rgb(166, 227, 161);
pub(crate) const YELLOW: Color = Color::Rgb(249, 226, 175);
pub(crate) const PEACH: Color = Color::Rgb(250, 179, 135);
pub(crate) const RED: Color = Color::Rgb(243, 139, 168);

const MIX: [(&str, Color); 4] = [("input", PEACH), ("cache write", MAUVE), ("cache read", TEAL), ("output", BLUE)];

pub fn compact(count: u64) -> String {
    let n = count as f64;
    match count {
        1_000_000_000.. => format!("{:.2}B", n / 1e9),
        100_000_000.. => format!("{:.0}M", n / 1e6),
        10_000_000.. => format!("{:.1}M", n / 1e6),
        1_000_000.. => format!("{:.2}M", n / 1e6),
        10_000.. => format!("{:.0}k", n / 1e3),
        1_000.. => format!("{:.1}k", n / 1e3),
        _ => count.to_string(),
    }
}

fn percent(value: f64) -> String {
    format!("{:.0}%", value * 100.0)
}

fn load_color(value: f64) -> Color {
    if value < 0.6 {
        GREEN
    } else if value < 0.85 {
        YELLOW
    } else {
        RED
    }
}

pub(crate) fn status_color(status: &str) -> Color {
    match status {
        "working" => YELLOW,
        "idle" => GREEN,
        "done" => BLUE,
        "blocked" => RED,
        _ => MUTED,
    }
}

fn panel(title: &str) -> Block<'_> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(SURFACE))
        .title(Line::from(format!(" {title} ")).fg(SUBTEXT))
}

fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else {
        let mut clipped: String = text.chars().take(width.saturating_sub(1)).collect();
        clipped.push('…');
        clipped
    }
}

fn duration(report: &Report) -> String {
    let (Some(start), Some(end)) = (report.started, report.updated) else { return "—".into() };
    let minutes = (end - start).num_minutes().max(0);
    match (minutes / 1440, minutes / 60 % 24, minutes % 60) {
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h {m:02}m"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

fn centered(frame: &mut Frame, area: Rect, lines: Vec<Line>) {
    let height = lines.len() as u16;
    let [_, middle, _] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(height), Constraint::Fill(1)]).areas(area);
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), middle);
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [header, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
    draw_header(frame, header, app);
    draw_footer(frame, footer);

    let Some(snapshot) = app.snapshot.as_ref() else {
        centered(frame, body, vec![Line::from("Loading agents…").fg(SUBTEXT)]);
        return;
    };
    if let Some(error) = &snapshot.error {
        centered(frame, body, vec![Line::from(error.as_str()).fg(RED)]);
        return;
    }
    if snapshot.agents.is_empty() {
        centered(frame, body, vec![Line::from("No agents are running in Herdr.").fg(SUBTEXT)]);
        return;
    }

    let [list_area, detail_area] = Layout::horizontal([Constraint::Length(34), Constraint::Fill(1)]).areas(body);
    draw_agents(frame, list_area, app);
    draw_detail(frame, detail_area, app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let title = Line::from(vec![Span::from(" ◆ ").fg(MAUVE), Span::from("Agent Tokens").fg(TEXT).bold()]);
    frame.render_widget(Paragraph::new(title), area);
    if let Some(snapshot) = &app.snapshot {
        let age = snapshot.at.elapsed().as_secs();
        let status = format!(
            "{} agents · updated {}s ago · auto {}s ",
            snapshot.agents.len(),
            age,
            REFRESH.as_secs()
        );
        frame.render_widget(Paragraph::new(Line::from(status).fg(MUTED)).alignment(Alignment::Right), area);
    }
}

fn draw_footer(frame: &mut Frame, area: Rect) {
    let keys = [("↑↓", "agent"), ("←→ 1-4", "view"), ("PgUp/PgDn", "scroll"), ("r", "refresh"), ("q", "close")];
    let mut spans = vec![Span::from(" ")];
    for (key, label) in keys {
        spans.push(Span::from(key).fg(MAUVE).bold());
        spans.push(Span::from(format!(" {label}   ")).fg(MUTED));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_agents(frame: &mut Frame, area: Rect, app: &mut App) {
    let snapshot = app.snapshot.as_ref().expect("snapshot is loaded");
    let width = area.width.saturating_sub(4) as usize;
    let items: Vec<ListItem> = snapshot
        .agents
        .iter()
        .map(|agent| {
            let context = match snapshot.usage.get(&agent.pane_id) {
                Some(Usage::Ready(report)) => compact(report.context),
                Some(Usage::Loading) => "…".to_string(),
                _ => String::new(),
            };
            let name = clip(&agent.workspace, width.saturating_sub(context.len() + 3));
            let gap = width.saturating_sub(name.chars().count() + context.len() + 2);
            ListItem::new(vec![
                Line::from(vec![
                    Span::from("● ").fg(status_color(&agent.status)),
                    Span::from(name).fg(TEXT).bold(),
                    Span::from(" ".repeat(gap)),
                    Span::from(context).fg(SAPPHIRE),
                ]),
                Line::from(vec![
                    Span::from("  "),
                    Span::from(agent.status.clone()).fg(status_color(&agent.status)),
                    Span::from(format!(" · {} · {}", agent.kind, agent.pane_id)).fg(MUTED),
                ]),
                Line::from(""),
            ])
        })
        .collect();
    let list = List::new(items)
        .block(panel("Agents"))
        .highlight_style(Style::new().bg(SELECTED))
        .highlight_symbol("▌");
    frame.render_stateful_widget(list, area, &mut app.list);
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let snapshot = app.snapshot.as_ref().expect("snapshot is loaded");
    let Some(agent) = app.list.selected().and_then(|i| snapshot.agents.get(i)) else { return };
    let title = format!("{} · {} · {}", agent.workspace, agent.kind, agent.pane_id);
    let block = panel(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let report = match snapshot.usage.get(&agent.pane_id) {
        Some(Usage::Ready(report)) => report.clone(),
        other => {
            let message = match other {
                Some(Usage::Loading) => "Reading transcript…".to_string(),
                Some(Usage::Unsupported) => format!("Token usage isn't available for {} agents yet.", agent.kind),
                Some(Usage::NoSession) => "Herdr hasn't reported a session for this agent yet.".to_string(),
                _ => "No transcript found for this session.".to_string(),
            };
            centered(frame, inner, vec![Line::from(message).fg(SUBTEXT)]);
            return;
        }
    };

    let [meta, tabs, content] =
        Layout::vertical([Constraint::Length(2), Constraint::Length(2), Constraint::Fill(1)]).areas(inner.inner(ratatui::layout::Margin::new(1, 0)));
    draw_meta(frame, meta, &report);
    frame.render_widget(
        Tabs::new(TABS.iter().enumerate().map(|(i, t)| format!("{} {t}", i + 1)))
            .select(app.tab)
            .style(Style::new().fg(MUTED))
            .highlight_style(Style::new().fg(MAUVE).bold().underlined())
            .divider(Span::from("  ")),
        tabs,
    );
    match app.tab {
        0 => draw_overview(frame, content, &report),
        1 => draw_prompts(frame, content, &report, app.scroll),
        2 => draw_subagents(frame, content, &report, app.scroll),
        _ => draw_models(frame, content, &report),
    }
}

fn draw_meta(frame: &mut Frame, area: Rect, report: &Report) {
    let mut first = vec![Span::from(report.cwd.clone()).fg(TEXT)];
    if !report.branch.is_empty() {
        first.push(Span::from("  ").fg(MUTED));
        first.push(Span::from(report.branch.clone()).fg(GREEN));
    }
    let updated = report.updated.map(|t| t.format("%H:%M").to_string()).unwrap_or_default();
    let second = Line::from(vec![
        Span::from(format!("session {}", report.session_id)).fg(MUTED),
        Span::from(format!(" · v{} · running {} · {} prompts · last activity {}", report.version, duration(report), report.turns.len(), updated)).fg(MUTED),
    ]);
    frame.render_widget(Paragraph::new(vec![Line::from(first), second]), area);
}

fn card(frame: &mut Frame, area: Rect, label: &str, value: String, color: Color, note: String) {
    let block = panel(label);
    let lines = vec![
        Line::from(value).fg(color).bold(),
        Line::from(note).fg(MUTED),
    ];
    let inner = block.inner(area);
    frame.render_widget(block, area);
    centered(frame, inner, lines);
}

fn draw_overview(frame: &mut Frame, area: Rect, report: &Report) {
    let mut limits = if report.rate_limits.is_empty() { 0 } else { report.rate_limits.len() as u16 + 2 };
    let mut mix_height = 4;
    let mut chart_height = area.height.saturating_sub(5 + mix_height + limits);
    if chart_height < 8 {
        chart_height = 0;
        limits = limits.min(area.height.saturating_sub(5 + mix_height));
        mix_height = mix_height.min(area.height.saturating_sub(5));
    }
    let [cards, mix, charts, limit_area] = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(mix_height),
        Constraint::Length(chart_height),
        Constraint::Length(limits),
    ])
    .areas(area);

    let [a, b, c, d] = Layout::horizontal([Constraint::Fill(1); 4]).areas(cards);
    let (context_value, context_color, context_note) = if report.context_window > 0 {
        let load = ratio(report.context, report.context_window);
        (compact(report.context), load_color(load), format!("{} of {}", percent(load), compact(report.context_window)))
    } else {
        (compact(report.context), SAPPHIRE, format!("peak {}", compact(report.peak_context)))
    };
    card(frame, a, "Context", context_value, context_color, context_note);
    let thinking = if report.totals.reasoning > 0 { format!("{} thinking", compact(report.totals.reasoning)) } else { String::new() };
    card(frame, b, "Output", compact(report.totals.output), BLUE, thinking);
    card(
        frame,
        c,
        "Cache hit",
        percent(report.totals.cache_hit()),
        TEAL,
        format!("{} read", compact(report.totals.cache_read)),
    );
    let subagents = report.subagent_totals();
    let processed_note = if subagents.calls > 0 {
        format!("+{} in subagents", compact(subagents.processed()))
    } else {
        format!("{} API calls", report.totals.calls)
    };
    card(frame, d, "Processed", compact(report.totals.processed()), MAUVE, processed_note);

    if mix.height >= 4 {
        draw_mix(frame, mix, &report.totals);
    }
    if charts.height == 0 {
        return;
    }
    let [growth, bars] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(charts);
    draw_growth(frame, growth, report);
    draw_prompt_bars(frame, bars, report);

    if limit_area.height >= 3 {
        let block = panel("Rate limits");
        let inner = block.inner(limit_area);
        frame.render_widget(block, limit_area);
        let rows = Layout::vertical(vec![Constraint::Length(1); report.rate_limits.len()]).split(inner);
        for (limit, row) in report.rate_limits.iter().zip(rows.iter()) {
            let window = if limit.window_minutes >= 1440 {
                format!("{}d", limit.window_minutes / 1440)
            } else {
                format!("{}h", limit.window_minutes / 60)
            };
            let resets = limit.resets_at.map(|t| t.format("resets %a %H:%M").to_string()).unwrap_or_default();
            frame.render_widget(
                LineGauge::default()
                    .ratio(limit.used.clamp(0.0, 1.0))
                    .label(Line::from(format!("{} {window}  {:>4}  {resets}", limit.name, percent(limit.used))).fg(SUBTEXT))
                    .filled_style(Style::new().fg(load_color(limit.used)))
                    .unfilled_style(Style::new().fg(SURFACE)),
                *row,
            );
        }
    }
}

fn draw_mix(frame: &mut Frame, area: Rect, totals: &Totals) {
    let block = panel("Token mix");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let values = [totals.input, totals.cache_write, totals.cache_read, totals.output];
    let total: u64 = values.iter().sum();
    let width = inner.width as usize;

    let mut bar = Vec::new();
    let mut used = 0;
    for (index, (value, (_, color))) in values.iter().zip(MIX).enumerate() {
        let cells = if index == values.len() - 1 {
            width.saturating_sub(used)
        } else {
            (ratio(*value, total) * width as f64).round() as usize
        };
        let cells = if *value > 0 { cells.max(1) } else { cells }.min(width.saturating_sub(used));
        used += cells;
        bar.push(Span::from("█".repeat(cells)).fg(color));
    }

    let mut legend = Vec::new();
    for (value, (label, color)) in values.iter().zip(MIX) {
        legend.push(Span::from("■ ").fg(color));
        legend.push(Span::from(format!("{label} ")).fg(SUBTEXT));
        legend.push(Span::from(format!("{} ", compact(*value))).fg(TEXT));
        legend.push(Span::from(format!("{}  ", percent(ratio(*value, total)))).fg(MUTED));
    }
    frame.render_widget(Paragraph::new(vec![Line::from(bar), Line::from(legend)]), inner);
}

fn draw_growth(frame: &mut Frame, area: Rect, report: &Report) {
    let block = panel("Context per API call");
    if report.calls.len() < 2 {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        centered(frame, inner, vec![Line::from("Not enough calls yet").fg(MUTED)]);
        return;
    }
    let points: Vec<(f64, f64)> =
        report.calls.iter().enumerate().map(|(i, call)| (i as f64, call.context as f64)).collect();
    let peak = report.peak_context.max(1) as f64;
    let top = if report.context_window > 0 { (report.context_window as f64).max(peak) } else { peak * 1.1 };
    let last = (points.len() - 1) as f64;
    let mut datasets =
        vec![Dataset::default().marker(Marker::Braille).graph_type(GraphType::Line).style(Style::new().fg(SAPPHIRE)).data(&points)];
    let window_line;
    if report.context_window > 0 {
        window_line = [(0.0, report.context_window as f64), (last, report.context_window as f64)];
        datasets.push(Dataset::default().marker(Marker::Braille).graph_type(GraphType::Line).style(Style::new().fg(RED)).data(&window_line));
    }
    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(
            Axis::default()
                .bounds([0.0, last])
                .labels(vec![Span::from("1").fg(MUTED), Span::from(format!("{}", points.len())).fg(MUTED)])
                .style(Style::new().fg(SURFACE)),
        )
        .y_axis(
            Axis::default()
                .bounds([0.0, top])
                .labels(vec![
                    Span::from("0").fg(MUTED),
                    Span::from(compact((top / 2.0) as u64)).fg(MUTED),
                    Span::from(compact(top as u64)).fg(MUTED),
                ])
                .style(Style::new().fg(SURFACE)),
        );
    frame.render_widget(chart, area);
}

fn draw_prompt_bars(frame: &mut Frame, area: Rect, report: &Report) {
    let block = panel("Output per prompt");
    if report.turns.is_empty() {
        let inner = block.inner(area);
        frame.render_widget(block, area);
        centered(frame, inner, vec![Line::from("No prompts yet").fg(MUTED)]);
        return;
    }
    let bar_width = 5;
    let fits = (area.width.saturating_sub(2) / (bar_width + 1)).max(1) as usize;
    let recent = &report.turns[report.turns.len().saturating_sub(fits)..];
    let bars: Vec<Bar> = recent
        .iter()
        .map(|turn| {
            Bar::default()
                .value(turn.totals.output)
                .text_value(compact(turn.totals.output))
                .label(Line::from(turn.started.map(|t| t.format("%H:%M").to_string()).unwrap_or_default()))
                .style(Style::new().fg(BLUE))
                .value_style(Style::new().fg(Color::Black).bg(BLUE))
        })
        .collect();
    frame.render_widget(
        BarChart::default()
            .block(block)
            .bar_width(bar_width)
            .bar_gap(1)
            .label_style(Style::new().fg(MUTED))
            .data(BarGroup::default().bars(&bars)),
        area,
    );
}

fn header(cells: &[&str]) -> Row<'static> {
    Row::new(cells.iter().map(|c| Cell::from(c.to_string()))).style(Style::new().fg(MUTED).add_modifier(Modifier::BOLD)).bottom_margin(1)
}

fn empty(frame: &mut Frame, area: Rect, message: &str) {
    centered(frame, area, vec![Line::from(message.to_string()).fg(MUTED)]);
}

fn draw_prompts(frame: &mut Frame, area: Rect, report: &Report, scroll: usize) {
    if report.turns.is_empty() {
        return empty(frame, area, "No prompts in this session yet.");
    }
    let today = Local::now().date_naive();
    let rows: Vec<Row> = report
        .turns
        .iter()
        .rev()
        .skip(scroll.min(report.turns.len().saturating_sub(1)))
        .map(|turn| {
            let when = turn
                .started
                .map(|t| if t.date_naive() == today { t.format("%H:%M").to_string() } else { t.format("%b %d %H:%M").to_string() })
                .unwrap_or_default();
            Row::new(vec![
                Cell::from(when).fg(SUBTEXT),
                Cell::from(turn.totals.calls.to_string()).fg(TEXT),
                Cell::from(compact(turn.totals.output)).fg(BLUE),
                Cell::from(compact(turn.context)).fg(SAPPHIRE),
                Cell::from(percent(turn.totals.cache_hit())).fg(TEAL),
                Cell::from(turn.prompt.clone()).fg(TEXT),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [Constraint::Length(12), Constraint::Length(6), Constraint::Length(8), Constraint::Length(8), Constraint::Length(6), Constraint::Fill(1)],
    )
    .header(header(&["TIME", "CALLS", "OUTPUT", "CONTEXT", "HIT", "PROMPT"]))
    .column_spacing(2);
    frame.render_widget(table, area);
}

fn draw_subagents(frame: &mut Frame, area: Rect, report: &Report, scroll: usize) {
    if report.subagents.is_empty() {
        return empty(frame, area, "No subagents in this session.");
    }
    let totals = report.subagent_totals();
    let [summary, table_area] = Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::from(format!("{} subagents", report.subagents.len())).fg(TEXT).bold(),
            Span::from(format!(
                " · {} calls · {} output · {} processed · {} cache hit",
                totals.calls,
                compact(totals.output),
                compact(totals.processed()),
                percent(totals.cache_hit())
            ))
            .fg(SUBTEXT),
        ])),
        summary,
    );
    let max = report.subagents.iter().map(|s| s.totals.output).max().unwrap_or(1).max(1);
    let rows: Vec<Row> = report
        .subagents
        .iter()
        .skip(scroll.min(report.subagents.len().saturating_sub(1)))
        .map(|sub| {
            let share = (ratio(sub.totals.output, max) * 6.0).round() as usize;
            Row::new(vec![
                Cell::from(sub.name.clone()).fg(TEXT),
                Cell::from(short_model(&sub.model)).fg(SUBTEXT),
                Cell::from(sub.totals.calls.to_string()).fg(TEXT),
                Cell::from(Line::from(vec![
                    Span::from(format!("{:>6} ", compact(sub.totals.output))).fg(BLUE),
                    Span::from("▇".repeat(share)).fg(BLUE),
                    Span::from("▇".repeat(6 - share)).fg(SURFACE),
                ])),
                Cell::from(compact(sub.totals.processed())).fg(MAUVE),
                Cell::from(percent(sub.totals.cache_hit())).fg(TEAL),
            ])
        })
        .collect();
    let table = Table::new(
        rows,
        [Constraint::Fill(1), Constraint::Length(12), Constraint::Length(5), Constraint::Length(13), Constraint::Length(9), Constraint::Length(4)],
    )
    .header(header(&["TASK", "MODEL", "CALLS", "OUTPUT", "PROCESSED", "HIT"]))
    .column_spacing(1);
    frame.render_widget(table, table_area);
}

fn short_model(model: &str) -> String {
    model.strip_prefix("claude-").unwrap_or(model).to_string()
}

fn model_rows<'a>(models: &[(String, Totals)]) -> Vec<Row<'a>> {
    models
        .iter()
        .map(|(model, totals)| {
            Row::new(vec![
                Cell::from(model.clone()).fg(TEXT),
                Cell::from(totals.calls.to_string()).fg(TEXT),
                Cell::from(compact(totals.input)).fg(PEACH),
                Cell::from(compact(totals.cache_write)).fg(MAUVE),
                Cell::from(compact(totals.cache_read)).fg(TEAL),
                Cell::from(compact(totals.output)).fg(BLUE),
                Cell::from(compact(totals.reasoning)).fg(SUBTEXT),
            ])
        })
        .collect()
}

fn draw_models(frame: &mut Frame, area: Rect, report: &Report) {
    let mut by_model: HashMap<String, Totals> = HashMap::new();
    for sub in &report.subagents {
        by_model.entry(sub.model.clone()).or_default().add(&sub.totals);
    }
    let mut subagent_models: Vec<_> = by_model.into_iter().collect();
    subagent_models.sort_by(|a, b| b.1.output.cmp(&a.1.output));

    let widths = [
        Constraint::Fill(1),
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(8),
    ];
    let columns = ["MODEL", "CALLS", "INPUT", "CACHE WRITE", "CACHE READ", "OUTPUT", "THINKING"];
    let main_height = report.models.len() as u16 + 4;
    let [main_area, sub_area] =
        Layout::vertical([Constraint::Length(main_height), Constraint::Fill(1)]).areas(area);

    frame.render_widget(
        Table::new(model_rows(&report.models), widths).header(header(&columns)).column_spacing(2).block(panel("Main session")),
        main_area,
    );
    if !subagent_models.is_empty() {
        frame.render_widget(
            Table::new(model_rows(&subagent_models), widths).header(header(&columns)).column_spacing(2).block(panel("Subagents")),
            sub_area,
        );
    }
}
