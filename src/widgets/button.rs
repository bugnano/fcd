use anyhow::Result;
use ratatui::{prelude::*, widgets::*};

use unicode_width::UnicodeWidthStr;

use crate::component::{Component, Focus};

#[derive(Debug)]
pub struct Button {
    label: String,
    style: Style,
    focused_style: Style,
    active_style: Style,
}

impl Button {
    pub fn new(
        label: &str,
        style: &Style,
        focused_style: &Style,
        active_style: &Style,
    ) -> Result<Button> {
        Ok(Button {
            label: String::from(label),
            style: *style,
            focused_style: *focused_style,
            active_style: *active_style,
        })
    }

    pub fn width(&self) -> usize {
        self.label.width() + 4
    }
}

impl Component for Button {
    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let button = Paragraph::new(Line::from(vec![
            Span::styled(
                "[ ",
                match focus {
                    Focus::Focused => self.focused_style,
                    _ => self.style,
                },
            ),
            Span::styled(
                &self.label,
                match focus {
                    Focus::Normal => self.style,
                    Focus::Focused => self.focused_style,
                    Focus::Active => self.active_style,
                },
            ),
            Span::styled(
                " ]",
                match focus {
                    Focus::Focused => self.focused_style,
                    _ => self.style,
                },
            ),
        ]));

        f.render_widget(button, *chunk);

        if let Focus::Focused = focus {
            if (chunk.width > 2) && (chunk.height > 0) {
                f.set_cursor(chunk.x + 2, chunk.y);
            }
        }
    }
}
