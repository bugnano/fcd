use anyhow::Result;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use unicode_width::UnicodeWidthChar;

use crate::{app::Events, component::Component};

#[derive(Debug)]
pub struct Input {
    pub style: Style,
    pub focused: bool,
    input: String,
    cursor_position: usize,
    scroll_offset: usize,
}

impl Input {
    pub fn new(style: &Style, focused: bool) -> Result<Input> {
        Ok(Input {
            style: *style,
            focused,
            input: String::from(""),
            cursor_position: 0,
            scroll_offset: 0,
        })
    }

    pub fn reset(&mut self) {
        self.input = String::from("");
        self.cursor_position = 0;
        self.scroll_offset = 0;
    }

    pub fn value(&mut self) -> String {
        self.input.clone()
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
    fn handle_events(&mut self, events: &Events) -> Result<bool> {
        let mut event_handled = false;

        match events {
            Events::Input(event) => match event {
                Event::Key(key) => match key {
                    Key::Char('\t') => (),
                    Key::Char(c) => {
                        event_handled = true;

                        self.enter_char(*c);
                    }
                    Key::Left => {
                        event_handled = true;

                        self.move_cursor_left();
                    }
                    Key::Right => {
                        event_handled = true;

                        self.move_cursor_right();
                    }
                    Key::Home => {
                        event_handled = true;

                        self.cursor_position = 0;
                    }
                    Key::End => {
                        event_handled = true;

                        self.cursor_position = self.clamp_cursor(self.input.len());
                    }
                    Key::Backspace => {
                        event_handled = true;

                        self.delete_char_left()
                    }
                    Key::Delete => {
                        event_handled = true;

                        self.delete_char_right()
                    }
                    _ => (),
                },
                _ => (),
            },
            _ => (),
        }

        Ok(event_handled)
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect) {
        if chunk.width == 0 {
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

        let input = Paragraph::new(scrolled_string.as_str())
            .style(self.style)
            .block(Block::default().style(self.style));

        f.render_widget(input, *chunk);

        if self.focused {
            f.set_cursor(chunk.x + (cursor_width as u16), chunk.y);
        }
    }
}
