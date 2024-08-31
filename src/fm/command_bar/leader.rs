use std::rc::Rc;

use ratatui::{prelude::*, widgets::*};

use crate::{
    component::{Component, Focus},
    fm::command_bar::component::{CommandBar, CommandBarComponent},
    palette::Palette,
};

#[derive(Debug)]
pub struct Leader {
    label: String,
}

impl Leader {
    pub fn new(_palette: &Rc<Palette>, label: char) -> Leader {
        Leader {
            label: String::from(label),
        }
    }
}

impl Component for Leader {
    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let label = Paragraph::new(Line::from(Span::raw(&self.label)));

        f.render_widget(label, *chunk);
    }
}

impl CommandBar for Leader {
    fn is_focusable(&self) -> bool {
        false
    }
}

impl CommandBarComponent for Leader {}
