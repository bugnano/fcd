use std::rc::Rc;

use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::PubSub,
    component::{Component, Focus},
    fm::command_bar::component::{CommandBar, CommandBarComponent},
    palette::Palette,
    widgets::input::Input,
};

#[derive(Debug)]
pub struct Filter {
    pubsub_tx: Sender<PubSub>,
    input: Input,
    rect: Rect,
}

impl Filter {
    pub fn new(_palette: &Rc<Palette>, pubsub_tx: Sender<PubSub>, filter: &str) -> Filter {
        Filter {
            pubsub_tx,
            input: Input::new(&Style::default(), filter, filter.len()),
            rect: Rect::default(),
        }
    }
}

impl Component for Filter {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        let input_handled = self.input.handle_key(key);

        match input_handled {
            true => {
                self.pubsub_tx
                    .send(PubSub::FileFilter(self.input.value()))
                    .unwrap();
            }
            false => match key {
                Key::Char('\n') => self.pubsub_tx.send(PubSub::CloseCommandBar).unwrap(),
                Key::Esc | Key::F(10) | Key::Char('0') => {
                    self.pubsub_tx.send(PubSub::CloseCommandBar).unwrap();

                    self.pubsub_tx
                        .send(PubSub::FileFilter(String::from("")))
                        .unwrap();
                }
                Key::Ctrl('c') => key_handled = false,
                Key::Ctrl('l') => key_handled = false,
                Key::Ctrl('z') => key_handled = false,
                Key::Ctrl('o') => key_handled = false,
                _ => (),
            },
        }

        key_handled
    }

    fn handle_mouse(&mut self, button: MouseButton, mouse_position: Position) {
        if self.rect.contains(mouse_position) {
            self.input.handle_mouse(button, mouse_position);
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let label = "filter: ";
        let label_width = label.width();
        let label = Paragraph::new(Line::from(label));

        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(label_width as u16), Constraint::Min(1)])
            .split(*chunk);

        self.rect = sections[1];

        f.render_widget(label, sections[0]);

        self.input.render(f, &sections[1], focus);
    }
}

impl CommandBar for Filter {
    fn is_focusable(&self) -> bool {
        true
    }
}

impl CommandBarComponent for Filter {}
