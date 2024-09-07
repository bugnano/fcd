use std::{cmp::min, path::Path, rc::Rc, str};

use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use regex::{self, Regex, RegexBuilder};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::PubSub,
    component::{Component, Focus},
    fm::entry::Entry,
    fnmatch,
    palette::Palette,
    tilde_layout::{tilde_layout, tilde_layout_styled},
    viewer::{
        dlg_goto::GotoType,
        dlg_text_search::{SearchType, TextSearch},
    },
};

#[derive(Debug)]
pub struct DirViewer {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    rect: Rect,
    filename_str: String,
    file_list: Vec<Entry>,
    first_line: usize,

    expression: Option<Regex>,
    lines_with_matches: Vec<bool>,
    backwards: bool,
    search_pos: usize,
}

impl DirViewer {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        _filename: &Path,
        filename_str: &str,
        file_list: Vec<Entry>,
    ) -> DirViewer {
        let mut viewer = DirViewer {
            palette: Rc::clone(palette),
            pubsub_tx,
            rect: Rect::default(),
            filename_str: String::from(filename_str),
            file_list,
            first_line: 0,

            expression: None,
            lines_with_matches: Vec::new(),
            backwards: false,
            search_pos: 0,
        };

        viewer.send_updated_position();

        viewer
    }

    pub fn clamp_first_line(&mut self) {
        if (self.first_line + (self.rect.height as usize)) > self.file_list.len() {
            self.first_line = self
                .file_list
                .len()
                .saturating_sub(self.rect.height as usize);
        }
    }

    pub fn send_updated_position(&mut self) {
        let current_line = self.first_line + (self.rect.height as usize);

        self.pubsub_tx
            .send(PubSub::FileInfo(
                String::from(&self.filename_str),
                format!(
                    "{}/{}",
                    min(current_line, self.file_list.len()),
                    self.file_list.len()
                ),
                format!(
                    "{:3}%",
                    match self.file_list.len() {
                        0 => 100,
                        n => (min(current_line, n) * 100) / n,
                    }
                ),
            ))
            .unwrap();
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

            let old_first_line = self.first_line;

            self.first_line = self.search_pos;
            self.clamp_first_line();

            if self.first_line != old_first_line {
                self.send_updated_position();
            }
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

            let old_first_line = self.first_line;

            self.first_line = self.search_pos;
            self.clamp_first_line();

            if self.first_line != old_first_line {
                self.send_updated_position();
            }
        }
    }

    fn handle_up(&mut self) {
        let old_first_line = self.first_line;

        self.first_line = self.first_line.saturating_sub(1);
        self.clamp_first_line();

        if self.first_line != old_first_line {
            self.send_updated_position();
        }

        self.search_pos = self.first_line;
    }

    fn handle_down(&mut self) {
        let old_first_line = self.first_line;

        self.first_line += 1;
        self.clamp_first_line();

        if self.first_line != old_first_line {
            self.send_updated_position();
        }

        self.search_pos = self.first_line;
    }
}

impl Component for DirViewer {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Up | Key::Char('k') => self.handle_up(),
            Key::Down | Key::Char('j') => self.handle_down(),
            Key::Home | Key::CtrlHome | Key::Char('g') => {
                let old_first_line = self.first_line;

                self.first_line = 0;
                self.clamp_first_line();

                if self.first_line != old_first_line {
                    self.send_updated_position();
                }

                self.search_pos = self.first_line;
            }
            Key::End | Key::CtrlEnd | Key::Char('G') => {
                let old_first_line = self.first_line;

                self.first_line = self.file_list.len();
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
            Key::Char(':') | Key::F(5) | Key::Char('5') => {
                // TODO: Don't show the dialog if the file size is 0
                self.pubsub_tx
                    .send(PubSub::DlgGoto(GotoType::LineNumber))
                    .unwrap();
            }
            Key::Char('/')
            | Key::Char('?')
            | Key::Char('f')
            | Key::Char('F')
            | Key::F(7)
            | Key::Char('7') => {
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
            _ => key_handled = false,
        }

        key_handled
    }

    fn handle_mouse(&mut self, button: MouseButton, _mouse_position: Position) {
        match button {
            MouseButton::WheelUp => self.handle_up(),
            MouseButton::WheelDown => self.handle_down(),
            _ => {}
        }
    }

    fn handle_pubsub(&mut self, event: &PubSub) {
        match event {
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
                            .send(PubSub::Error(
                                format!("Invalid number: {}", str_line_number),
                                None,
                            ))
                            .unwrap();
                    }
                }
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
                            .file_list
                            .iter()
                            .map(|entry| re.is_match(&entry.label))
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
                            .send(PubSub::Error(String::from("Invalid search string"), None))
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

        let highlighted_lines: Vec<Vec<(String, Style)>> = self
            .file_list
            .iter()
            .skip(self.first_line)
            .take(chunk.height.into())
            .enumerate()
            .map(|(i, entry)| {
                let filename_max_width = (chunk.width as usize)
                    .saturating_sub(entry.shown_size.width())
                    .saturating_sub(9);

                let filename = tilde_layout(&entry.label, filename_max_width);
                let filename_width = filename.width();

                let normal_style = entry.style;

                let highlighted_style = if (self.first_line + i) == self.search_pos {
                    self.palette.markselect
                } else {
                    self.palette.selected
                };

                let mut line = match (
                    &self.expression,
                    self.lines_with_matches.get(self.first_line + i),
                ) {
                    (Some(re), Some(true)) => {
                        let mut v = Vec::new();
                        let mut i_text = 0;

                        for m in re.find_iter(&entry.label) {
                            v.push((String::from(&entry.label[i_text..m.start()]), normal_style));

                            v.push((String::from(m.as_str()), highlighted_style));

                            i_text = m.end();
                        }

                        if i_text < entry.label.len() {
                            v.push((String::from(&entry.label[i_text..]), normal_style));
                        }

                        tilde_layout_styled(&v, filename_max_width)
                    }
                    _ => vec![(filename, normal_style)],
                };

                // The reason why I add {:width$} whitespaces after the
                // filename instead of putting the filename directly
                // inside {:width$} is because the {:width$} formatting
                // has a bug with some 0-width Unicode characters
                line.push((
                    format!(
                        "{:width$} {} {}",
                        "",
                        &entry.shown_size,
                        &entry.shown_mtime,
                        width = filename_max_width.saturating_sub(filename_width)
                    ),
                    normal_style,
                ));

                line
            })
            .collect();

        let items: Vec<ListItem> = highlighted_lines
            .iter()
            .map(|line| {
                Line::from(
                    line.iter()
                        .map(|(label, style)| Span::styled(label, *style))
                        .collect::<Vec<Span>>(),
                )
                .into()
            })
            .collect();

        let items = List::new(items).block(Block::default().style(self.palette.panel));

        f.render_widget(items, *chunk);
    }
}
