use std::rc::Rc;

use ratatui::{prelude::*, widgets::*};

use crate::{
    component::{Component, Focus},
    config::Config,
    fm::command_bar::component::{CommandBar, CommandBarComponent},
};

#[derive(Debug)]
pub struct CommandBarError {
    config: Rc<Config>,
    label: String,
}

impl CommandBarError {
    pub fn new(config: &Rc<Config>, label: &str) -> CommandBarError {
        CommandBarError {
            config: Rc::clone(config),
            label: format!("ERROR: {}", label),
        }
    }
}

impl Component for CommandBarError {
    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let label = Paragraph::new(Line::from(Span::raw(&self.label))).block(
            Block::default().style(
                Style::default()
                    .fg(self.config.ui.error_fg)
                    .bg(self.config.ui.error_bg),
            ),
        );

        f.render_widget(label, *chunk);
    }
}

impl CommandBar for CommandBarError {
    fn is_focusable(&self) -> bool {
        false
    }
}

impl CommandBarComponent for CommandBarError {}
