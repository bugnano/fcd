use anyhow::Result;
use ratatui::prelude::*;

use crate::events::Events;

pub trait Component {
    fn init(&mut self) -> Result<()> {
        Ok(())
    }

    fn handle_events(&mut self, _events: &Events) -> Result<bool> {
        Ok(false)
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect);
}
