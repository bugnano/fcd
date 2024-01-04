use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

use anyhow::Result;
use crossbeam_channel::Sender;
use ratatui::prelude::*;
use termion::event::*;

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    viewer::{
        hex_viewer::{HexViewer, ViewerType},
        text_viewer::TextViewer,
    },
};

#[derive(Debug)]
pub struct FileViewer {
    main_viewer: Box<dyn Component>,
    hex_viewer: Option<HexViewer>,
    hex_mode: bool,
}

impl FileViewer {
    pub fn new(
        config: &Config,
        pubsub_tx: Sender<PubSub>,
        filename: &Path,
        tabsize: u8,
    ) -> Result<FileViewer> {
        let filename_str = fs::canonicalize(filename)?.to_string_lossy().to_string();
        let attr = fs::metadata(filename)?;

        let main_viewer = match attr.is_dir() {
            true => {
                // TODO: Show directory contents
                todo!();
            }
            false => {
                let mut f = File::open(filename)?;
                let mut buffer: Vec<u8> = vec![0; 131072];

                let is_text_file = if attr.len() > (4 * 1024 * 1024) {
                    false
                } else {
                    let bytes_read = f.read(&mut buffer)?;
                    buffer.resize(bytes_read, 0);

                    !buffer.contains(&0)
                };

                match is_text_file {
                    true => {
                        f.read_to_end(&mut buffer)?;

                        Box::new(TextViewer::new(
                            config,
                            pubsub_tx.clone(),
                            filename,
                            &filename_str,
                            tabsize,
                            buffer,
                        )?) as Box<dyn Component>
                    }
                    false => Box::new(HexViewer::new(
                        config,
                        pubsub_tx.clone(),
                        filename,
                        &filename_str,
                        attr.len(),
                        ViewerType::Dump,
                    )?) as Box<dyn Component>,
                }
            }
        };

        let hex_viewer = match attr.is_dir() {
            true => None,
            false => Some(HexViewer::new(
                config,
                pubsub_tx.clone(),
                filename,
                &filename_str,
                attr.len(),
                ViewerType::Hex,
            )?),
        };

        Ok(FileViewer {
            main_viewer,
            hex_viewer,
            hex_mode: false,
        })
    }
}

impl Component for FileViewer {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        match (self.hex_mode, &mut self.hex_viewer) {
            (true, Some(hex_viewer)) => hex_viewer.handle_key(key),
            _ => self.main_viewer.handle_key(key),
        }
    }

    fn handle_pubsub(&mut self, event: &PubSub) -> Result<()> {
        match (self.hex_mode, &mut self.hex_viewer) {
            (true, Some(hex_viewer)) => hex_viewer.handle_pubsub(event)?,
            _ => self.main_viewer.handle_pubsub(event)?,
        }

        match event {
            PubSub::ToggleHex => self.hex_mode = !self.hex_mode,
            _ => (),
        }

        Ok(())
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        match (self.hex_mode, &mut self.hex_viewer) {
            (true, Some(hex_viewer)) => hex_viewer.render(f, chunk, focus),
            _ => self.main_viewer.render(f, chunk, focus),
        }
    }
}
