use std::{
    cmp::{max, min},
    fs::{self, File},
    io::{BufReader, Seek},
    path::{Path, PathBuf},
    str, thread,
};

use anyhow::Result;
use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

        let mut viewer = HexViewer {
            config: *config,
            pubsub_tx,
            rect: Rect::default(),
            filename_str: String::from(filename_str),
            file_length,
            reader,
            offset: 0,
        };

        viewer.send_updated_position();

        Ok(viewer)
    }

    pub fn clamp_offset(&mut self) {
        // TODO
    }

    pub fn send_updated_position(&mut self) {
        // TODO: This is wrong
        let offset = self.offset + (self.rect.height as u64);

        self.pubsub_tx
            .send(PubSub::FileInfo(
                String::from(&self.filename_str),
                format!("{}/{}", min(offset, self.file_length), self.file_length),
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
    }
}
