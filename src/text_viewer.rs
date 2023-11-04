use std::{
    fs,
    path::{Path, PathBuf},
    str, thread,
};

use anyhow::Result;
use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use bat::assets::HighlightingAssets;
use encoding_rs::WINDOWS_1252;
use log::debug;
use syntect::{easy::HighlightLines, util::LinesWithEndings};

use crate::{app::Events, component::Component};

fn expand_tabs_for_line(line: &str, tabsize: usize) -> String {
    let mut expanded = String::new();
    let mut column = 0;

    for c in line.chars() {
        if c == '\t' {
            let spaces_to_insert = tabsize - (column % tabsize);
            expanded.push_str(&" ".repeat(spaces_to_insert));
            column += spaces_to_insert;
        } else {
            expanded.push(c);
            column += 1;
        }
    }

    expanded
}

#[derive(Debug)]
pub struct TextViewer {
    filename: PathBuf,
    tabsize: u8,
    data: Vec<u8>,
    content: String,
    lines: Vec<String>,
    styled_lines: Vec<Vec<(Color, String)>>,
    state: TableState,
}

impl TextViewer {
    pub fn new(filename: &Path, tabsize: u8, s: Sender<Events>) -> Result<TextViewer> {
        let data = fs::read(filename)?;

        let content = match str::from_utf8(&data) {
            Ok(content) => content.to_string(),
            Err(e) => {
                // TODO: Instead of a fallback to WINDOWS_1252, we could use chardetng
                // to find the correct encoding
                match WINDOWS_1252.decode_without_bom_handling_and_without_replacement(&data) {
                    Some(content) => content.to_string(),
                    None => return Err(e.into()),
                }
            }
        };

        let lines: Vec<String> = LinesWithEndings::from(&content)
            .map(|e| expand_tabs_for_line(e, tabsize.into()))
            .collect();

        // Default to unstyled text
        let styled_lines: Vec<Vec<(Color, String)>> = lines
            .iter()
            .map(|line| vec![(Color::Gray, String::from(line))])
            .collect();

        // Do the highlighting in a separate thread
        let file_to_highlight = filename.to_path_buf();
        let lines_to_highlight = lines.clone();
        thread::spawn(move || {
            // Load these once at the start of your program
            let assets = HighlightingAssets::from_binary();
            let syntax_set = assets.get_syntax_set().unwrap();
            let theme = assets.get_theme("base16");

            let syntax = match syntax_set.find_syntax_for_file(file_to_highlight) {
                Ok(syntax) => syntax.unwrap_or_else(|| syntax_set.find_syntax_plain_text()),
                Err(_) => syntax_set.find_syntax_plain_text(),
            };

            let mut highlighter = HighlightLines::new(syntax, &theme);
            let styled_lines: Vec<Vec<(Color, String)>> = lines_to_highlight
                .iter()
                .map(|line| {
                    highlighter
                        .highlight_line(line, &syntax_set)
                        .unwrap()
                        .iter()
                        .map(|(style, text)| {
                            (
                                match style.foreground.r {
                                    0 => Color::Black,
                                    1 => Color::Red,
                                    2 => Color::Green,
                                    3 => Color::Yellow,
                                    4 => Color::Blue,
                                    5 => Color::Magenta,
                                    6 => Color::Cyan,
                                    7 => Color::Gray,
                                    8 => Color::DarkGray,
                                    9 => Color::LightRed,
                                    10 => Color::LightGreen,
                                    11 => Color::LightYellow,
                                    12 => Color::LightBlue,
                                    13 => Color::LightMagenta,
                                    14 => Color::LightCyan,
                                    15 => Color::White,
                                    _ => {
                                        debug!("{:?}", style);
                                        Color::Gray
                                    }
                                },
                                String::from(*text),
                            )
                        })
                        .collect()
                })
                .collect();

            s.send(Events::Highlight(styled_lines)).unwrap();
        });

        let mut state = TableState::default();

        state.select(Some(0));

        Ok(TextViewer {
            filename: filename.to_path_buf(),
            tabsize,
            data,
            content: content.to_string(),
            lines,
            styled_lines,
            state,
        })
    }
}

impl Component for TextViewer {
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
            Events::Highlight(styled_lines) => self.styled_lines = styled_lines.to_vec(),
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
            .styled_lines
            .iter()
            .enumerate()
            .map(|(i, e)| {
                Row::new(vec![
                    Cell::from(Span::styled(
                        format!(
                            "{:width$}",
                            i + 1,
                            width = self.lines.len().to_string().len()
                        )
                        .to_string(),
                        Style::default().fg(Color::White),
                    )),
                    Cell::from(Line::from(
                        e.iter()
                            .map(|(color, text)| Span::styled(text, Style::default().fg(*color)))
                            .collect::<Vec<_>>(),
                    )),
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
