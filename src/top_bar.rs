use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use ratatui::{prelude::*, widgets::*};

use crate::component::Component;

#[derive(Debug)]
pub struct TopBar {
    filename: PathBuf,
}

impl TopBar {
    pub fn new(filename: &Path) -> Result<TopBar> {
        Ok(TopBar {
            filename: fs::canonicalize(filename)?,
        })
    }
}

impl Component for TopBar {
    fn render(&mut self, f: &mut Frame, chunk: &Rect) {
        let block = Block::default()
            .title(Span::styled(
                self.filename.to_string_lossy(),
                Style::default().fg(Color::Black),
            ))
            .style(Style::default().bg(Color::Cyan));

        f.render_widget(block, *chunk);
    }
}
