use std::{path::PathBuf, rc::Rc};

use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    fm::command_bar::component::{CommandBar, CommandBarComponent},
    widgets::input::Input,
};

#[derive(Debug, Clone)]
pub enum CmdBarType {
    TagGlob,
    UntagGlob,
    Mkdir,
    Rename,
    Shell(PathBuf),
    SaveReport(PathBuf),
}

#[derive(Debug)]
pub struct CmdBar {
    pubsub_tx: Sender<PubSub>,
    command_bar_type: CmdBarType,
    label: String,
    input: Input,
}

impl CmdBar {
    pub fn new(
        _config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        command_bar_type: CmdBarType,
        label: &str,
        input: &str,
        cursor_position: usize,
    ) -> CmdBar {
        CmdBar {
            pubsub_tx,
            command_bar_type,
            label: String::from(label),
            input: Input::new(&Style::default(), input, cursor_position),
        }
    }
}

impl Component for CmdBar {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        let input_handled = self.input.handle_key(key);

        if !input_handled {
            match key {
                Key::Char('\n') => {
                    self.pubsub_tx.send(PubSub::CloseCommandBar).unwrap();

                    match &self.command_bar_type {
                        CmdBarType::TagGlob => {
                            self.pubsub_tx
                                .send(PubSub::TagGlob(self.input.value()))
                                .unwrap();
                        }
                        CmdBarType::UntagGlob => {
                            self.pubsub_tx
                                .send(PubSub::UntagGlob(self.input.value()))
                                .unwrap();
                        }
                        CmdBarType::Mkdir => {
                            self.pubsub_tx
                                .send(PubSub::Mkdir(self.input.value()))
                                .unwrap();
                        }
                        CmdBarType::Rename => {
                            self.pubsub_tx
                                .send(PubSub::Rename(self.input.value()))
                                .unwrap();
                        }
                        CmdBarType::Shell(cwd) => {
                            self.pubsub_tx
                                .send(PubSub::Shell(cwd.clone(), self.input.value()))
                                .unwrap();
                        }
                        CmdBarType::SaveReport(cwd) => {
                            self.pubsub_tx
                                .send(PubSub::SaveReport(cwd.clone(), self.input.value()))
                                .unwrap();
                        }
                    }
                }
                Key::Esc | Key::F(10) | Key::Char('0') => {
                    self.pubsub_tx.send(PubSub::CloseCommandBar).unwrap();
                }
                Key::Ctrl('c') => key_handled = false,
                Key::Ctrl('l') => key_handled = false,
                Key::Ctrl('z') => key_handled = false,
                Key::Ctrl('o') => key_handled = false,
                _ => (),
            }
        }

        key_handled
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(self.label.width() as u16),
                Constraint::Min(1),
            ])
            .split(*chunk);

        let label = Paragraph::new(Span::raw(&self.label));

        f.render_widget(label, sections[0]);

        self.input.render(f, &sections[1], focus);
    }
}

impl CommandBar for CmdBar {
    fn is_focusable(&self) -> bool {
        true
    }
}

impl CommandBarComponent for CmdBar {}
