use std::cmp::min;

use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use crate::component::{Component, Focus};

#[derive(Debug)]
pub struct RadioBox {
    buttons: Vec<String>,
    style: Style,
    focused_style: Style,
    selected_button: usize,
    cursor_position: usize,
}

impl RadioBox {
    pub fn new<T: IntoIterator<Item = U>, U: AsRef<str>>(
        buttons: T,
        style: &Style,
        focused_style: &Style,
        selected_button: usize,
    ) -> RadioBox {
        let b: Vec<String> = buttons
            .into_iter()
            .map(|item| String::from(item.as_ref()))
            .collect();

        let selected = min(selected_button, b.len().saturating_sub(1));

        RadioBox {
            buttons: b,
            style: *style,
            focused_style: *focused_style,
            selected_button: selected,
            cursor_position: 0,
        }
    }

    pub fn value(&mut self) -> usize {
        self.selected_button
    }
}

impl Component for RadioBox {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Up | Key::Char('k') => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                } else {
                    key_handled = false;
                }
            }
            Key::Down | Key::Char('j') => {
                if (self.cursor_position + 1) < self.buttons.len() {
                    self.cursor_position += 1;
                } else {
                    key_handled = false;
                }
            }
            Key::Char(' ') | Key::Char('\n') => self.selected_button = self.cursor_position,
            _ => key_handled = false,
        }

        key_handled
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let list = List::new(
            self.buttons
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    ListItem::new(format!(
                        "({}) {}",
                        if i == self.selected_button {
                            "\u{2022}"
                        } else {
                            " "
                        },
                        label
                    ))
                })
                .collect::<Vec<ListItem>>(),
        )
        .style(self.style)
        .highlight_style(match focus {
            Focus::Focused => self.focused_style,
            _ => self.style,
        });

        let mut state = ListState::default();
        state.select(Some(self.cursor_position));

        f.render_stateful_widget(list, *chunk, &mut state);

        if let Focus::Focused = focus {
            if (chunk.width > 1) && (chunk.height > (self.cursor_position as u16)) {
                f.set_cursor(chunk.x + 1, chunk.y + (self.cursor_position as u16));
            }
        }
    }
}
