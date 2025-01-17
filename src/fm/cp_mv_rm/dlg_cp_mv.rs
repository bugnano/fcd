use std::{
    cmp::{max, min},
    path::{Path, PathBuf},
    rc::Rc,
};

use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    fm::{
        cp_mv_rm::database::{DBJobOperation, OnConflict},
        entry::Entry,
    },
    palette::Palette,
    tilde_layout::tilde_layout,
    widgets::{button::Button, input::Input, radio_box::RadioBox},
};

#[derive(Debug)]
pub struct DlgCpMv {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    cwd: PathBuf,
    entries: Vec<Entry>,
    operation: DBJobOperation,
    input: Input,
    radio: RadioBox,
    btn_ok: Button,
    btn_cancel: Button,
    section_focus_position: usize,
    button_focus_position: usize,
    input_rect: Rect,
    radio_rect: Rect,
    btn_ok_rect: Rect,
    btn_cancel_rect: Rect,
}

impl DlgCpMv {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        cwd: &Path,
        entries: &[Entry],
        dest: &str,
        operation: DBJobOperation,
    ) -> DlgCpMv {
        DlgCpMv {
            palette: Rc::clone(palette),
            pubsub_tx,
            cwd: PathBuf::from(cwd),
            entries: Vec::from(entries),
            operation,
            input: Input::new(&palette.dialog_input, dest, dest.len()),
            radio: RadioBox::new(
                ["Overwrite", "Skip", "Rename Existing", "Rename Copy"],
                &palette.dialog,
                &palette.dialog_focus,
                2,
            ),
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
            radio_rect: Rect::default(),
            btn_ok_rect: Rect::default(),
            btn_cancel_rect: Rect::default(),
        }
    }

    fn on_ok(&mut self) {
        let on_conflict = match self.radio.value() {
            0 => OnConflict::Overwrite,
            1 => OnConflict::Skip,
            2 => OnConflict::RenameExisting,
            3 => OnConflict::RenameCopy,
            _ => unreachable!(),
        };

        self.pubsub_tx
            .send(PubSub::DoDirscan(
                self.cwd.clone(),
                self.entries.clone(),
                self.input.value(),
                on_conflict,
                self.operation,
            ))
            .unwrap();
    }
}

impl Component for DlgCpMv {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        let input_handled = match self.section_focus_position {
            0 => self.input.handle_key(key),
            1 => self.radio.handle_key(key),
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
                    self.section_focus_position = self.section_focus_position.saturating_sub(1);
                }
                Key::Down | Key::Char('j') => {
                    self.section_focus_position = min(self.section_focus_position + 1, 2)
                }
                Key::Left | Key::Char('h') => {
                    if self.section_focus_position == 2 {
                        self.button_focus_position = 0;
                    }
                }
                Key::Right | Key::Char('l') => {
                    if self.section_focus_position == 2 {
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

            if self.radio_rect.contains(mouse_position) {
                self.section_focus_position = 1;

                self.radio.handle_mouse(button, mouse_position);
            }

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
        let area = centered_rect((((chunk.width as usize) * 17) / 20) as u16, 14, chunk);

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.palette.dialog), area);
        if let Some(shadow) = self.palette.shadow {
            render_shadow(f, &area, &shadow);
        }

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(6),
                Constraint::Length(3),
            ])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let title = self.operation.to_string();

        let upper_block = Block::default()
            .title_top(
                Line::from(Span::styled(
                    tilde_layout(&format!(" {} ", title), sections[0].width as usize),
                    self.palette.dialog_title,
                ))
                .centered(),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let upper_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(upper_block.inner(sections[0]));

        self.input_rect = upper_area[1];

        let question = Paragraph::new(Span::raw(tilde_layout(
            &match self.entries.len() {
                1 => format!("{} {} to:", title, self.entries[0].file_name),
                n => format!("{} {} files/directories to:", title, n),
            },
            upper_area[0].width as usize,
        )));

        f.render_widget(upper_block, sections[0]);
        f.render_widget(question, upper_area[0]);
        self.input.render(
            f,
            &self.input_rect,
            match self.section_focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );

        // Middle section

        let label = "On conflict:";

        let middle_block = Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_set(MIDDLE_BORDER_SET)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let middle_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(4)])
            .split(centered_rect(
                max(label.width(), self.radio.width()) as u16,
                5,
                &middle_block.inner(sections[1]),
            ));

        self.radio_rect = middle_area[1];

        let label = Paragraph::new(Span::raw(tilde_layout(
            label,
            middle_area[0].width as usize,
        )));

        f.render_widget(middle_block, sections[1]);
        f.render_widget(label, middle_area[0]);
        self.radio.render(
            f,
            &self.radio_rect,
            match self.section_focus_position {
                1 => Focus::Focused,
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
