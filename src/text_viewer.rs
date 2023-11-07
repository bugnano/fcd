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

use crate::{app::Events, component::Component, config::Config};

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
    config: Config,
    rect: Rect,
    filename: PathBuf,
    tabsize: u8,
    data: Vec<u8>,
    content: String,
    lines: Vec<String>,
    styled_lines: Vec<Vec<(Color, String)>>,
    first_line: usize,
}

impl TextViewer {
    pub fn new(
        config: &Config,
        rect: &Rect,
        filename: &Path,
        tabsize: u8,
        s: Sender<Events>,
    ) -> Result<TextViewer> {
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
            .map(|line| vec![(config.highlight.base05, String::from(line))])
            .collect();

        let viewer = TextViewer {
            config: *config,
            rect: *rect,
            filename: filename.to_path_buf(),
            tabsize,
            data,
            content: content.to_string(),
            lines,
            styled_lines,
            first_line: 0,
        };

        viewer.highlight(s);

        Ok(viewer)
    }

    fn highlight(&self, s: Sender<Events>) {
        let filename = self.filename.clone();
        let lines = self.lines.clone();
        let config = self.config.clone();

        // Do the highlighting in a separate thread
        thread::spawn(move || {
            // Load these once at the start of your program
            let assets = HighlightingAssets::from_binary();
            let syntax_set = assets.get_syntax_set().unwrap();
            let theme = assets.get_theme("base16");

            let syntax = match syntax_set.find_syntax_for_file(&filename) {
                Ok(syntax) => syntax.unwrap_or_else(|| syntax_set.find_syntax_plain_text()),
                Err(_) => syntax_set.find_syntax_plain_text(),
            };

            let mut highlighter = HighlightLines::new(syntax, &theme);
            let styled_lines: Vec<Vec<(Color, String)>> = lines
                .iter()
                .map(|line| {
                    highlighter
                        .highlight_line(line, &syntax_set)
                        .unwrap()
                        .iter()
                        .map(|(style, text)| {
                            (
                                match style.foreground.r {
                                    0x00 => config.highlight.base00,
                                    0x01 => config.highlight.base08,
                                    0x02 => config.highlight.base0b,
                                    0x03 => config.highlight.base0a,
                                    0x04 => config.highlight.base0d,
                                    0x05 => config.highlight.base0e,
                                    0x06 => config.highlight.base0c,
                                    0x07 => config.highlight.base05,
                                    0x08 => config.highlight.base03,
                                    0x09 => config.highlight.base09,
                                    0x0F => config.highlight.base0f,
                                    _ => {
                                        debug!("{:?}", style);
                                        config.highlight.base05
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
    }

    pub fn resize(&mut self, rect: &Rect) {
        self.rect = *rect;

        self.first_line = match self.first_line {
            i if (i + (self.rect.height as usize)) > self.lines.len() => {
                self.lines.len() - (self.rect.height as usize)
            }
            i => i,
        };
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

                        self.first_line = match self.first_line {
                            i if i > 0 => i - 1,
                            _ => 0,
                        };
                    }
                    Key::Down => {
                        event_handled = true;

                        self.first_line = match self.first_line {
                            i if (i + 1 + (self.rect.height as usize)) <= self.lines.len() => i + 1,
                            i => i,
                        };
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
        let line_number_width = self.lines.len().to_string().len();
        let widths = [
            Constraint::Length((line_number_width + 1) as u16),
            Constraint::Percentage(100),
        ];

        let items: Vec<Row> = self
            .styled_lines
            .iter()
            .skip(self.first_line)
            .take(chunk.height.into())
            .enumerate()
            .map(|(i, e)| {
                Row::new(vec![
                    Cell::from(Span::styled(
                        format!(
                            "{:width$} ",
                            self.first_line + i + 1,
                            width = line_number_width
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

        let items = Table::new(items)
            .block(Block::default().style(Style::default().bg(self.config.highlight.base00)))
            .widths(&widths)
            .column_spacing(0);

        f.render_widget(items, *chunk);
    }
}
