use std::rc::Rc;

use anyhow::Result;
use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    widgets::input::Input,
};

#[derive(Debug)]
pub struct Filter {
    pubsub_tx: Sender<PubSub>,
    input: Input,
}

impl Filter {
    pub fn new(_config: &Rc<Config>, pubsub_tx: Sender<PubSub>, filter: &str) -> Result<Filter> {
        Ok(Filter {
            pubsub_tx,
            input: Input::new(&Style::default(), filter, filter.len())?,
        })
    }
}

impl Component for Filter {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        let input_handled = self.input.handle_key(key)?;

        let key_handled = match input_handled {
            true => {
                self.pubsub_tx
                    .send(PubSub::FileFilter(self.input.value()))
                    .unwrap();

                true
            }
            false => match key {
                Key::Char('\n') => {
                    self.pubsub_tx.send(PubSub::CloseCommandBar).unwrap();

                    true
                }
                Key::BackTab | Key::Char('\t') => true,
                _ => false,
            },
        };

        Ok(key_handled)
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let label = "filter: ";
        let label_width = label.width();
        let label = Paragraph::new(Line::from(label));

        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(label_width as u16), Constraint::Min(1)])
            .split(*chunk);

        f.render_widget(label, sections[0]);

        self.input.render(f, &sections[1], focus);
    }
}
