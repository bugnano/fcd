use std::{
    cmp::{max, min},
    path::{Path, PathBuf},
    rc::Rc,
};

use crossbeam_channel::Sender;
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use termion::event::*;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::{centered_rect, render_shadow, PubSub},
    component::{Component, Focus},
    config::Config,
    fm::entry::Entry,
    tilde_layout::tilde_layout,
    widgets::{button::Button, input::Input, radio_box::RadioBox},
};

#[derive(Debug, Clone, Copy)]
pub enum DlgCpMvType {
    Cp,
    Mv,
}

#[derive(Debug, Clone, Copy)]
pub enum OnConflict {
    Overwrite,
    Skip,
    RenameExisting,
    RenameCopy,
}

#[derive(Debug)]
pub struct DlgCpMv {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    cwd: PathBuf,
    entries: Vec<Entry>,
    dlg_cp_mv_type: DlgCpMvType,
    input: Input,
    radio: RadioBox,
    btn_ok: Button,
    btn_cancel: Button,
    section_focus_position: usize,
    button_focus_position: usize,
}

impl DlgCpMv {
    pub fn new(
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        cwd: &Path,
        entries: &[Entry],
        dest: &str,
        dlg_cp_mv_type: DlgCpMvType,
    ) -> DlgCpMv {
        DlgCpMv {
            config: Rc::clone(config),
            pubsub_tx,
            cwd: PathBuf::from(cwd),
            entries: Vec::from(entries),
            dlg_cp_mv_type,
            input: Input::new(
                &Style::default()
                    .fg(config.dialog.input_fg)
                    .bg(config.dialog.input_bg),
                dest,
                dest.len(),
            ),
            radio: RadioBox::new(
                ["Overwrite", "Skip", "Rename Existing", "Rename Copy"],
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                2,
            ),
            btn_ok: Button::new(
                "OK",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
            btn_cancel: Button::new(
                "Cancel",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
            section_focus_position: 0,
            button_focus_position: 0,
        }
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

                    if (self.section_focus_position == 0) || (self.button_focus_position == 0) {
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
                                self.dlg_cp_mv_type,
                            ))
                            .unwrap();
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

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect((((chunk.width as usize) * 17) / 20) as u16, 14, chunk);

        f.render_widget(Clear, area);
        f.render_widget(
            Block::default().style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            ),
            area,
        );
        if self.config.ui.use_shadows {
            render_shadow(
                f,
                &area,
                &Style::default()
                    .bg(self.config.ui.shadow_bg)
                    .fg(self.config.ui.shadow_fg),
            );
        }

        let middle_border_set = symbols::border::Set {
            top_left: symbols::line::NORMAL.vertical_right,
            top_right: symbols::line::NORMAL.vertical_left,
            ..symbols::border::PLAIN
        };

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

        let title = match &self.dlg_cp_mv_type {
            DlgCpMvType::Cp => "Copy",
            DlgCpMvType::Mv => "Move",
        };

        let upper_block = Block::default()
            .title(
                Title::from(Span::styled(
                    tilde_layout(&format!(" {} ", title), sections[0].width as usize),
                    Style::default().fg(self.config.dialog.title_fg),
                ))
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

        let upper_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(upper_block.inner(sections[0]));

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
            &upper_area[1],
            match self.section_focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );

        // Middle section

        let label = "On conflict:";

        let middle_block = Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_set(middle_border_set)
            .padding(Padding::horizontal(1))
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

        let middle_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(4)])
            .split(centered_rect(
                max(label.width(), self.radio.width()) as u16,
                5,
                &middle_block.inner(sections[1]),
            ));

        let label = Paragraph::new(Span::raw(tilde_layout(
            label,
            middle_area[0].width as usize,
        )));

        f.render_widget(middle_block, sections[1]);
        f.render_widget(label, middle_area[0]);
        self.radio.render(
            f,
            &middle_area[1],
            match self.section_focus_position {
                1 => Focus::Focused,
                _ => Focus::Normal,
            },
        );

        // Lower section

        let lower_block = Block::default()
            .borders(Borders::ALL)
            .border_set(middle_border_set)
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

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

        f.render_widget(lower_block, sections[2]);
        self.btn_ok.render(
            f,
            &lower_area[0],
            match (self.section_focus_position, self.button_focus_position) {
                (2, 0) => Focus::Focused,
                (_, 0) => Focus::Active,
                _ => Focus::Normal,
            },
        );
        self.btn_cancel.render(
            f,
            &lower_area[2],
            match (self.section_focus_position, self.button_focus_position) {
                (2, 1) => Focus::Focused,
                (_, 1) => Focus::Active,
                _ => Focus::Normal,
            },
        );
    }
}
