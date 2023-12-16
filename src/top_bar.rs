use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use ratatui::{prelude::*, widgets::*};

use crate::{component::Component, config::Config, tilde_layout::tilde_layout};

#[derive(Debug)]
pub struct TopBar {
    config: Config,
    filename: PathBuf,
}

impl TopBar {
    pub fn new(config: &Config, filename: &Path) -> Result<TopBar> {
        Ok(TopBar {
            config: *config,
            filename: fs::canonicalize(filename)?,
        })
    }
}

impl Component for TopBar {
    fn render(&mut self, f: &mut Frame, chunk: &Rect) {
        let block = Block::default()
            .title(Span::raw(tilde_layout(
                &self.filename.to_string_lossy(),
                chunk.width.into(),
            )))
            .style(
                Style::default()
                    .fg(self.config.ui.selected_fg)
                    .bg(self.config.ui.selected_bg),
            );

        f.render_widget(block, *chunk);
    }
}
