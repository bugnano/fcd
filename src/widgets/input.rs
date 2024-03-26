use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use unicode_width::UnicodeWidthChar;

use crate::component::{Component, Focus};

#[derive(Debug)]
pub struct Input {
    input: String,
    style: Style,
    cursor_position: usize,
    scroll_offset: usize,
}

impl Input {
    pub fn new(style: &Style, input: &str, cursor_position: usize) -> Input {
        let mut widget = Input {
            input: String::from(input),
            style: *style,
            cursor_position: 0,
            scroll_offset: 0,
        };

        widget.cursor_position = widget.clamp_cursor(cursor_position);

        widget
    }

    pub fn value(&mut self) -> String {
        String::from(self.input.trim())
    }

    fn move_cursor_left(&mut self) {
        self.cursor_position = self.clamp_cursor(self.cursor_position.saturating_sub(1));
    }

    fn move_cursor_right(&mut self) {
        self.cursor_position = self.clamp_cursor(self.cursor_position.saturating_add(1));
    }

    fn enter_char(&mut self, new_char: char) {
        self.input = self
            .input
            .chars()
            .take(self.cursor_position)
            .chain(std::iter::once(new_char))
            .chain(self.input.chars().skip(self.cursor_position))
            .collect();

        self.move_cursor_right();
    }

    fn delete_char_left(&mut self) {
        self.input = self
            .input
            .chars()
            .take(self.cursor_position.saturating_sub(1))
            .chain(self.input.chars().skip(self.cursor_position))
            .collect();

        self.move_cursor_left();
    }

    fn delete_char_right(&mut self) {
        self.input = self
            .input
            .chars()
            .take(self.cursor_position)
            .chain(
                self.input
                    .chars()
                    .skip(self.cursor_position.saturating_add(1)),
            )
            .collect();
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }
}

impl Component for Input {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Char('\t') | Key::Char('\n') => key_handled = false,
            Key::Char(c) => self.enter_char(*c),
            Key::Left => self.move_cursor_left(),
            Key::Right => self.move_cursor_right(),
            Key::Home => self.cursor_position = 0,
            Key::End => self.cursor_position = self.clamp_cursor(self.input.len()),
            Key::Backspace => self.delete_char_left(),
            Key::Delete => self.delete_char_right(),
            _ => key_handled = false,
        }

        key_handled
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        if (chunk.width == 0) || (chunk.height == 0) {
            return;
        }

        if self.scroll_offset > self.cursor_position {
            self.scroll_offset = self.cursor_position;
        }

        let mut cursor_width = 0;
        let offset_from_cursor = self
            .input
            .chars()
            .skip(self.scroll_offset)
            .take(self.cursor_position - self.scroll_offset)
            .collect::<Vec<char>>()
            .iter()
            .rev()
            .take_while(|c| {
                let width = c.width().unwrap_or(0);

                if (cursor_width + width) < chunk.width.into() {
                    cursor_width += width;

                    true
                } else {
                    false
                }
            })
            .count();

        self.scroll_offset = self.cursor_position - offset_from_cursor;

        let mut string_width = 0;
        let scrolled_string: String = self
            .input
            .chars()
            .skip(self.scroll_offset)
            .take_while(|c| {
                let width = c.width().unwrap_or(0);

                if (string_width + width) <= chunk.width.into() {
                    string_width += width;

                    true
                } else {
                    false
                }
            })
            .collect();

        let input =
            Paragraph::new(scrolled_string.as_str()).block(Block::default().style(self.style));

        f.render_widget(input, *chunk);

        if let Focus::Focused = focus {
            f.set_cursor(chunk.x + (cursor_width as u16), chunk.y);
        }
    }
}
