use std::fmt;

use anyhow::Result;
use ratatui::prelude::*;

use crate::app::Events;

pub trait Component {
    fn handle_events(&mut self, _events: &Events) -> Result<bool> {
        Ok(false)
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect);
}

impl fmt::Debug for dyn Component + '_ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dyn Component")
    }
}
