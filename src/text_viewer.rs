use std::{
    fs::File,
    io::{BufRead, BufReader},
};

use anyhow::Result;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use crate::component::Component;
use crate::events::Events;

#[derive(Debug)]
pub struct TextViewer {
    lines: Vec<String>,
    state: TableState,
}

impl TextViewer {
    pub fn new() -> Result<TextViewer> {
        let file = File::open("lorem.txt")?;
        let buffered = BufReader::new(file);
        let lines: Vec<String> = buffered.lines().map(|e| String::from(e.unwrap())).collect();

        let state = TableState::default();

        Ok(TextViewer { lines, state })
    }
}

impl Component for TextViewer {
    fn init(&mut self) -> Result<()> {
        self.state.select(Some(0));

        Ok(())
    }

    fn handle_events(&mut self, events: &Events) -> Result<bool> {
        let mut event_handled = false;

        match events {
            Events::Input(event) => match event {
                Event::Key(key) => match key {
                    Key::Up => {
                        event_handled = true;

                        self.state.select(Some(match self.state.selected() {
                            Some(i) if i > 0 => i - 1,
                            Some(_i) => 0,
                            None => 0,
                        }));
                        *self.state.offset_mut() = self.state.selected().unwrap();
                    }
                    Key::Down => {
                        event_handled = true;

                        self.state.select(Some(match self.state.selected() {
                            Some(i) if (i + 1) < self.lines.len() => i + 1,
                            Some(i) => i,
                            None => 0,
                        }));
                        *self.state.offset_mut() = self.state.selected().unwrap();
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
        let widths = [
            Constraint::Length((self.lines.len().to_string().len() + 1) as u16),
            Constraint::Percentage(100),
        ];

        let items: Vec<Row> = self
            .lines
            .iter()
            .enumerate()
            .map(|(i, e)| {
                Row::new(vec![
                    Span::styled(
                        format!(
                            "{:width$}",
                            i + 1,
                            width = self.lines.len().to_string().len()
                        )
                        .to_string(),
                        Style::default().fg(Color::White),
                    ),
                    e.to_string().into(),
                ])
            })
            .collect();
        let items = Table::new(Vec::from(items))
            .block(Block::default().style(Style::default().bg(Color::Blue)))
            .widths(&widths)
            .column_spacing(0)
            .highlight_style(
                Style::default()
                    .bg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            );

        f.render_stateful_widget(items, *chunk, &mut self.state);
    }
}
