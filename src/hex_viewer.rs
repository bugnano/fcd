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
};

#[derive(Debug)]
pub struct HexViewer {
    config: Config,
    pubsub_tx: Sender<PubSub>,
    rect: Rect,
    filename_str: String,
    file_length: u64,
    reader: BufReader<File>,
    offset: u64,
    len_address: usize,
    line_width: usize,
}

impl HexViewer {
    pub fn new(
        config: &Config,
        pubsub_tx: Sender<PubSub>,
        filename: &Path,
        filename_str: &str,
        file_length: u64,
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
            reader,
            offset: 0,
            len_address: max(len_address + (len_address % 2), 8),
            line_width: 0,
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
        let width = (self.rect.width as usize).saturating_sub(self.len_address + 4);
        let num_dwords = max(width / 17, 1);

        self.line_width = num_dwords * 4;

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

                //self.search_pos = self.offset;
            }
            Key::Down | Key::Char('j') => {
                let old_offset = self.offset;

                self.offset += self.line_width as u64;
                self.clamp_offset();

                if self.offset != old_offset {
                    self.send_updated_position();
                }

                //self.search_pos = self.offset;
            }
            Key::Home | Key::Char('g') => {
                let old_offset = self.offset;

                self.offset = 0;
                self.clamp_offset();

                if self.offset != old_offset {
                    self.send_updated_position();
                }

                //self.search_pos = self.offset;
            }
            Key::End | Key::Char('G') => {
                let old_offset = self.offset;

                self.offset = self.file_length;
                self.clamp_offset();

                if self.offset != old_offset {
                    self.send_updated_position();
                }

                //self.search_pos = self.offset;
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

                //self.search_pos = self.offset;
            }
            Key::PageDown | Key::Ctrl('f') => {
                let old_offset = self.offset;

                self.offset +=
                    (self.rect.height as u64).saturating_sub(1) * (self.line_width as u64);
                self.clamp_offset();

                if self.offset != old_offset {
                    self.send_updated_position();
                }

                //self.search_pos = self.offset;
            }
            Key::Char(':') | Key::F(5) => {
                // TODO: Don't show the dialog if the file size is 0
                self.pubsub_tx
                    .send(PubSub::DlgGoto(GotoType::HexOffset))
                    .unwrap();
            }
            /*
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
            */
            _ => key_handled = false,
        }

        Ok(key_handled)
    }

    fn handle_pubsub(&mut self, event: &PubSub) -> Result<()> {
        match event {
            PubSub::Goto(GotoType::HexOffset, str_offset) => {
                match u64::from_str_radix(
                    if str_offset.to_lowercase().starts_with("0x") {
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

                        //self.search_pos = self.offset;
                    }
                    Err(_) => {
                        self.pubsub_tx
                            .send(PubSub::Error(format!("Invalid number: {}", str_offset)))
                            .unwrap();
                    }
                }
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

        let stream_position = self.reader.stream_position().unwrap() as i64;
        self.reader
            .seek_relative((self.offset as i64) - stream_position)
            .unwrap();

        let hex_width = (self.line_width / 4) * 13;

        let widths = [
            Constraint::Length((self.len_address + 1) as u16),
            Constraint::Length(hex_width as u16),
            Constraint::Length((self.line_width + 3) as u16),
        ];

        let lines: Vec<Vec<u8>> = (0..chunk.height)
            .map_while(|_n| {
                let mut buffer: Vec<u8> = vec![0; self.line_width];

                let bytes_read = self.reader.read(&mut buffer).unwrap();
                buffer.resize(bytes_read, 0);

                match buffer.is_empty() {
                    true => None,
                    false => Some(buffer),
                }
            })
            .collect();

        let items: Vec<Row> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                Row::new(vec![
                    Cell::from(Span::styled(
                        format!(
                            "{:0width$X} ",
                            self.offset + ((self.line_width * i) as u64),
                            width = self.len_address
                        ),
                        Style::default(),
                    )),
                    Cell::from(Span::styled(hex_string(line), Style::default())),
                    Cell::from(Span::styled(
                        format!(" \u{2502}{}\u{2502}", masked_string(line)),
                        Style::default(),
                    )),
                ])
            })
            .collect();

        let table = Table::new(items, widths)
            .block(Block::default().style(Style::default().bg(self.config.highlight.base00)))
            .column_spacing(0);

        f.render_widget(table, *chunk);
    }
}

fn hex_string(line: &[u8]) -> String {
    line.iter()
        .enumerate()
        .map(|(i, e)| format!("{}{:02X} ", if (i % 4) == 0 { " " } else { "" }, e))
        .collect()
}

fn masked_string(line: &[u8]) -> String {
    line.iter()
        .map(|&c| {
            if (0x20..0x7F).contains(&c) {
                char::from_u32(c.into()).unwrap()
            } else {
                '\u{00B7}'
            }
        })
        .collect()
}
