use std::rc::Rc;

use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    palette::Palette,
    widgets::{button::Button, input::Input},
};

#[derive(Debug, Copy, Clone)]
pub enum GotoType {
    LineNumber,
    HexOffset,
}

#[derive(Debug)]
pub struct DlgGoto {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    goto_type: GotoType,
    input: Input,
    btn_ok: Button,
    btn_cancel: Button,
    section_focus_position: usize,
    button_focus_position: usize,
    input_rect: Rect,
    btn_ok_rect: Rect,
    btn_cancel_rect: Rect,
}

impl DlgGoto {
    pub fn new(palette: &Rc<Palette>, pubsub_tx: Sender<PubSub>, goto_type: GotoType) -> DlgGoto {
        DlgGoto {
            palette: Rc::clone(palette),
            pubsub_tx,
            goto_type,
            input: Input::new(&palette.dialog_input, "", 0),
            btn_ok: Button::new(
                "OK",
                &palette.dialog,
                &palette.dialog_focus,
                &palette.dialog_title,
            ),
            btn_cancel: Button::new(
                "Cancel",
                &palette.dialog,
                &palette.dialog_focus,
                &palette.dialog_title,
            ),
            section_focus_position: 0,
            button_focus_position: 0,
            input_rect: Rect::default(),
            btn_ok_rect: Rect::default(),
            btn_cancel_rect: Rect::default(),
        }
    }
}

impl Component for DlgGoto {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        let input_handled = if self.section_focus_position == 0 {
            self.input.handle_key(key)
        } else {
            false
        };

        if !input_handled {
            match key {
                Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                }
                Key::Char('\n') | Key::Char(' ') => {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    if (self.section_focus_position != 1) || (self.button_focus_position == 0) {
                        self.pubsub_tx
                            .send(PubSub::Goto(self.goto_type, self.input.value()))
                            .unwrap();
                    }
                }
                Key::BackTab | Key::Char('\t') => {
                    self.section_focus_position = (self.section_focus_position + 1) % 2;
                }
                Key::Up | Key::Char('k') => {
                    self.section_focus_position = 0;
                }
                Key::Down | Key::Char('j') => {
                    self.section_focus_position = 1;
                }
                Key::Left | Key::Char('h') => {
                    if self.section_focus_position == 1 {
                        self.button_focus_position = 0;
                    }
                }
                Key::Right | Key::Char('l') => {
                    if self.section_focus_position == 1 {
                        self.button_focus_position = 1;
                    }
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

    fn handle_mouse(&mut self, button: MouseButton, mouse_position: Position) {
        if matches!(button, MouseButton::Left | MouseButton::Right) {
            if self.input_rect.contains(mouse_position) {
                self.section_focus_position = 0;

                self.input.handle_mouse(button, mouse_position);
            }

            if self.btn_ok_rect.contains(mouse_position) {
                self.section_focus_position = 1;
                self.button_focus_position = 0;

                if let MouseButton::Left = button {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    self.pubsub_tx
                        .send(PubSub::Goto(self.goto_type, self.input.value()))
                        .unwrap();
                }
            }

            if self.btn_cancel_rect.contains(mouse_position) {
                self.section_focus_position = 1;
                self.button_focus_position = 1;

                if let MouseButton::Left = button {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                }
            }
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect(30, 7, chunk);

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.palette.dialog), area);
        if let Some(shadow) = self.palette.shadow {
            render_shadow(f, &area, &shadow);
        }

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Length(3)])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let label = match self.goto_type {
            GotoType::LineNumber => "Line number: ",
            GotoType::HexOffset => "Hex offset: ",
        };

        let upper_block = Block::default()
            .title_top(Line::from(Span::styled(" Goto ", self.palette.dialog_title)).centered())
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let upper_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(label.width() as u16), Constraint::Min(1)])
            .split(upper_block.inner(sections[0]));

        self.input_rect = upper_area[1];

        f.render_widget(upper_block, sections[0]);
        f.render_widget(Paragraph::new(Span::raw(label)), upper_area[0]);
        self.input.render(
            f,
            &self.input_rect,
            match self.section_focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );

        // Lower section

        let lower_block = Block::default()
            .borders(Borders::ALL)
            .border_set(MIDDLE_BORDER_SET)
            .style(self.palette.dialog);

        let lower_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(self.btn_ok.width() as u16),
                Constraint::Length(1),
                Constraint::Length(self.btn_cancel.width() as u16),
            ])
            .split(centered_rect(
                (self.btn_ok.width() + 1 + self.btn_cancel.width()) as u16,
                1,
                &lower_block.inner(sections[1]),
            ));

        self.btn_ok_rect = lower_area[0];
        self.btn_cancel_rect = lower_area[2];

        f.render_widget(lower_block, sections[1]);
        self.btn_ok.render(
            f,
            &self.btn_ok_rect,
            match (self.section_focus_position, self.button_focus_position) {
                (1, 0) => Focus::Focused,
                (_, 0) => Focus::Active,
                _ => Focus::Normal,
            },
        );
        self.btn_cancel.render(
            f,
            &self.btn_cancel_rect,
            match (self.section_focus_position, self.button_focus_position) {
                (1, 1) => Focus::Focused,
                (_, 1) => Focus::Active,
                _ => Focus::Normal,
            },
        );
    }
}
