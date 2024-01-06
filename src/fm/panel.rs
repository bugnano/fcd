use std::path::{Path, PathBuf};

use anyhow::Result;
use crossbeam_channel::Sender;
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    tilde_layout::tilde_layout,
};

#[derive(Debug)]
pub struct Panel {
    config: Config,
    pubsub_tx: Sender<PubSub>,
    cwd: PathBuf,
}

impl Panel {
    pub fn new(config: &Config, pubsub_tx: Sender<PubSub>, initial_path: &Path) -> Result<Panel> {
        Ok(Panel {
            config: *config,
            pubsub_tx,
            cwd: initial_path.to_path_buf(),
        })
    }
}

impl Component for Panel {
    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let middle_border_set = symbols::border::Set {
            top_left: symbols::line::NORMAL.vertical_right,
            top_right: symbols::line::NORMAL.vertical_left,
            ..symbols::border::PLAIN
        };

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(*chunk);

        let upper_block = Block::default()
            .title(
                Title::from(Line::from(vec![
                    Span::raw(symbols::line::NORMAL.horizontal),
                    Span::styled(
                        tilde_layout(
                            &format!(" {} ", self.cwd.to_string_lossy()),
                            chunk.width.saturating_sub(4).into(),
                        ),
                        match focus {
                            Focus::Focused => Style::default()
                                .fg(self.config.panel.reverse_fg)
                                .bg(self.config.panel.reverse_bg),
                            _ => Style::default()
                                .fg(self.config.panel.fg)
                                .bg(self.config.panel.bg),
                        },
                    ),
                    Span::raw(symbols::line::NORMAL.horizontal),
                ]))
                .position(Position::Top)
                .alignment(Alignment::Left),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .style(
                Style::default()
                    .fg(self.config.panel.fg)
                    .bg(self.config.panel.bg),
            );

        f.render_widget(upper_block, sections[0]);

        let lower_block = Block::default()
            .borders(Borders::ALL)
            .border_set(middle_border_set)
            .style(
                Style::default()
                    .fg(self.config.panel.fg)
                    .bg(self.config.panel.bg),
            );

        f.render_widget(lower_block, sections[1]);
    }
}
