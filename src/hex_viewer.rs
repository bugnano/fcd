use std::{
    cmp::{max, min},
    fs::File,
    io::{BufReader, Read, Seek},
    path::Path,
    str,
};

use anyhow::Result;
use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    dlg_goto::GotoType,
    dlg_hex_search::HexSearch,
};

pub fn search_next_from_pos(
    expression: &[u8],
    reader: &mut BufReader<File>,
    file_length: u64,
    pos: u64,
) -> Option<u64> {
    let mut search_pos = pos;
    let mut buffer: Vec<u8> = Vec::new();

    loop {
        let buf_size = min(file_length.saturating_sub(search_pos), 131072);
        if buffer.len() != (buf_size as usize) {
            buffer.resize(buf_size as usize, 0);
        }

        let stream_position = reader.stream_position().unwrap() as i64;

        reader
            .seek_relative((search_pos as i64) - stream_position)
            .unwrap();

        reader.read_exact(&mut buffer).unwrap();

        match buffer
            .windows(expression.len())
            .position(|window| window == expression)
        {
            Some(pos) => {
                search_pos += pos as u64;

                return Some(search_pos);
            }
            None => {
                search_pos += buffer.len() as u64;

                if search_pos >= file_length {
                    break;
                }

                search_pos = search_pos.saturating_sub(expression.len() as u64);
            }
        }
    }

    None
}

pub fn search_prev_from_pos(
    expression: &[u8],
    reader: &mut BufReader<File>,
    file_length: u64,
    pos: u64,
) -> Option<u64> {
    let mut search_pos = pos;
    let mut buffer: Vec<u8> = Vec::new();

    loop {
        let size = min(131072, search_pos);

        search_pos = search_pos.saturating_sub(size);

        let buf_size = min(file_length.saturating_sub(size), 131072);
        if buffer.len() != (buf_size as usize) {
            buffer.resize(buf_size as usize, 0);
        }

        let stream_position = reader.stream_position().unwrap() as i64;
        reader
            .seek_relative((search_pos as i64) - stream_position)
            .unwrap();

        reader.read_exact(&mut buffer).unwrap();

        match buffer
            .windows(expression.len())
            .rposition(|window| window == expression)
        {
            Some(pos) => {
                search_pos += pos as u64;
                return Some(search_pos);
            }
            None => {
                if search_pos == 0 {
                    break;
                }

                search_pos = min(search_pos + (expression.len() as u64), file_length);
            }
        }
    }

    None
}

#[derive(Debug, Copy, Clone)]
pub enum ViewerType {
    Hex,
    Dump,
}

#[derive(Debug)]
pub struct HexViewer {
    config: Config,
    pubsub_tx: Sender<PubSub>,
    rect: Rect,
    filename_str: String,
    file_length: u64,
    viewer_type: ViewerType,
    reader: BufReader<File>,
    offset: u64,
    len_address: usize,
    line_width: usize,

    expression: Option<Vec<u8>>,
    backwards: bool,
    search_pos: u64,
}

impl HexViewer {
    pub fn new(
        config: &Config,
        pubsub_tx: Sender<PubSub>,
        filename: &Path,
        filename_str: &str,
        file_length: u64,
        viewer_type: ViewerType,
    ) -> Result<HexViewer> {
        let f = File::open(filename)?;
        let reader = BufReader::with_capacity(131072, f);

        let len_address = format!("{:X}", file_length).len();

        let mut viewer = HexViewer {
            config: *config,
            pubsub_tx,
            rect: Rect::default(),
            filename_str: String::from(filename_str),
            file_length,
            viewer_type,
            reader,
            offset: 0,
            len_address: max(len_address + (len_address % 2), 8),
            line_width: 0,

            expression: None,
            backwards: false,
            search_pos: 0,
        };

        viewer.send_updated_position();

        Ok(viewer)
    }

    pub fn clamp_offset(&mut self) {
        let last_line = match self.line_width {
            0 => self.file_length,
            w => match self.file_length % (w as u64) {
                0 => self.file_length,
                n => self.file_length.saturating_sub(n) + (w as u64),
            },
        };

        if (self.offset + ((self.rect.height as u64) * (self.line_width as u64))) > last_line {
            self.offset =
                last_line.saturating_sub((self.rect.height as u64) * (self.line_width as u64));
        }
    }

    pub fn send_updated_position(&mut self) {
        match self.viewer_type {
            ViewerType::Hex => {
                let width = (self.rect.width as usize).saturating_sub(self.len_address + 4);
                let num_dwords = max(width / 17, 1);

                self.line_width = num_dwords * 4;
            }
            ViewerType::Dump => {
                let width = (self.rect.width as usize).saturating_sub(self.len_address + 1);

                self.line_width = max(width.saturating_sub(width % 16), 16);
            }
        }

        let offset = self.offset + ((self.rect.height as u64) * (self.line_width as u64));

        self.pubsub_tx
            .send(PubSub::FileInfo(
                String::from(&self.filename_str),
                format!(
                    "{:0width$X}/{:0width$X}",
                    min(offset, self.file_length),
                    self.file_length,
                    width = self.len_address
                ),
                format!(
                    "{:3}%",
                    match self.file_length {
                        0 => 100,
                        n => (min(offset, n) * 100) / n,
                    }
                ),
            ))
            .unwrap();
    }

    pub fn search_next(&mut self) -> bool {
        if let Some(e) = &self.expression {
            match search_next_from_pos(e, &mut self.reader, self.file_length, self.search_pos)
                .or_else(|| search_next_from_pos(e, &mut self.reader, self.file_length, 0))
            {
                Some(pos) => {
                    self.search_pos = pos;

                    let old_offset = self.offset;

                    self.offset = pos.saturating_sub(match self.line_width {
                        0 => 0,
                        w => pos % (w as u64),
                    });
                    self.clamp_offset();

                    if self.offset != old_offset {
                        self.send_updated_position();
                    }
                }
                None => return false,
            }
        }

        true
    }

    pub fn search_prev(&mut self) -> bool {
        if let Some(e) = &self.expression {
            match search_prev_from_pos(e, &mut self.reader, self.file_length, self.search_pos)
                .or_else(|| {
                    search_prev_from_pos(e, &mut self.reader, self.file_length, self.file_length)
                }) {
                Some(pos) => {
                    self.search_pos = pos;

                    let old_offset = self.offset;

                    self.offset = pos.saturating_sub(match self.line_width {
                        0 => 0,
                        w => pos % (w as u64),
                    });
                    self.clamp_offset();

                    if self.offset != old_offset {
                        self.send_updated_position();
                    }
                }
                None => return false,
            }
        }

        true
    }
}

impl Component for HexViewer {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        let mut key_handled = true;

        match key {
            Key::Up | Key::Char('k') => {
                let old_offset = self.offset;

                self.offset = self.offset.saturating_sub(self.line_width as u64);
                self.clamp_offset();

                if self.offset != old_offset {
                    self.send_updated_position();
                }

                self.search_pos = self.offset;
            }
            Key::Down | Key::Char('j') => {
                let old_offset = self.offset;

                self.offset += self.line_width as u64;
                self.clamp_offset();

                if self.offset != old_offset {
                    self.send_updated_position();
                }

                self.search_pos = self.offset;
            }
            Key::Home | Key::Char('g') => {
                let old_offset = self.offset;

                self.offset = 0;
                self.clamp_offset();

                if self.offset != old_offset {
                    self.send_updated_position();
                }

                self.search_pos = self.offset;
            }
            Key::End | Key::Char('G') => {
                let old_offset = self.offset;

                self.offset = self.file_length;
                self.clamp_offset();

                if self.offset != old_offset {
                    self.send_updated_position();
                }

                self.search_pos = self.offset;
            }
            Key::PageUp | Key::Ctrl('b') => {
                let old_offset = self.offset;

                self.offset = self.offset.saturating_sub(
                    (self.rect.height as u64).saturating_sub(1) * (self.line_width as u64),
                );
                self.clamp_offset();

                if self.offset != old_offset {
                    self.send_updated_position();
                }

                self.search_pos = self.offset;
            }
            Key::PageDown | Key::Ctrl('f') => {
                let old_offset = self.offset;

                self.offset +=
                    (self.rect.height as u64).saturating_sub(1) * (self.line_width as u64);
                self.clamp_offset();

                if self.offset != old_offset {
                    self.send_updated_position();
                }

                self.search_pos = self.offset;
            }
            Key::Char('h') | Key::F(4) => {
                self.pubsub_tx.send(PubSub::ToggleHex).unwrap();

                match self.viewer_type {
                    ViewerType::Hex => {
                        self.pubsub_tx
                            .send(PubSub::FromHexOffset(self.offset))
                            .unwrap();
                    }
                    ViewerType::Dump => {
                        self.pubsub_tx
                            .send(PubSub::ToHexOffset(self.offset))
                            .unwrap();
                    }
                }
            }
            Key::Char(':') | Key::F(5) => {
                // TODO: Don't show the dialog if the file size is 0
                self.pubsub_tx
                    .send(PubSub::DlgGoto(GotoType::HexOffset))
                    .unwrap();
            }
            Key::Char('/') | Key::Char('?') | Key::Char('f') | Key::Char('F') | Key::F(7) => {
                // TODO: Don't show the dialog if the file size is 0
                self.pubsub_tx
                    .send(PubSub::DlgHexSearch(HexSearch {
                        search_string: String::new(),
                        hexadecimal: matches!(self.viewer_type, ViewerType::Hex),
                        backwards: matches!(key, Key::Char('?') | Key::Char('F')),
                    }))
                    .unwrap();
            }
            Key::Char('n') => {
                if let Some(_e) = &self.expression {
                    self.pubsub_tx
                        .send(PubSub::Info(
                            String::from("Search"),
                            String::from("Searching..."),
                        ))
                        .unwrap();

                    match self.backwards {
                        true => self.pubsub_tx.send(PubSub::HVSearchPrev).unwrap(),
                        false => self.pubsub_tx.send(PubSub::HVSearchNext).unwrap(),
                    }
                }
            }
            Key::Char('N') => {
                if let Some(_e) = &self.expression {
                    self.pubsub_tx
                        .send(PubSub::Info(
                            String::from("Search"),
                            String::from("Searching..."),
                        ))
                        .unwrap();

                    match self.backwards {
                        true => self.pubsub_tx.send(PubSub::HVSearchNext).unwrap(),
                        false => self.pubsub_tx.send(PubSub::HVSearchPrev).unwrap(),
                    }
                }
            }
            Key::Esc => self.expression = None,
            _ => key_handled = false,
        }

        Ok(key_handled)
    }

    fn handle_pubsub(&mut self, event: &PubSub) -> Result<()> {
        match event {
            PubSub::Goto(GotoType::HexOffset, str_offset) => {
                match u64::from_str_radix(
                    if str_offset.starts_with("0x") || str_offset.starts_with("0X") {
                        &str_offset[2..]
                    } else {
                        str_offset
                    },
                    16,
                ) {
                    Ok(offset) => {
                        let old_offset = self.offset;

                        self.offset = offset.saturating_sub(match self.line_width {
                            0 => 0,
                            w => offset % (w as u64),
                        });
                        self.clamp_offset();

                        if self.offset != old_offset {
                            self.send_updated_position();
                        }

                        self.search_pos = self.offset;
                    }
                    Err(_) => {
                        self.pubsub_tx
                            .send(PubSub::Error(format!("Invalid number: {}", str_offset)))
                            .unwrap();
                    }
                }
            }
            PubSub::FromHexOffset(offset) => {
                if let ViewerType::Dump = self.viewer_type {
                    self.offset = offset.saturating_sub(match self.line_width {
                        0 => 0,
                        w => offset % (w as u64),
                    });
                    self.clamp_offset();

                    self.send_updated_position();

                    self.search_pos = self.offset;
                }
            }
            PubSub::ToHexOffset(offset) => {
                if let ViewerType::Hex = self.viewer_type {
                    self.offset = offset.saturating_sub(match self.line_width {
                        0 => 0,
                        w => offset % (w as u64),
                    });
                    self.clamp_offset();

                    self.send_updated_position();

                    self.search_pos = self.offset;
                }
            }
            PubSub::HexSearch(search) => {
                if search.search_string.is_empty() {
                    self.expression = None;
                    return Ok(());
                }

                self.backwards = search.backwards;

                self.expression = match search.hexadecimal {
                    true => {
                        let mut use_hex_value = true;

                        search
                            .search_string
                            .split('"')
                            .try_fold(Vec::new(), |mut acc, part| {
                                match use_hex_value {
                                    true => {
                                        for hex_part in part.split_whitespace() {
                                            let mut hex_value = String::from(
                                                if hex_part.starts_with("0x")
                                                    || hex_part.starts_with("0X")
                                                {
                                                    &hex_part[2..]
                                                } else {
                                                    hex_part
                                                },
                                            );

                                            if (hex_value.len() % 2) != 0 {
                                                hex_value.insert(0, '0');
                                            }

                                            for chunk in hex_value.as_bytes().chunks(2) {
                                                match str::from_utf8(chunk) {
                                                    Ok(s) => match u8::from_str_radix(s, 16) {
                                                        Ok(value) => acc.push(value),
                                                        Err(_) => return None,
                                                    },
                                                    Err(_) => return None,
                                                }
                                            }
                                        }
                                    }
                                    false => acc.extend(part.as_bytes()),
                                }

                                use_hex_value = !use_hex_value;

                                Some(acc)
                            })
                    }
                    false => Some(search.search_string.as_bytes().to_vec()),
                };

                match &self.expression {
                    Some(e) => match e.is_empty() {
                        true => self.expression = None,
                        false => {
                            self.pubsub_tx
                                .send(PubSub::Info(
                                    String::from("Search"),
                                    String::from("Searching..."),
                                ))
                                .unwrap();

                            self.pubsub_tx.send(PubSub::HVStartSearch).unwrap();
                        }
                    },
                    None => {
                        self.pubsub_tx
                            .send(PubSub::Warning(
                                String::from("Search"),
                                format!("Hex pattern error: {}", search.search_string),
                            ))
                            .unwrap();
                    }
                }
            }
            PubSub::HVStartSearch => match &self.expression {
                Some(_e) => {
                    let found = match self.backwards {
                        true => self.search_prev(),
                        false => self.search_next(),
                    };

                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    if !found {
                        self.expression = None;

                        self.pubsub_tx
                            .send(PubSub::Warning(
                                String::from("Search"),
                                String::from("Search string not found"),
                            ))
                            .unwrap();
                    }
                }
                None => self.pubsub_tx.send(PubSub::CloseDialog).unwrap(),
            },
            PubSub::HVSearchNext => {
                self.search_pos = min(
                    match self.line_width {
                        0 => self.search_pos,
                        w => self.search_pos.saturating_sub(self.search_pos % (w as u64)),
                    } + (self.line_width as u64),
                    self.file_length,
                );

                self.search_next();
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
            }
            PubSub::HVSearchPrev => {
                self.search_pos = match self.line_width {
                    0 => self.search_pos,
                    w => self.search_pos.saturating_sub(self.search_pos % (w as u64)),
                };

                self.search_prev();
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
            }
            _ => (),
        }

        Ok(())
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let old_width = self.rect.width;
        let old_height = self.rect.height;
        let old_offset = self.offset;

        self.rect = *chunk;
        self.clamp_offset();

        if (self.rect.width != old_width)
            || (self.rect.height != old_height)
            || (self.offset != old_offset)
        {
            self.send_updated_position();
        }

        let hex_width = (self.line_width / 4) * 13;

        let widths = match self.viewer_type {
            ViewerType::Hex => vec![
                Constraint::Length((self.len_address + 1) as u16),
                Constraint::Length(hex_width as u16),
                Constraint::Length((self.line_width + 3) as u16),
            ],
            ViewerType::Dump => vec![
                Constraint::Length((self.len_address + 1) as u16),
                Constraint::Length(self.line_width as u16),
            ],
        };

        let expression_len = match &self.expression {
            Some(e) => e.len(),
            None => 0,
        };

        let buf_start = self.offset.saturating_sub(expression_len as u64);

        let stream_position = self.reader.stream_position().unwrap() as i64;
        self.reader
            .seek_relative((buf_start as i64) - stream_position)
            .unwrap();

        let buf_size = min(
            (self.line_width * (chunk.height as usize)) + (expression_len * 2),
            self.file_length.saturating_sub(buf_start) as usize,
        );
        let mut buffer: Vec<u8> = vec![0; buf_size];

        self.reader.read_exact(&mut buffer).unwrap();

        let mut bytes_remaining = 0;
        let buffer_with_matches: Vec<(bool, u8)> = buffer
            .windows(max(expression_len, 1))
            .map(|window| match &self.expression {
                Some(e) => {
                    if bytes_remaining > 0 {
                        bytes_remaining -= 1;
                        (true, window[0])
                    } else if window == e {
                        bytes_remaining = expression_len.saturating_sub(1);
                        (true, window[0])
                    } else {
                        (false, window[0])
                    }
                }
                None => (false, window[0]),
            })
            .collect::<Vec<(bool, u8)>>()
            .iter()
            .chain(
                &buffer[buffer
                    .len()
                    .saturating_sub(expression_len.saturating_sub(1))..]
                    .iter()
                    .map(|&c| {
                        if bytes_remaining > 0 {
                            bytes_remaining -= 1;
                            (true, c)
                        } else {
                            (false, c)
                        }
                    })
                    .collect::<Vec<(bool, u8)>>(),
            )
            .copied()
            .collect();

        let style_hex_even = Style::default().fg(self.config.viewer.hex_even_fg);
        let style_hex_odd = Style::default().fg(self.config.viewer.hex_odd_fg);
        let style_even = Style::default().fg(self.config.viewer.hex_text_even_fg);
        let style_odd = Style::default().fg(self.config.viewer.hex_text_odd_fg);
        let style_dump = Style::default().fg(self.config.highlight.base05);

        let items: Vec<Row> = buffer_with_matches[self.offset.saturating_sub(buf_start) as usize..]
            .chunks(self.line_width)
            .enumerate()
            .map(|(i, line)| {
                let highlight = match self.line_width {
                    0 => self.search_pos == (self.offset + (i as u64)),
                    w => {
                        self.search_pos.saturating_sub(self.search_pos % (w as u64))
                            == (self.offset + ((i * w) as u64))
                    }
                };

                let style_highlighted = Style::default()
                    .fg(match highlight {
                        true => self.config.ui.markselect_fg,
                        false => self.config.ui.selected_fg,
                    })
                    .bg(self.config.ui.selected_bg);

                match self.viewer_type {
                    ViewerType::Hex => Row::new(vec![
                        Cell::from(Span::styled(
                            format!(
                                "{:0width$X} ",
                                self.offset + ((self.line_width * i) as u64),
                                width = self.len_address
                            ),
                            Style::default().fg(self.config.viewer.lineno_fg),
                        )),
                        Cell::from(Line::from(hex_string(
                            line,
                            &style_hex_even,
                            &style_hex_odd,
                            &style_highlighted,
                        ))),
                        Cell::from(Line::from(
                            std::iter::once(Span::styled(
                                " \u{2502}",
                                Style::default().fg(self.config.viewer.lineno_fg),
                            ))
                            .chain(masked_string(
                                line,
                                &style_even,
                                &style_odd,
                                &style_highlighted,
                            ))
                            .chain(std::iter::once(Span::styled(
                                "\u{2502}",
                                Style::default().fg(self.config.viewer.lineno_fg),
                            )))
                            .collect::<Vec<Span>>(),
                        )),
                    ]),
                    ViewerType::Dump => Row::new(vec![
                        Cell::from(Span::styled(
                            format!(
                                "{:0width$X} ",
                                self.offset + ((self.line_width * i) as u64),
                                width = self.len_address
                            ),
                            Style::default().fg(self.config.viewer.lineno_fg),
                        )),
                        Cell::from(Line::from(masked_string(
                            line,
                            &style_dump,
                            &style_dump,
                            &style_highlighted,
                        ))),
                    ]),
                }
            })
            .collect();

        let table = Table::new(items, widths)
            .block(Block::default().style(Style::default().bg(self.config.highlight.base00)))
            .column_spacing(0);

        f.render_widget(table, *chunk);
    }
}

pub fn hex_string<'a>(
    line: &'a [(bool, u8)],
    style_even: &Style,
    style_odd: &Style,
    style_highlighted: &Style,
) -> Vec<Span<'a>> {
    line.iter()
        .enumerate()
        .flat_map(|(i, &(highlited, c))| {
            let mut line: Vec<Span> = Vec::new();

            if (i % 4) == 0 {
                line.push(Span::styled(
                    " ",
                    if (i % 8) < 4 { *style_even } else { *style_odd },
                ));
            }

            line.push(Span::styled(
                format!("{:02X}", c),
                if highlited {
                    *style_highlighted
                } else if (i % 8) < 4 {
                    *style_even
                } else {
                    *style_odd
                },
            ));

            line.push(Span::styled(
                " ",
                if (i % 8) < 4 { *style_even } else { *style_odd },
            ));

            line
        })
        .collect()
}

pub fn masked_string<'a>(
    line: &'a [(bool, u8)],
    style_even: &Style,
    style_odd: &Style,
    style_highlighted: &Style,
) -> Vec<Span<'a>> {
    line.iter()
        .enumerate()
        .map(|(i, &(highlited, c))| {
            Span::styled(
                String::from(if (0x20..0x7F).contains(&c) {
                    char::from_u32(c.into()).unwrap()
                } else {
                    '\u{00B7}'
                }),
                if highlited {
                    *style_highlighted
                } else if (i % 8) < 4 {
                    *style_even
                } else {
                    *style_odd
                },
            )
        })
        .collect()
}
