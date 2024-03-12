use std::rc::Rc;

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

#[derive(Debug, Copy, Clone)]
pub enum DialogType {
    Error,
    Warning,
    Info,
}

#[derive(Debug)]
pub struct DlgError {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    message: String,
    title: String,
    dialog_type: DialogType,
}

impl DlgError {
    pub fn new(
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        message: &str,
        title: &str,
        dialog_type: DialogType,
    ) -> Result<DlgError> {
        Ok(DlgError {
            config: Rc::clone(config),
            pubsub_tx,
            message: String::from(message),
            title: String::from(title),
            dialog_type,
        })
    }
}

impl Component for DlgError {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        let mut key_handled = true;

        match self.dialog_type {
            DialogType::Error | DialogType::Warning => {
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
            }
            DialogType::Info => match key {
                Key::Char(_) | Key::F(_) => (),
                _ => key_handled = false,
            },
        }

        Ok(key_handled)
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect((self.message.width() + 6) as u16, 7, chunk);

        let (style, title_style) = match self.dialog_type {
            DialogType::Error => (
                Style::default()
                    .fg(self.config.error.fg)
                    .bg(self.config.error.bg),
                Style::default()
                    .fg(self.config.error.title_fg)
                    .bg(self.config.error.bg),
            ),
            DialogType::Warning | DialogType::Info => (
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
                Style::default()
                    .fg(self.config.dialog.title_fg)
                    .bg(self.config.dialog.bg),
            ),
        };

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(style), area);
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
                    Title::from(Span::styled(format!(" {} ", self.title), title_style))
                        .position(Position::Top)
                        .alignment(Alignment::Center),
                )
                .borders(Borders::ALL)
                .padding(Padding::uniform(1))
                .style(style),
        );

        f.render_widget(message, section);
    }
}
