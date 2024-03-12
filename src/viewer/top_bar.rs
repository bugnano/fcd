use std::rc::Rc;

use anyhow::Result;
use ratatui::{
    prelude::*,
    widgets::{block::Title, *},
};

use unicode_width::UnicodeWidthStr;

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    tilde_layout::tilde_layout,
};

#[derive(Debug)]
pub struct TopBar {
    config: Rc<Config>,
    filename: String,
    position: String,
    percent: String,
}

impl TopBar {
    pub fn new(config: &Rc<Config>) -> Result<TopBar> {
        Ok(TopBar {
            config: Rc::clone(config),
            filename: String::new(),
            position: String::new(),
            percent: String::new(),
        })
    }
}

impl Component for TopBar {
    fn handle_pubsub(&mut self, event: &PubSub) -> Result<()> {
        match event {
            PubSub::FileInfo(filename, position, percent) => {
                self.filename = String::from(filename);
                self.position = String::from(position);
                self.percent = String::from(percent);
            }
            _ => (),
        }

        Ok(())
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let position_width = (self.position.width() + 4) as u16;
        let filename_width = chunk
            .width
            .saturating_sub(position_width)
            .saturating_sub(self.percent.width() as u16);

        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(filename_width),
                Constraint::Length(position_width),
                Constraint::Length(self.percent.width() as u16),
            ])
            .split(*chunk);

        let filename_block = Block::default()
            .title(Span::raw(tilde_layout(
                &self.filename,
                filename_width.into(),
            )))
            .style(
                Style::default()
                    .fg(self.config.ui.selected_fg)
                    .bg(self.config.ui.selected_bg),
            );

        let position_block = Block::default()
            .title(Title::from(Span::raw(&self.position)).alignment(Alignment::Center))
            .style(
                Style::default()
                    .fg(self.config.ui.selected_fg)
                    .bg(self.config.ui.selected_bg),
            );

        let percent_block = Block::default().title(Span::raw(&self.percent)).style(
            Style::default()
                .fg(self.config.ui.selected_fg)
                .bg(self.config.ui.selected_bg),
        );

        f.render_widget(filename_block, sections[0]);
        f.render_widget(position_block, sections[1]);
        f.render_widget(percent_block, sections[2]);
    }
}
