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
};

#[derive(Debug)]
pub struct Panel {
    config: Config,
    pubsub_tx: Sender<PubSub>,
}

impl Panel {
    pub fn new(config: &Config, pubsub_tx: Sender<PubSub>) -> Result<Panel> {
        Ok(Panel {
            config: *config,
            pubsub_tx,
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
                Title::from(Span::styled(
                    " Panel ",
                    match focus {
                        Focus::Focused => Style::default()
                            .fg(self.config.panel.reverse_fg)
                            .bg(self.config.panel.reverse_bg),
                        _ => Style::default().fg(self.config.panel.fg),
                    },
                ))
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
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
