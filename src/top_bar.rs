use anyhow::Result;
use ratatui::{prelude::*, widgets::*};

use crate::component::Component;

#[derive(Debug)]
pub struct TopBar {}

impl TopBar {
    pub fn new() -> Result<TopBar> {
        Ok(TopBar {})
    }
}

impl Component for TopBar {
    fn render<B: Backend>(&mut self, f: &mut Frame<B>, chunk: &Rect) {
        let block = Block::default()
            .title(Span::styled(
                "TODO: File name",
                Style::default().fg(Color::Black),
            ))
            .style(Style::default().bg(Color::Cyan));

        f.render_widget(block, *chunk);
    }
}
