mod herdr;
mod ui;
mod usage;
mod worker;

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::widgets::ListState;

use worker::Snapshot;

pub const TABS: [&str; 4] = ["Overview", "Prompts", "Subagents", "Models"];

pub struct App {
    pub snapshot: Option<Snapshot>,
    pub list: ListState,
    pub tab: usize,
    pub scroll: usize,
    initial_pane: Option<String>,
}

impl App {
    fn apply(&mut self, snapshot: Snapshot) {
        let previous = self.selected_pane().map(str::to_string).or_else(|| self.initial_pane.take());
        let index = previous
            .and_then(|pane| snapshot.agents.iter().position(|a| a.pane_id == pane))
            .unwrap_or(0);
        self.list.select((!snapshot.agents.is_empty()).then_some(index));
        self.snapshot = Some(snapshot);
    }

    pub fn selected_pane(&self) -> Option<&str> {
        let snapshot = self.snapshot.as_ref()?;
        snapshot.agents.get(self.list.selected()?).map(|a| a.pane_id.as_str())
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.snapshot.as_ref().map_or(0, |s| s.agents.len());
        if count == 0 {
            return;
        }
        let current = self.list.selected().unwrap_or(0) as isize;
        self.list.select(Some((current + delta).rem_euclid(count as isize) as usize));
        self.scroll = 0;
    }

    fn switch_tab(&mut self, delta: isize) {
        self.tab = (self.tab as isize + delta).rem_euclid(TABS.len() as isize) as usize;
        self.scroll = 0;
    }
}

fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("open") {
        std::process::exit(herdr::open_dashboard());
    }
    let (request_tx, request_rx) = mpsc::channel();
    let (snapshot_tx, snapshot_rx) = mpsc::channel();
    thread::spawn(move || worker::run(request_rx, snapshot_tx));

    let mut app = App { snapshot: None, list: ListState::default(), tab: 0, scroll: 0, initial_pane: herdr::initial_pane() };
    let mut terminal = ratatui::init();
    let result = (|| -> std::io::Result<()> {
        loop {
            while let Ok(snapshot) = snapshot_rx.try_recv() {
                app.apply(snapshot);
            }
            terminal.draw(|frame| ui::draw(frame, &mut app))?;

            if !event::poll(Duration::from_millis(150))? {
                continue;
            }
            let Event::Key(key) = event::read()? else { continue };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
                KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => app.switch_tab(1),
                KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => app.switch_tab(-1),
                KeyCode::Char(c @ '1'..='4') => {
                    app.tab = c as usize - '1' as usize;
                    app.scroll = 0;
                }
                KeyCode::PageDown | KeyCode::Char('J') | KeyCode::Char(' ') => app.scroll += 5,
                KeyCode::PageUp | KeyCode::Char('K') => app.scroll = app.scroll.saturating_sub(5),
                KeyCode::Char('r') => {
                    let _ = request_tx.send(());
                }
                _ => {}
            }
        }
    })();
    ratatui::restore();
    result
}
