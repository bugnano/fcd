use std::{
    cmp::{max, min},
    path::{Path, PathBuf},
    rc::Rc,
    str, thread,
};

use crossbeam_channel::{Receiver, Sender};
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use bat::assets::HighlightingAssets;
use encoding_rs::WINDOWS_1252;
use regex::{self, Regex, RegexBuilder};
use syntect::{easy::HighlightLines, util::LinesWithEndings};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    fnmatch,
    viewer::{
        dlg_goto::GotoType,
        dlg_text_search::{SearchType, TextSearch},
    },
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

#[derive(Debug, Clone)]
enum ComponentPubSub {
    Highlight(Vec<Vec<(String, Style)>>),
}

#[derive(Debug)]
pub struct TextViewer {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    component_pubsub_tx: Sender<ComponentPubSub>,
    component_pubsub_rx: Receiver<ComponentPubSub>,
    highlight_tx: Sender<()>,
    highlight_rx: Receiver<()>,
    rect: Rect,
    filename: PathBuf,
    filename_str: String,
    tabsize: u8,
    data: Vec<u8>,
    line_offset: Vec<usize>,
    lines: Vec<String>,
    styled_lines: Vec<Vec<(String, Style)>>,
    first_line: usize,
    wrap: bool,

    expression: Option<Regex>,
    lines_with_matches: Vec<bool>,
    backwards: bool,
    search_pos: usize,
}

impl TextViewer {
    pub fn new(
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        filename: &Path,
        filename_str: &str,
        tabsize: u8,
        data: Vec<u8>,
    ) -> TextViewer {
        let tab_size = if tabsize > 0 {
            tabsize
        } else {
            config.viewer.tab_size
        };

        let content = match str::from_utf8(&data) {
            Ok(content) => content.replace("\r\n", "\n").replace('\r', "\n"),
            Err(e) => {
                // TODO: Instead of a fallback to WINDOWS_1252, we could use chardetng
                // to find the correct encoding
                match WINDOWS_1252.decode_without_bom_handling_and_without_replacement(&data) {
                    Some(content) => content.replace("\r\n", "\n").replace('\r', "\n"),
                    None => panic!("{}", e),
                }
            }
        };

        let lines: Vec<String> = LinesWithEndings::from(&content)
            .map(|e| expand_tabs_for_line(e, tab_size.into()))
            .collect();

        // Default to unstyled text
        let styled_lines: Vec<Vec<(String, Style)>> = lines
            .iter()
            .map(|line| {
                vec![(
                    String::from(line),
                    Style::default().fg(config.highlight.base05),
                )]
            })
            .collect();

        let mut total_offset = 0;
        let line_offset: Vec<usize> = std::iter::once(total_offset)
            .chain(data.split_inclusive(|&c| c == 0x0A).map(|line| {
                total_offset += line.len();
                total_offset
            }))
            .collect();

        let (component_pubsub_tx, component_pubsub_rx) = crossbeam_channel::unbounded();
        let (highlight_tx, highlight_rx) = crossbeam_channel::unbounded();

        let mut viewer = TextViewer {
            config: Rc::clone(config),
            pubsub_tx,
            component_pubsub_tx,
            component_pubsub_rx,
            highlight_tx,
            highlight_rx,
            rect: Rect::default(),
            filename: filename.to_path_buf(),
            filename_str: String::from(filename_str),
            tabsize: tab_size,
            data,
            line_offset,
            lines,
            styled_lines,
            first_line: 0,
            wrap: false,

            expression: None,
            lines_with_matches: Vec::new(),
            backwards: false,
            search_pos: 0,
        };

        viewer.send_updated_position();
        viewer.highlight();

        viewer
    }

    fn handle_component_pubsub(&mut self) {
        if let Ok(event) = self.component_pubsub_rx.try_recv() {
            match event {
                ComponentPubSub::Highlight(styled_lines) => {
                    self.styled_lines = styled_lines.to_vec();
                }
            }
        }
    }

    fn highlight(&self) {
        let filename = self.filename.clone();
        let lines = self.lines.clone();
        let config = *self.config.clone();
        let component_pubsub_tx = self.component_pubsub_tx.clone();
        let pubsub_tx = self.pubsub_tx.clone();
        let highlight_rx = self.highlight_rx.clone();

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
            let styled_lines: Vec<Vec<(String, Style)>> = lines
                .iter()
                .map_while(|line| {
                    if !highlight_rx.is_empty() {
                        return None;
                    }

                    Some(
                        highlighter
                            .highlight_line(line, syntax_set)
                            .unwrap()
                            .iter()
                            .map(|(style, text)| {
                                (
                                    String::from(text.trim_end_matches('\n')),
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
                                            log::debug!("{:?}", style);
                                            config.highlight.base05
                                        }
                                    }),
                                )
                            })
                            .collect(),
                    )
                })
                .collect();

            if highlight_rx.is_empty() {
                // First send the component event
                let _ = component_pubsub_tx.send(ComponentPubSub::Highlight(styled_lines));

                // Then notify the app that there is an component event
                let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
            }
        });
    }

    fn clamp_first_line(&mut self) {
        match self.wrap {
            true => {
                if (self.first_line + 1) > self.lines.len() {
                    self.first_line = self.lines.len().saturating_sub(1);
                }
            }
            false => {
                if (self.first_line + (self.rect.height as usize)) > self.lines.len() {
                    self.first_line = self.lines.len().saturating_sub(self.rect.height as usize);
                }
            }
        }
    }

    fn send_updated_position(&mut self) {
        let current_line = match self.wrap {
            true => self.first_line + 1,
            false => self.first_line + (self.rect.height as usize),
        };

        self.pubsub_tx
            .send(PubSub::FileInfo(
                String::from(&self.filename_str),
                format!(
                    "{}/{}",
                    min(current_line, self.lines.len()),
                    self.lines.len()
                ),
                format!(
                    "{:3}%",
                    match self.lines.len() {
                        0 => 100,
                        n => (min(current_line, n) * 100) / n,
                    }
                ),
            ))
            .unwrap();
    }

    fn search_next(&mut self) {
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

            let old_first_line = self.first_line;

            self.first_line = self.search_pos;
            self.clamp_first_line();

            if self.first_line != old_first_line {
                self.send_updated_position();
            }
        }
    }

    fn search_prev(&mut self) {
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

            let old_first_line = self.first_line;

            self.first_line = self.search_pos;
            self.clamp_first_line();

            if self.first_line != old_first_line {
                self.send_updated_position();
            }
        }
    }
}

impl Component for TextViewer {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Up | Key::Char('k') => {
                let old_first_line = self.first_line;

                self.first_line = self.first_line.saturating_sub(1);
                self.clamp_first_line();

                if self.first_line != old_first_line {
                    self.send_updated_position();
                }

                self.search_pos = self.first_line;
            }
            Key::Down | Key::Char('j') => {
                let old_first_line = self.first_line;

                self.first_line += 1;
                self.clamp_first_line();

                if self.first_line != old_first_line {
                    self.send_updated_position();
                }

                self.search_pos = self.first_line;
            }
            Key::Home | Key::Char('g') => {
                let old_first_line = self.first_line;

                self.first_line = 0;
                self.clamp_first_line();

                if self.first_line != old_first_line {
                    self.send_updated_position();
                }

                self.search_pos = self.first_line;
            }
            Key::End | Key::Char('G') => {
                let old_first_line = self.first_line;

                self.first_line = self.lines.len();
                self.clamp_first_line();

                if self.first_line != old_first_line {
                    self.send_updated_position();
                }

                self.search_pos = self.first_line;
            }
            Key::PageUp | Key::Ctrl('b') => {
                let old_first_line = self.first_line;

                self.first_line = self
                    .first_line
                    .saturating_sub((self.rect.height as usize).saturating_sub(1));
                self.clamp_first_line();

                if self.first_line != old_first_line {
                    self.send_updated_position();
                }

                self.search_pos = self.first_line;
            }
            Key::PageDown | Key::Ctrl('f') => {
                let old_first_line = self.first_line;

                self.first_line += (self.rect.height as usize).saturating_sub(1);
                self.clamp_first_line();

                if self.first_line != old_first_line {
                    self.send_updated_position();
                }

                self.search_pos = self.first_line;
            }
            Key::Char('h') | Key::F(4) => {
                self.pubsub_tx.send(PubSub::ToggleHex).unwrap();
                self.pubsub_tx
                    .send(PubSub::ToHexOffset(
                        self.line_offset[self.first_line] as u64,
                    ))
                    .unwrap();
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
                        search_string: String::new(),
                        search_type: match key {
                            Key::Char('/') | Key::Char('?') => SearchType::Regex,
                            _ => SearchType::Normal,
                        },
                        case_sensitive: false,
                        backwards: matches!(key, Key::Char('?') | Key::Char('F')),
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
            Key::Char('w') | Key::F(2) => {
                self.wrap = !self.wrap;
                self.send_updated_position();
            }
            _ => key_handled = false,
        }

        key_handled
    }

    fn handle_pubsub(&mut self, event: &PubSub) {
        match event {
            PubSub::ComponentThreadEvent => self.handle_component_pubsub(),
            PubSub::Goto(GotoType::LineNumber, str_line_number) => {
                match str_line_number.parse::<usize>() {
                    Ok(line_number) => {
                        let old_first_line = self.first_line;

                        self.first_line = line_number.saturating_sub(1);
                        self.clamp_first_line();

                        if self.first_line != old_first_line {
                            self.send_updated_position();
                        }

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
            PubSub::FromHexOffset(offset) => {
                self.first_line = match self.line_offset.binary_search(&(*offset as usize)) {
                    Ok(line_number) => line_number,
                    Err(line_number) => line_number,
                };
                self.clamp_first_line();

                self.send_updated_position();

                self.search_pos = self.first_line;
            }
            PubSub::TextSearch(search) => {
                if search.search_string.is_empty() {
                    self.expression = None;
                    return;
                }

                self.backwards = search.backwards;

                let expression = match search.search_type {
                    SearchType::Normal => regex::escape(&search.search_string),
                    SearchType::Regex => String::from(&search.search_string),
                    SearchType::Wildcard => {
                        let re = fnmatch::translate(&search.search_string);

                        String::from(&re[2..(re.len() - 2)])
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
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let old_height = self.rect.height;
        let old_first_line = self.first_line;

        self.rect = *chunk;
        self.clamp_first_line();

        if (self.rect.height != old_height) || (self.first_line != old_first_line) {
            self.send_updated_position();
        }

        let line_number_width = self.lines.len().to_string().len();
        let text_width = chunk.width.saturating_sub((line_number_width + 1) as u16);
        let widths = [
            Constraint::Length((line_number_width + 1) as u16),
            Constraint::Length(text_width),
        ];

        let highlighted_lines: Vec<Vec<(String, Style)>> = self
            .styled_lines
            .iter()
            .skip(self.first_line)
            .take(chunk.height.into())
            .enumerate()
            .map(|(i, e)| {
                let highlighted_style = Style::default()
                    .fg(if (self.first_line + i) == self.search_pos {
                        self.config.ui.markselect_fg
                    } else {
                        self.config.ui.selected_fg
                    })
                    .bg(self.config.ui.selected_bg);

                match (
                    &self.expression,
                    self.lines_with_matches.get(self.first_line + i),
                ) {
                    (Some(re), Some(true)) => {
                        let line = &self.lines[self.first_line + i].trim_end_matches('\n');

                        let mut v = Vec::new();
                        let mut bytes_written = 0;
                        let mut i_e = 0;
                        let mut i_text = 0;
                        for m in re.find_iter(line) {
                            while bytes_written < m.start() {
                                let (text, style) = &e[i_e];

                                if (bytes_written + (text.len() - i_text)) <= m.start() {
                                    v.push((String::from(&text[i_text..]), *style));
                                    bytes_written += text.len() - i_text;
                                    i_e += 1;
                                    i_text = 0;
                                } else {
                                    let end = i_text + (m.start() - bytes_written);

                                    v.push((String::from(&text[i_text..end]), *style));
                                    i_text = end;
                                    bytes_written = m.start();
                                }
                            }

                            v.push((String::from(m.as_str()), highlighted_style));

                            while bytes_written < m.end() {
                                let (text, _style) = &e[i_e];

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
                            let (text, style) = &e[i_e];

                            if (bytes_written + (text.len() - i_text)) <= line.len() {
                                v.push((String::from(&text[i_text..]), *style));
                                bytes_written += text.len() - i_text;
                                i_e += 1;
                                i_text = 0;
                            } else {
                                let end = i_text + (line.len() - bytes_written);

                                v.push((String::from(&text[i_text..end]), *style));
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

        let wrapped_lines: Vec<Vec<Line>> = highlighted_lines
            .iter()
            .map(|line| match self.wrap {
                true => {
                    let mut lines = Vec::new();
                    let mut current_line = Vec::new();
                    let mut current_width = 0;
                    for (text, style) in line {
                        if (current_width + text.width()) <= text_width.into() {
                            current_line.push(Span::styled(text, *style));
                            current_width += text.width();
                        } else {
                            let mut s = String::new();

                            for c in text.chars() {
                                if current_width + c.width().unwrap_or(0) > text_width.into() {
                                    current_line.push(Span::styled(s.clone(), *style));
                                    lines.push(Line::from(current_line.clone()));
                                    s.clear();
                                    current_line.clear();
                                    current_width = 0;
                                }

                                s.push(c);
                                current_width += c.width().unwrap_or(0);
                            }

                            if !s.is_empty() {
                                current_line.push(Span::styled(s.clone(), *style));
                            }
                        }
                    }

                    if !current_line.is_empty() {
                        lines.push(Line::from(current_line.clone()));
                    }

                    lines
                }
                false => vec![Line::from(
                    line.iter()
                        .map(|(text, style)| Span::styled(text, *style))
                        .collect::<Vec<Span>>(),
                )],
            })
            .collect();

        let items: Vec<Row> = wrapped_lines
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
                        Style::default().fg(self.config.viewer.lineno_fg),
                    )),
                    Cell::from(e.clone()),
                ])
                .height(max(e.len() as u16, 1))
            })
            .collect();

        let table = Table::new(items, widths)
            .block(Block::default().style(Style::default().bg(self.config.highlight.base00)))
            .column_spacing(0);

        f.render_widget(table, *chunk);
    }
}

impl Drop for TextViewer {
    fn drop(&mut self) {
        let _ = self.highlight_tx.send(());
    }
}
