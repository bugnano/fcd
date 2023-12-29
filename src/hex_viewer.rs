use std::{
    cmp::{max, min},
    fs::{self, File},
    io::{BufReader, Read, Seek},
    path::{Path, PathBuf},
    str, thread,
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
    dlg_text_search::{SearchType, TextSearch},
    fnmatch,
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
        // TODO
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
                    Cell::from(Span::styled(hex_string(&line), Style::default())),
                    Cell::from(Span::styled(
                        format!(" \u{2502}{}\u{2502}", masked_string(&line)),
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
            if (c < 0x20) || (c >= 0x7F) {
                '\u{00B7}'
            } else {
                char::from_u32(c.into()).unwrap()
            }
        })
        .collect()
}
