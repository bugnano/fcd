use std::fmt;

use anyhow::Result;
use ratatui::prelude::*;
use termion::event::*;

use crate::app::PubSub;

#[derive(Debug, Copy, Clone)]
pub enum Focus {
    Normal,
    Focused,
    Active,
}

pub trait Component {
    fn handle_key(&mut self, _key: &Key) -> Result<bool> {
        Ok(false)
    }

    fn handle_mouse(&mut self, _event: &MouseEvent) -> Result<()> {
        Ok(())
    }

    fn handle_pubsub(&mut self, _event: &PubSub) -> Result<()> {
        Ok(())
    }

    fn resize(&mut self, _rect: &Rect) {}

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus);
}

impl fmt::Debug for dyn Component + '_ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dyn Component")
    }
}
