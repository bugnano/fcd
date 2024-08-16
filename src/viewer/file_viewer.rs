use std::{
    fs::{self, File},
    io::Read,
    path::Path,
    rc::Rc,
};

use anyhow::Result;
use crossbeam_channel::Sender;
use ratatui::prelude::*;
use termion::event::*;

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    fm::entry::{get_file_list, sort_by_function, SortBy, SortOrder},
    viewer::{
        dir_viewer::DirViewer,
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
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        filename: &Path,
        tabsize: u8,
    ) -> Result<FileViewer> {
        let filename_str = fs::canonicalize(filename)?.to_string_lossy().to_string();
        let attr = fs::metadata(filename)?;

        let main_viewer = match attr.is_dir() {
            true => {
                let mut file_list = get_file_list(filename, None)?;

                // TODO: It would be nice to use the same hidden file filter, sort method and sort
                // order of the other panel when using the file viewer as a quick preview
                file_list.sort_unstable_by(|a, b| {
                    sort_by_function(SortBy::Name)(a, b, SortOrder::Normal)
                });

                Box::new(DirViewer::new(
                    config,
                    pubsub_tx.clone(),
                    filename,
                    &filename_str,
                    file_list,
                )) as Box<dyn Component>
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
                        )) as Box<dyn Component>
                    }
                    false => Box::new(HexViewer::new(
                        config,
                        pubsub_tx.clone(),
                        filename,
                        &filename_str,
                        attr.len(),
                        ViewerType::Dump,
                    )) as Box<dyn Component>,
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
            )),
        };

        Ok(FileViewer {
            main_viewer,
            hex_viewer,
            hex_mode: false,
        })
    }
}

impl Component for FileViewer {
    fn handle_key(&mut self, key: &Key) -> bool {
        match (self.hex_mode, &mut self.hex_viewer) {
            (true, Some(hex_viewer)) => hex_viewer.handle_key(key),
            _ => self.main_viewer.handle_key(key),
        }
    }

    fn handle_pubsub(&mut self, event: &PubSub) {
        match (self.hex_mode, &mut self.hex_viewer) {
            (true, Some(hex_viewer)) => hex_viewer.handle_pubsub(event),
            _ => self.main_viewer.handle_pubsub(event),
        }

        #[allow(clippy::single_match)]
        match event {
            PubSub::ToggleHex => self.hex_mode = !self.hex_mode,
            _ => (),
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        match (self.hex_mode, &mut self.hex_viewer) {
            (true, Some(hex_viewer)) => hex_viewer.render(f, chunk, focus),
            _ => self.main_viewer.render(f, chunk, focus),
        }
    }
}
