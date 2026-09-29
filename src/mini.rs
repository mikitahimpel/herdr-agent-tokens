use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::herdr;
use crate::ui::{compact, status_color, MAUVE, MUTED, SAPPHIRE, SELECTED, SUBTEXT, TEXT, BLUE};
use crate::usage::ratio;
use crate::worker::{self, Snapshot, Usage};

const HOVER: Color = Color::Rgb(69, 71, 90);

struct Chip {
    area: Rect,
    pane_id: String,
}

fn short_pane(pane_id: &str) -> &str {
    pane_id.rsplit(':').next().unwrap_or(pane_id)
}

fn chip_spans(snapshot: &Snapshot, index: usize) -> Vec<Span<'static>> {
    let agent = &snapshot.agents[index];
    let mut spans = vec![
        Span::from(" ● ").fg(status_color(&agent.status)),
        Span::from(agent.workspace.clone()).fg(TEXT).bold(),
        Span::from(format!(" {} ", short_pane(&agent.pane_id))).fg(MUTED),
    ];
    match snapshot.usage.get(&agent.pane_id) {
        Some(Usage::Ready(report)) => {
            spans.push(Span::from(compact(report.context)).fg(SAPPHIRE));
            if report.context_window > 0 {
                let load = ratio(report.context, report.context_window);
                spans.push(Span::from(format!(" {:.0}%", load * 100.0)).fg(SUBTEXT));
            }
            spans.push(Span::from(" ctx  ").fg(MUTED));
            spans.push(Span::from(compact(report.totals.output)).fg(BLUE));
            spans.push(Span::from(" out ").fg(MUTED));
        }
        _ => spans.push(Span::from(format!("{} ", agent.kind)).fg(MUTED)),
    }
    spans
}

fn draw(frame: &mut Frame, snapshot: Option<&Snapshot>, hover: Option<Position>) -> Vec<Chip> {
    let area = frame.area();
    let title = Line::from(vec![
        Span::from(" ◆ ").fg(MAUVE),
        Span::from("Agent Tokens").fg(TEXT).bold(),
        Span::from("  click an agent for details").fg(MUTED),
    ]);
    frame.render_widget(Paragraph::new(title), Rect { height: 1, ..area });

    let Some(snapshot) = snapshot else {
        frame.render_widget(Paragraph::new(Line::from(" loading…").fg(MUTED)), Rect { y: area.y + 1, height: 1, ..area });
        return Vec::new();
    };
    if let Some(error) = &snapshot.error {
        frame.render_widget(Paragraph::new(Line::from(format!(" {error}")).fg(MUTED)), Rect { y: area.y + 1, height: 1, ..area });
        return Vec::new();
    }

    let total_output: u64 = snapshot
        .usage
        .values()
        .filter_map(|u| match u {
            Usage::Ready(report) => Some(report.totals.output + report.subagent_totals().output),
            _ => None,
        })
        .sum();
    let summary = Line::from(vec![
        Span::from(format!("{} agents · ", snapshot.agents.len())).fg(MUTED),
        Span::from(compact(total_output)).fg(BLUE),
        Span::from(" out total ").fg(MUTED),
    ]);
    frame.render_widget(Paragraph::new(summary).alignment(Alignment::Right), Rect { height: 1, ..area });

    let mut chips = Vec::new();
    let (mut x, mut y) = (area.x + 1, area.y + 1);
    for index in 0..snapshot.agents.len() {
        let spans = chip_spans(snapshot, index);
        let width = spans.iter().map(|s| s.width() as u16).sum::<u16>().min(area.width.saturating_sub(2));
        if x > area.x + 1 && x + width > area.right() {
            x = area.x + 1;
            y += 1;
        }
        if y >= area.bottom() {
            break;
        }
        let rect = Rect { x, y, width, height: 1 };
        let hovered = hover.is_some_and(|p| rect.contains(p));
        let background = if hovered { HOVER } else { SELECTED };
        frame.render_widget(Paragraph::new(Line::from(spans)).style(Style::new().bg(background)), rect);
        chips.push(Chip { area: rect, pane_id: snapshot.agents[index].pane_id.clone() });
        x += width + 1;
    }
    if chips.is_empty() && snapshot.agents.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(" no agents running").fg(MUTED)),
            Rect { y: area.y + 1, height: 1, ..area },
        );
    }
    chips
}

pub fn run() -> io::Result<()> {
    let (request_tx, request_rx) = mpsc::channel();
    let (snapshot_tx, snapshot_rx) = mpsc::channel();
    thread::spawn(move || worker::run(request_rx, snapshot_tx));

    let mut terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture)?;
    let mut snapshot: Option<Snapshot> = None;
    let mut hover: Option<Position> = None;
    let mut chips: Vec<Chip> = Vec::new();

    let result = (|| -> io::Result<()> {
        loop {
            while let Ok(next) = snapshot_rx.try_recv() {
                snapshot = Some(next);
            }
            terminal.draw(|frame| chips = draw(frame, snapshot.as_ref(), hover))?;
            if !event::poll(Duration::from_millis(200))? {
                continue;
            }
            match event::read()? {
                Event::Mouse(mouse) => {
                    let position = Position { x: mouse.column, y: mouse.row };
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            if let Some(chip) = chips.iter().find(|c| c.area.contains(position)) {
                                herdr::open_dashboard_for(Some(chip.pane_id.clone()));
                            }
                        }
                        MouseEventKind::Moved => hover = Some(position),
                        _ => {}
                    }
                }
                Event::Key(key) if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(());
                }
                Event::Key(key) if key.code == KeyCode::Char('r') => {
                    let _ = request_tx.send(());
                }
                _ => {}
            }
        }
    })();

    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}
