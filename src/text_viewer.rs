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
use regex::{self, Regex, RegexBuilder};
use syntect::{easy::HighlightLines, util::LinesWithEndings};

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    dlg_goto::GotoType,
    dlg_text_search::{SearchType, TextSearch},
    fnmatch,
};

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
    pubsub_tx: Sender<PubSub>,
    rect: Rect,
    filename: PathBuf,
    tabsize: u8,
    data: Vec<u8>,
    content: String,
    lines: Vec<String>,
    styled_lines: Vec<Vec<(Style, String)>>,
    first_line: usize,

    expression: Option<Regex>,
    lines_with_matches: Vec<bool>,
    backwards: bool,
    search_pos: usize,
}

impl TextViewer {
    pub fn new(
        config: &Config,
        pubsub_tx: Sender<PubSub>,
        rect: &Rect,
        filename: &Path,
        tabsize: u8,
    ) -> Result<TextViewer> {
        let data = fs::read(filename)?;

        let tab_size = if tabsize > 0 {
            tabsize
        } else {
            config.viewer.tab_size
        };

        let content = match str::from_utf8(&data) {
            Ok(content) => String::from(content),
            Err(e) => {
                // TODO: Instead of a fallback to WINDOWS_1252, we could use chardetng
                // to find the correct encoding
                match WINDOWS_1252.decode_without_bom_handling_and_without_replacement(&data) {
                    Some(content) => String::from(content),
                    None => return Err(e.into()),
                }
            }
        };

        let lines: Vec<String> = LinesWithEndings::from(&content)
            .map(|e| expand_tabs_for_line(e, tab_size.into()))
            .collect();

        // Default to unstyled text
        let styled_lines: Vec<Vec<(Style, String)>> = lines
            .iter()
            .map(|line| {
                vec![(
                    Style::default().fg(config.highlight.base05),
                    String::from(line),
                )]
            })
            .collect();

        let viewer = TextViewer {
            config: *config,
            pubsub_tx,
            rect: *rect,
            filename: filename.to_path_buf(),
            tabsize: tab_size,
            data,
            content,
            lines,
            styled_lines,
            first_line: 0,

            expression: None,
            lines_with_matches: Vec::new(),
            backwards: false,
            search_pos: 0,
        };

        viewer.highlight();

        Ok(viewer)
    }

    fn highlight(&self) {
        let filename = self.filename.clone();
        let lines = self.lines.clone();
        let config = self.config;
        let pubsub_tx = self.pubsub_tx.clone();

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

            let mut highlighter = HighlightLines::new(syntax, theme);
            let styled_lines: Vec<Vec<(Style, String)>> = lines
                .iter()
                .map(|line| {
                    highlighter
                        .highlight_line(line, syntax_set)
                        .unwrap()
                        .iter()
                        .map(|(style, text)| {
                            (
                                Style::default().fg(match style.foreground.r {
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
                                }),
                                String::from(*text),
                            )
                        })
                        .collect()
                })
                .collect();

            pubsub_tx.send(PubSub::Highlight(styled_lines)).unwrap();
        });
    }

    pub fn resize(&mut self, rect: &Rect) {
        self.rect = *rect;

        self.clamp_first_line();
    }

    pub fn clamp_first_line(&mut self) {
        self.first_line = match self.first_line {
            i if (i + (self.rect.height as usize)) > self.lines.len() => {
                self.lines.len().saturating_sub(self.rect.height as usize)
            }
            i => i,
        };
    }

    pub fn search_next(&mut self) {
        if let Some(_re) = &self.expression {
            self.search_pos = match self
                .lines_with_matches
                .iter()
                .skip(self.search_pos + 1)
                .position(|&matches| matches)
            {
                Some(pos) => self.search_pos + 1 + pos,
                None => self
                    .lines_with_matches
                    .iter()
                    .position(|&matches| matches)
                    .unwrap(),
            };

            self.first_line = self.search_pos;
            self.clamp_first_line();
        }
    }

    pub fn search_prev(&mut self) {
        if let Some(_re) = &self.expression {
            self.search_pos = match self
                .lines_with_matches
                .iter()
                .rev()
                .skip(self.lines_with_matches.len().wrapping_sub(self.search_pos))
                .position(|&matches| matches)
            {
                Some(pos) => self.search_pos.saturating_sub(1).saturating_sub(pos),
                None => self
                    .lines_with_matches
                    .len()
                    .saturating_sub(1)
                    .saturating_sub(
                        self.lines_with_matches
                            .iter()
                            .rev()
                            .position(|&matches| matches)
                            .unwrap(),
                    ),
            };

            self.first_line = self.search_pos;
            self.clamp_first_line();
        }
    }
}

impl Component for TextViewer {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        let mut key_handled = true;

        match key {
            Key::Up | Key::Char('k') => {
                self.first_line = match self.first_line {
                    i if i > 0 => i - 1,
                    _ => 0,
                };
                self.search_pos = self.first_line;
            }
            Key::Down | Key::Char('j') => {
                self.first_line = match self.first_line {
                    i if (i + 1 + (self.rect.height as usize)) <= self.lines.len() => i + 1,
                    i => i,
                };
                self.search_pos = self.first_line;
            }
            Key::Home | Key::Char('g') => {
                self.first_line = 0;
                self.search_pos = self.first_line;
            }
            Key::End | Key::Char('G') => {
                self.first_line = self.lines.len().saturating_sub(self.rect.height as usize);
                self.search_pos = self.first_line;
            }
            Key::PageUp | Key::Ctrl('b') => {
                self.first_line = self
                    .first_line
                    .saturating_sub((self.rect.height as usize) - 1);
                self.search_pos = self.first_line;
            }
            Key::PageDown | Key::Ctrl('f') => {
                self.first_line = match self.first_line {
                    i if (i + ((self.rect.height as usize) - 1) + (self.rect.height as usize))
                        <= self.lines.len() =>
                    {
                        i + ((self.rect.height as usize) - 1)
                    }
                    _ => self.lines.len().saturating_sub(self.rect.height as usize),
                };
                self.search_pos = self.first_line;
            }
            Key::Char(':') | Key::F(5) => {
                // TODO: Don't show the dialog if the file size is 0
                self.pubsub_tx
                    .send(PubSub::DlgGoto(GotoType::LineNumber))
                    .unwrap();
            }
            Key::Char('/') | Key::Char('?') | Key::Char('f') | Key::Char('F') | Key::F(7) => {
                // TODO: Don't show the dialog if the file size is 0
                self.pubsub_tx
                    .send(PubSub::DlgTextSearch(TextSearch {
                        search_string: String::from(""),
                        search_type: match key {
                            Key::Char('/') | Key::Char('?') => SearchType::Regex,
                            _ => SearchType::Normal,
                        },
                        case_sensitive: false,
                        backwards: match key {
                            Key::Char('?') | Key::Char('F') => true,
                            _ => false,
                        },
                        whole_words: false,
                    }))
                    .unwrap();
            }
            Key::Char('n') => match self.backwards {
                true => self.search_prev(),
                false => self.search_next(),
            },
            Key::Char('N') => match self.backwards {
                true => self.search_next(),
                false => self.search_prev(),
            },
            Key::Esc => self.expression = None,
            _ => key_handled = false,
        }

        Ok(key_handled)
    }

    fn handle_pubsub(&mut self, event: &PubSub) -> Result<()> {
        match event {
            PubSub::Highlight(styled_lines) => self.styled_lines = styled_lines.to_vec(),
            PubSub::Goto(GotoType::LineNumber, str_line_number) => {
                match str_line_number.parse::<usize>() {
                    Ok(line_number) => {
                        self.first_line = if (line_number.saturating_sub(1)
                            + (self.rect.height as usize))
                            <= self.lines.len()
                        {
                            line_number.saturating_sub(1)
                        } else {
                            self.lines.len().saturating_sub(self.rect.height as usize)
                        };
                        self.search_pos = self.first_line;
                    }
                    Err(_) => {
                        self.pubsub_tx
                            .send(PubSub::Error(format!(
                                "Invalid number: {}",
                                str_line_number
                            )))
                            .unwrap();
                    }
                }
            }
            PubSub::TextSearch(search) => {
                if search.search_string.len() == 0 {
                    self.expression = None;
                    return Ok(());
                }

                self.backwards = search.backwards;

                let expression = match search.search_type {
                    SearchType::Normal => regex::escape(&search.search_string),
                    SearchType::Regex => String::from(&search.search_string),
                    SearchType::Wildcard => {
                        let re = fnmatch::translate(&search.search_string);

                        String::from(&re[..(re.len() - 2)])
                    }
                };

                let expression = match search.whole_words {
                    true => format!(r"\b{}\b", expression),
                    false => expression,
                };

                self.expression = match RegexBuilder::new(&expression)
                    .case_insensitive(!search.case_sensitive)
                    .build()
                {
                    Ok(re) => {
                        self.lines_with_matches = self
                            .lines
                            .iter()
                            .map(|line| re.is_match(line.trim_end_matches('\n')))
                            .collect();

                        match self.lines_with_matches.iter().any(|&matches| matches) {
                            true => Some(re),
                            false => {
                                self.pubsub_tx
                                    .send(PubSub::Warning(
                                        String::from("Search"),
                                        String::from("Search string not found"),
                                    ))
                                    .unwrap();

                                None
                            }
                        }
                    }
                    Err(_) => {
                        self.pubsub_tx
                            .send(PubSub::Error(String::from("Invalid search string")))
                            .unwrap();

                        None
                    }
                };

                if let Some(_re) = &self.expression {
                    self.search_pos = self.first_line;

                    if !self.lines_with_matches[self.search_pos] {
                        match self.backwards {
                            true => self.search_prev(),
                            false => self.search_next(),
                        }
                    }
                }
            }
            _ => (),
        }

        Ok(())
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let line_number_width = self.lines.len().to_string().len();
        let widths = [
            Constraint::Length((line_number_width + 1) as u16),
            Constraint::Length(chunk.width.saturating_sub((line_number_width + 1) as u16)),
        ];

        let highlighted_lines: Vec<Vec<(Style, String)>> = self
            .styled_lines
            .iter()
            .skip(self.first_line)
            .take(chunk.height.into())
            .enumerate()
            .map(|(i, e)| {
                match (
                    &self.expression,
                    self.lines_with_matches.get(self.first_line + i),
                ) {
                    (Some(re), Some(true)) => {
                        let line = &self.lines[self.first_line + i];

                        let mut v = Vec::new();
                        let mut bytes_written = 0;
                        let mut i_e = 0;
                        let mut i_text = 0;
                        for m in re.find_iter(line.trim_end_matches('\n')) {
                            while bytes_written < m.start() {
                                let (color, text) = &e[i_e];

                                if (bytes_written + (text.len() - i_text)) <= m.start() {
                                    v.push((*color, String::from(&text[i_text..])));
                                    bytes_written += text.len() - i_text;
                                    i_e += 1;
                                    i_text = 0;
                                } else {
                                    let end = i_text + (m.start() - bytes_written);

                                    v.push((*color, String::from(&text[i_text..end])));
                                    i_text = end;
                                    bytes_written = m.start();
                                }
                            }

                            v.push((
                                Style::default()
                                    .fg(if (self.first_line + i) == self.search_pos {
                                        self.config.ui.markselect_fg
                                    } else {
                                        self.config.ui.selected_fg
                                    })
                                    .bg(self.config.ui.selected_bg),
                                String::from(m.as_str()),
                            ));

                            while bytes_written < m.end() {
                                let (_color, text) = &e[i_e];

                                if (bytes_written + (text.len() - i_text)) <= m.end() {
                                    bytes_written += text.len() - i_text;
                                    i_e += 1;
                                    i_text = 0;
                                } else {
                                    i_text += m.end() - bytes_written;
                                    bytes_written = m.end();
                                }
                            }
                        }

                        while bytes_written < line.len() {
                            let (color, text) = &e[i_e];

                            if (bytes_written + (text.len() - i_text)) <= line.len() {
                                v.push((*color, String::from(&text[i_text..])));
                                bytes_written += text.len() - i_text;
                                i_e += 1;
                                i_text = 0;
                            } else {
                                let end = i_text + (line.len() - bytes_written);

                                v.push((*color, String::from(&text[i_text..end])));
                                i_text = end;
                                bytes_written = line.len();
                            }
                        }

                        v
                    }
                    _ => e.to_vec(),
                }
            })
            .collect();

        let items: Vec<Row> = highlighted_lines
            .iter()
            .enumerate()
            .map(|(i, e)| {
                Row::new(vec![
                    Cell::from(Span::styled(
                        format!(
                            "{:width$} ",
                            self.first_line + i + 1,
                            width = line_number_width
                        ),
                        Style::default().fg(Color::White),
                    )),
                    Cell::from(Line::from(
                        e.iter()
                            .map(|(color, text)| Span::styled(text, *color))
                            .collect::<Vec<_>>(),
                    )),
                ])
            })
            .collect();

        let items = Table::new(items, widths)
            .block(Block::default().style(Style::default().bg(self.config.highlight.base00)))
            .column_spacing(0);

        f.render_widget(items, *chunk);
    }
}
