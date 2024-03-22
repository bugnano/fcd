use std::rc::Rc;

use anyhow::Result;
use crossbeam_channel::Sender;
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use termion::event::*;

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    fm::{
        entry::Entry,
        panel::{Panel, PanelComponent},
    },
    tilde_layout::tilde_layout,
    viewer::file_viewer::FileViewer,
};

#[derive(Debug)]
pub struct QuickView {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    enabled: bool,
    filename: String,
    viewer: Option<FileViewer>,
    tabsize: u8,
}

impl QuickView {
    pub fn new(config: &Rc<Config>, pubsub_tx: Sender<PubSub>, tabsize: u8) -> Result<QuickView> {
        Ok(QuickView {
            config: Rc::clone(config),
            pubsub_tx,
            enabled: false,
            filename: String::from(""),
            viewer: None,
            tabsize,
        })
    }

    fn update_quickview(&mut self, entry: Option<&Entry>) {
        match (&self.enabled, entry) {
            (true, Some(entry)) => {
                let file_name = entry
                    .file
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                if file_name != self.filename {
                    self.filename = file_name;

                    // We use the quick viewer only for regular files and directories
                    if entry.stat.is_file() || entry.stat.is_dir() {
                        self.viewer = FileViewer::new(
                            &self.config,
                            self.pubsub_tx.clone(),
                            &entry.file,
                            self.tabsize,
                        )
                        .ok();
                    } else {
                        self.viewer = None;
                    }
                }
            }
            _ => {
                self.filename = String::from("");
                self.viewer = None;
            }
        };
    }
}

impl Component for QuickView {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        match &mut self.viewer {
            Some(viewer) => viewer.handle_key(key),
            None => Ok(false),
        }
    }

    fn handle_pubsub(&mut self, event: &PubSub) -> Result<()> {
        if let Some(viewer) = &mut self.viewer {
            viewer.handle_pubsub(event)?
        }

        match event {
            PubSub::ToggleQuickView(entry) => {
                self.enabled = !self.enabled;

                self.update_quickview(entry.as_ref());
            }
            PubSub::UpdateQuickView(entry) => {
                if self.enabled {
                    self.update_quickview(entry.as_ref());
                }
            }
            _ => (),
        }

        Ok(())
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let block = Block::default()
            .title(
                Title::from(Line::from(vec![
                    Span::raw(symbols::line::NORMAL.horizontal),
                    Span::styled(
                        tilde_layout(
                            &match self.filename.is_empty() {
                                true => String::from(" (Preview) "),
                                false => format!(" (Preview) {} ", self.filename),
                            },
                            chunk.width.saturating_sub(4).into(),
                        ),
                        match focus {
                            Focus::Focused => Style::default()
                                .fg(self.config.panel.reverse_fg)
                                .bg(self.config.panel.reverse_bg),
                            _ => Style::default()
                                .fg(self.config.panel.fg)
                                .bg(self.config.panel.bg),
                        },
                    ),
                    Span::raw(symbols::line::NORMAL.horizontal),
                ]))
                .position(Position::Top)
                .alignment(Alignment::Left),
            )
            .borders(Borders::ALL)
            .style(
                Style::default()
                    .fg(self.config.panel.fg)
                    .bg(self.config.panel.bg),
            );

        let inner = block.inner(*chunk);

        f.render_widget(block, *chunk);

        if let Some(viewer) = &mut self.viewer {
            viewer.render(f, &inner, focus);
        }
    }
}

impl Panel for QuickView {}
impl PanelComponent for QuickView {}
