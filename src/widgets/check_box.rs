use anyhow::Result;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use crate::component::{Component, Focus};

#[derive(Debug)]
pub struct CheckBox {
    label: String,
    style: Style,
    focused_style: Style,
    checked: bool,
}

impl CheckBox {
    pub fn new(label: &str, style: &Style, focused_style: &Style) -> Result<CheckBox> {
        Ok(CheckBox {
            label: String::from(label),
            style: *style,
            focused_style: *focused_style,
            checked: false,
        })
    }

    pub fn value(&mut self) -> bool {
        self.checked
    }
}

impl Component for CheckBox {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        let mut key_handled = true;

        match key {
            Key::Char(' ') | Key::Char('\n') => self.checked = !self.checked,
            _ => key_handled = false,
        }

        Ok(key_handled)
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let check_box = Paragraph::new(format!(
            "[{}] {}",
            if self.checked { "x" } else { " " },
            &self.label
        ))
        .block(Block::default().style(match focus {
            Focus::Focused => self.focused_style,
            _ => self.style,
        }));

        f.render_widget(check_box, *chunk);

        if let Focus::Focused = focus {
            if (chunk.width > 1) && (chunk.height > 0) {
                f.set_cursor(chunk.x + 1, chunk.y);
            }
        }
    }
}
