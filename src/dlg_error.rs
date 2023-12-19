use anyhow::Result;
use crossbeam_channel::Sender;
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use termion::event::*;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::{centered_rect, render_shadow, PubSub},
    component::{Component, Focus},
    config::Config,
    tilde_layout::tilde_layout,
};

#[derive(Debug)]
pub struct DlgError {
    config: Config,
    pubsub_tx: Sender<PubSub>,
    message: String,
}

impl DlgError {
    pub fn new(config: &Config, pubsub_tx: Sender<PubSub>, message: &str) -> Result<DlgError> {
        Ok(DlgError {
            config: *config,
            pubsub_tx,
            message: String::from(message),
        })
    }
}

impl Component for DlgError {
    fn handle_key(&mut self, _key: &Key) -> Result<bool> {
        self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

        Ok(true)
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect((self.message.width() + 6) as u16, 7, chunk);

        f.render_widget(Clear, area);
        f.render_widget(
            Block::default().style(
                Style::default()
                    .fg(self.config.error.fg)
                    .bg(self.config.error.bg),
            ),
            area,
        );
        if self.config.ui.use_shadows {
            render_shadow(
                f,
                &area,
                &Style::default()
                    .bg(self.config.ui.shadow_bg)
                    .fg(self.config.ui.shadow_fg),
            );
        }

        let section = centered_rect(
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
            &area,
        );

        let message = Paragraph::new(Span::raw(tilde_layout(
            &self.message,
            section.width.saturating_sub(4).into(),
        )))
        .block(
            Block::default()
                .title(
                    Title::from(Span::styled(
                        " Error ",
                        Style::default().fg(self.config.error.title_fg),
                    ))
                    .position(Position::Top)
                    .alignment(Alignment::Center),
                )
                .borders(Borders::ALL)
                .padding(Padding::uniform(1))
                .style(
                    Style::default()
                        .fg(self.config.error.fg)
                        .bg(self.config.error.bg),
                ),
        );

        f.render_widget(message, section);
    }
}
