use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    str,
};

use anyhow::Result;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use encoding_rs::WINDOWS_1252;
use log::debug;
use syntect::{
    easy::HighlightLines,
    highlighting::{self, ThemeSet},
    parsing::{ParseState, SyntaxSet},
    util::LinesWithEndings,
};

use crate::{app::Events, component::Component};

fn expand_tabs_for_line(line: &str, tabsize: usize) -> String {
    let mut expanded = String::with_capacity(line.len() * 2);
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
    pub fn new(filename: &Path, tabsize: u8) -> Result<TextViewer> {
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

        // TODO: Load these once at the start of your program
        let ps = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();

        let syntax = match ps.find_syntax_for_file(filename) {
            Ok(syntax) => syntax.unwrap_or_else(|| ps.find_syntax_plain_text()),
            Err(_) => ps.find_syntax_plain_text(),
        };

        let mut h = HighlightLines::new(syntax, &ts.themes["base16-ocean.dark"]);
        let styled_lines: Vec<Vec<(Color, String)>> = lines
            .iter()
            .map(|line| {
                h.highlight_line(line, &ps)
                    .unwrap()
                    .iter()
                    .map(|(style, text)| {
                        (
                            match (style.foreground.r, style.foreground.g, style.foreground.b) {
                                (0x2b, 0x30, 0x3b) => Color::Black, // base00
                                (0x34, 0x3d, 0x46) => Color::LightRed,
                                (0x4f, 0x5b, 0x66) => Color::LightGreen,
                                (0x65, 0x73, 0x7e) => Color::DarkGray, // base03
                                (0xa7, 0xad, 0xba) => Color::LightYellow,
                                (0xc0, 0xc5, 0xce) => Color::Gray, // base05
                                (0xdf, 0xe1, 0xe8) => Color::LightBlue,
                                (0xef, 0xf1, 0xf5) => Color::White, // base07
                                (0xbf, 0x61, 0x6a) => Color::Red,   // base08
                                (0xd0, 0x87, 0x70) => Color::LightMagenta,
                                (0xeb, 0xcb, 0x8b) => Color::Yellow, // base0A
                                (0xa3, 0xbe, 0x8c) => Color::Green,  // base0B
                                (0x96, 0xb5, 0xb4) => Color::Cyan,   // base0C
                                (0x8f, 0xa1, 0xb3) => Color::Blue,   // base0D
                                (0xb4, 0x8e, 0xad) => Color::Magenta, // base0E
                                (0xab, 0x79, 0x67) => Color::LightCyan,
                                _ => Color::Gray,
                            },
                            String::from(*text),
                        )
                    })
                    .collect()
            })
            .collect();

        let mut state = TableState::default();

        state.select(Some(0));

        Ok(TextViewer {
            filename: filename.to_path_buf(),
            tabsize,
            data,
            content: content.to_string(),
            lines: lines,
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
