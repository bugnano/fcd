use std::rc::Rc;

use ratatui::{prelude::*, widgets::*};

use crate::{
    component::{Component, Focus},
    fm::command_bar::component::{CommandBar, CommandBarComponent},
    palette::Palette,
};

#[derive(Debug)]
pub struct CommandBarError {
    palette: Rc<Palette>,
    label: String,
}

impl CommandBarError {
    pub fn new(palette: &Rc<Palette>, label: &str) -> CommandBarError {
        CommandBarError {
            palette: Rc::clone(palette),
            label: format!("ERROR: {}", label),
        }
    }
}

impl Component for CommandBarError {
    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let label = Paragraph::new(Line::from(Span::raw(&self.label)))
            .block(Block::default().style(self.palette.cmdbar_error));

        f.render_widget(label, *chunk);
    }
}

impl CommandBar for CommandBarError {
    fn is_focusable(&self) -> bool {
        false
    }
}

impl CommandBarComponent for CommandBarError {}
