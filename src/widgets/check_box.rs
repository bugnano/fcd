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
    pub fn new(label: &str, style: &Style, focused_style: &Style, checked: bool) -> CheckBox {
        CheckBox {
            label: String::from(label),
            style: *style,
            focused_style: *focused_style,
            checked,
        }
    }

    pub fn value(&mut self) -> bool {
        self.checked
    }
}

impl Component for CheckBox {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Char(' ') => self.checked = !self.checked,
            _ => key_handled = false,
        }

        key_handled
    }

    fn handle_mouse(&mut self, button: MouseButton, _mouse_position: Position) {
        if let MouseButton::Left = button {
            self.checked = !self.checked;
        }
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
                f.set_cursor_position((chunk.x + 1, chunk.y));
            }
        }
    }
}
