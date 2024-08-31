use std::rc::Rc;

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
    palette::Palette,
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
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    message: String,
    title: String,
    dialog_type: DialogType,
    next_action: Option<Box<PubSub>>,
}

impl DlgError {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        message: &str,
        title: &str,
        dialog_type: DialogType,
        next_action: Option<Box<PubSub>>,
    ) -> DlgError {
        DlgError {
            palette: Rc::clone(palette),
            pubsub_tx,
            message: String::from(message),
            title: String::from(title),
            dialog_type,
            next_action,
        }
    }
}

impl Component for DlgError {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match self.dialog_type {
            DialogType::Error | DialogType::Warning => {
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                if let Some(next_action) = &self.next_action {
                    self.pubsub_tx.send(*next_action.clone()).unwrap();
                }
            }
            DialogType::Info => match key {
                Key::Ctrl('c') => key_handled = false,
                Key::Ctrl('l') => key_handled = false,
                Key::Ctrl('z') => key_handled = false,
                Key::Ctrl('o') => key_handled = false,
                _ => (),
            },
        }

        key_handled
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect((self.message.width() + 6) as u16, 7, chunk);

        let (style, title_style) = match self.dialog_type {
            DialogType::Error => (self.palette.error, self.palette.error_title),
            DialogType::Warning | DialogType::Info => {
                (self.palette.dialog, self.palette.dialog_title)
            }
        };

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(style), area);
        if let Some(shadow) = self.palette.shadow {
            render_shadow(f, &area, &shadow);
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
