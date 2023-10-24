use anyhow::Result;
use ratatui::prelude::*;

use crate::{
    button_bar::ButtonBar, component::Component, events::Events, text_viewer::TextViewer,
    top_bar::TopBar,
};

#[derive(Debug)]
pub struct App {
    top_bar: TopBar,
    text_viewer: TextViewer,
    button_bar: ButtonBar,
}

impl App {
    pub fn new() -> Result<App> {
        Ok(App {
            top_bar: TopBar::new()?,
            text_viewer: TextViewer::new()?,
            button_bar: ButtonBar::new()?,
        })
    }
}

impl Component for App {
    fn init(&mut self) -> Result<()> {
        self.top_bar.init()?;
        self.text_viewer.init()?;
        self.button_bar.init()?;

        Ok(())
    }

    fn handle_events(&mut self, events: &Events) -> Result<bool> {
        let mut event_handled = false;

        if !event_handled {
            event_handled = self.top_bar.handle_events(events)?;
        }

        if !event_handled {
            event_handled = self.text_viewer.handle_events(events)?;
        }

        if !event_handled {
            event_handled = self.button_bar.handle_events(events)?;
        }

        Ok(event_handled)
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(1),
                    Constraint::Min(1),
                    Constraint::Length(1),
                ]
                .as_ref(),
            )
            .split(*chunk);

        self.top_bar.render(f, &chunks[0]);
        self.text_viewer.render(f, &chunks[1]);
        self.button_bar.render(f, &chunks[2]);
    }
}
