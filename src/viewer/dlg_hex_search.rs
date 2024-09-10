use std::rc::Rc;

use crossbeam_channel::Sender;
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use termion::event::*;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    palette::Palette,
    widgets::{button::Button, check_box::CheckBox, input::Input},
};

#[derive(Debug, Clone)]
pub struct HexSearch {
    pub search_string: String,
    pub hexadecimal: bool,
    pub backwards: bool,
}

#[derive(Debug)]
pub struct DlgHexSearch {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    input: Input,
    check_boxes: Vec<CheckBox>,
    btn_ok: Button,
    btn_cancel: Button,
    section_focus_position: usize,
    check_focus_position: usize,
    button_focus_position: usize,
    input_rect: Rect,
    check_box_rect: Rc<[Rect]>,
    btn_ok_rect: Rect,
    btn_cancel_rect: Rect,
}

impl DlgHexSearch {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        hex_search: &HexSearch,
    ) -> DlgHexSearch {
        DlgHexSearch {
            palette: Rc::clone(palette),
            pubsub_tx,
            input: Input::new(&palette.dialog_input, "", 0),
            check_boxes: vec![
                CheckBox::new(
                    "Hexadecimal",
                    &palette.dialog,
                    &palette.dialog_focus,
                    hex_search.hexadecimal,
                ),
                CheckBox::new(
                    "Backwards",
                    &palette.dialog,
                    &palette.dialog_focus,
                    hex_search.backwards,
                ),
            ],
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
            check_focus_position: 0,
            button_focus_position: 0,
            input_rect: Rect::default(),
            check_box_rect: Rc::new([]),
            btn_ok_rect: Rect::default(),
            btn_cancel_rect: Rect::default(),
        }
    }

    fn on_ok(&mut self) {
        self.pubsub_tx
            .send(PubSub::HexSearch(HexSearch {
                search_string: self.input.value(),
                hexadecimal: self.check_boxes[0].value(),
                backwards: self.check_boxes[1].value(),
            }))
            .unwrap();
    }
}

impl Component for DlgHexSearch {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        let input_handled = match self.section_focus_position {
            0 => self.input.handle_key(key),
            1 => self.check_boxes[self.check_focus_position].handle_key(key),
            2 => false,
            _ => unreachable!(),
        };

        if !input_handled {
            match key {
                Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                }
                Key::Char('\n') | Key::Char(' ') => {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    if (self.section_focus_position != 2) || (self.button_focus_position == 0) {
                        self.on_ok();
                    }
                }
                Key::BackTab => {
                    self.section_focus_position =
                        ((self.section_focus_position as isize) - 1).rem_euclid(3) as usize;
                }
                Key::Char('\t') => {
                    self.section_focus_position = (self.section_focus_position + 1) % 3;
                }
                Key::Up | Key::Char('k') => {
                    if self.section_focus_position > 0 {
                        self.section_focus_position -= 1;
                    }
                }
                Key::Down | Key::Char('j') => {
                    if (self.section_focus_position + 1) < 3 {
                        self.section_focus_position += 1;
                    }
                }
                Key::Left | Key::Char('h') => match self.section_focus_position {
                    0 => (),
                    1 => self.check_focus_position = 0,
                    2 => self.button_focus_position = 0,
                    _ => unreachable!(),
                },
                Key::Right | Key::Char('l') => match self.section_focus_position {
                    0 => (),
                    1 => self.check_focus_position = 1,
                    2 => self.button_focus_position = 1,
                    _ => unreachable!(),
                },
                Key::Ctrl('c') => key_handled = false,
                Key::Ctrl('l') => key_handled = false,
                Key::Ctrl('z') => key_handled = false,
                Key::Ctrl('o') => key_handled = false,
                _ => (),
            }
        }

        key_handled
    }

    fn handle_mouse(&mut self, button: MouseButton, mouse_position: layout::Position) {
        if matches!(button, MouseButton::Left | MouseButton::Right) {
            if self.input_rect.contains(mouse_position) {
                self.section_focus_position = 0;

                self.input.handle_mouse(button, mouse_position);
            }

            self.check_box_rect
                .iter()
                .enumerate()
                .for_each(|(i, rect)| {
                    if rect.contains(mouse_position) {
                        self.section_focus_position = 1;
                        self.check_focus_position = i;

                        self.check_boxes[i].handle_mouse(button, mouse_position);
                    }
                });

            if self.btn_ok_rect.contains(mouse_position) {
                self.section_focus_position = 2;
                self.button_focus_position = 0;

                if let MouseButton::Left = button {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                    self.on_ok();
                }
            }

            if self.btn_cancel_rect.contains(mouse_position) {
                self.section_focus_position = 2;
                self.button_focus_position = 1;

                if let MouseButton::Left = button {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                }
            }
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect(58, 10, chunk);

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.palette.dialog), area);
        if let Some(shadow) = self.palette.shadow {
            render_shadow(f, &area, &shadow);
        }

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(2),
                Constraint::Length(3),
            ])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let upper_block = Block::default()
            .title(
                Title::from(Span::styled(" Search ", self.palette.dialog_title))
                    .position(Position::Top)
                    .alignment(Alignment::Center),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let upper_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(upper_block.inner(sections[0]));

        self.input_rect = upper_area[1];

        let label = Paragraph::new(Span::raw("Enter search string:"));

        f.render_widget(upper_block, sections[0]);
        f.render_widget(label, upper_area[0]);
        self.input.render(
            f,
            &self.input_rect,
            match self.section_focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );

        // Middle section

        let middle_block = Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_set(MIDDLE_BORDER_SET)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let check_sections = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Min(1)])
            .split(middle_block.inner(sections[1]));

        self.check_box_rect = check_sections;

        f.render_widget(middle_block, sections[1]);

        self.check_boxes
            .iter_mut()
            .enumerate()
            .for_each(|(i, check_box)| {
                check_box.render(
                    f,
                    &self.check_box_rect[i],
                    if (self.section_focus_position == 1) && (self.check_focus_position == i) {
                        Focus::Focused
                    } else {
                        Focus::Normal
                    },
                );
            });

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
                &lower_block.inner(sections[2]),
            ));

        self.btn_ok_rect = lower_area[0];
        self.btn_cancel_rect = lower_area[2];

        f.render_widget(lower_block, sections[2]);
        self.btn_ok.render(
            f,
            &self.btn_ok_rect,
            match (self.section_focus_position, self.button_focus_position) {
                (2, 0) => Focus::Focused,
                (_, 0) => Focus::Active,
                _ => Focus::Normal,
            },
        );
        self.btn_cancel.render(
            f,
            &self.btn_cancel_rect,
            match (self.section_focus_position, self.button_focus_position) {
                (2, 1) => Focus::Focused,
                (_, 1) => Focus::Active,
                _ => Focus::Normal,
            },
        );
    }
}
