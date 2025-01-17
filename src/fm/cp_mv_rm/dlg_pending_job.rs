use std::{
    cmp::min,
    path::{Path, PathBuf},
    rc::Rc,
};

use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use pathdiff::diff_paths;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    fm::cp_mv_rm::database::{DBJobEntry, DataBase},
    palette::Palette,
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgPendingJob {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    job: DBJobEntry,
    db_file: Option<PathBuf>,
    messages: Vec<String>,
    btn_continue: Button,
    btn_skip: Button,
    btn_abort: Button,
    first_line: usize,
    focus_position: usize,
    rect: Rect,
    btn_continue_rect: Rect,
    btn_skip_rect: Rect,
    btn_abort_rect: Rect,
}

impl DlgPendingJob {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        job: &DBJobEntry,
        db_file: Option<&Path>,
    ) -> DlgPendingJob {
        let mut messages: Vec<String> = vec![
            format!("Status: {}", job.status),
            format!("Operation: {}", job.operation),
            format!("From: {}", job.cwd.to_string_lossy()),
        ];

        if let Some(dest) = &job.dest {
            messages.push(format!("To: {}", dest.to_string_lossy()));
        }

        messages.push(String::from("Files:"));

        messages.extend(job.entries.iter().map(|entry| {
            format!(
                "{}",
                diff_paths(&entry.file, &job.cwd).unwrap().to_string_lossy()
            )
        }));

        DlgPendingJob {
            palette: Rc::clone(palette),
            pubsub_tx,
            job: job.clone(),
            db_file: db_file.map(PathBuf::from),
            messages,
            btn_continue: Button::new(
                "Continue",
                &palette.dialog,
                &palette.dialog_focus,
                &palette.dialog_title,
            ),
            btn_skip: Button::new(
                "Skip",
                &palette.dialog,
                &palette.dialog_focus,
                &palette.dialog_title,
            ),
            btn_abort: Button::new(
                "Abort",
                &palette.dialog,
                &palette.dialog_focus,
                &palette.dialog_title,
            ),
            first_line: 0,
            focus_position: 0,
            rect: Rect::default(),
            btn_continue_rect: Rect::default(),
            btn_skip_rect: Rect::default(),
            btn_abort_rect: Rect::default(),
        }
    }

    fn clamp_first_line(&mut self) {
        if (self.first_line + (self.rect.height as usize)) > self.messages.len() {
            self.first_line = self
                .messages
                .len()
                .saturating_sub(self.rect.height as usize);
        }
    }

    fn on_abort(&mut self) {
        self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

        self.db_file
            .as_deref()
            .and_then(|db_file| DataBase::new(db_file).ok())
            .map(|db| db.delete_job(self.job.id));

        self.pubsub_tx.send(PubSub::NextPendingJob).unwrap();
    }
}

impl Component for DlgPendingJob {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                self.pubsub_tx.send(PubSub::NextPendingJob).unwrap();
            }
            Key::Char('\n') | Key::Char(' ') => match self.focus_position {
                0 => {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    self.pubsub_tx
                        .send(PubSub::MountArchivesForJob(self.job.clone()))
                        .unwrap();
                }
                1 => {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                    self.pubsub_tx.send(PubSub::NextPendingJob).unwrap();
                }
                2 => self.on_abort(),
                _ => unreachable!(),
            },
            Key::Left | Key::Char('h') => {
                self.focus_position = self.focus_position.saturating_sub(1)
            }
            Key::Right | Key::Char('l') => self.focus_position = min(self.focus_position + 1, 2),
            Key::Up | Key::Char('k') => {
                self.first_line = self.first_line.saturating_sub(1);
            }
            Key::Down | Key::Char('j') => {
                self.first_line = self.first_line.saturating_add(1);
                self.clamp_first_line();
            }
            Key::Home | Key::CtrlHome | Key::Char('g') => {
                self.first_line = 0;
            }
            Key::End | Key::CtrlEnd | Key::Char('G') => {
                self.first_line = self.messages.len();
                self.clamp_first_line();
            }
            Key::PageUp | Key::Ctrl('b') => {
                let rect_height = (self.rect.height as usize).saturating_sub(1);

                self.first_line = self.first_line.saturating_sub(rect_height);
            }
            Key::PageDown | Key::Ctrl('f') => {
                let rect_height = (self.rect.height as usize).saturating_sub(1);

                self.first_line = self.first_line.saturating_add(rect_height);
                self.clamp_first_line();
            }
            Key::Ctrl('c') => key_handled = false,
            Key::Ctrl('l') => key_handled = false,
            Key::Ctrl('z') => key_handled = false,
            Key::Ctrl('o') => key_handled = false,
            _ => (),
        }

        key_handled
    }

    fn handle_mouse(&mut self, button: MouseButton, mouse_position: Position) {
        match button {
            MouseButton::Left | MouseButton::Right => {
                if self.btn_continue_rect.contains(mouse_position) {
                    self.focus_position = 0;

                    if let MouseButton::Left = button {
                        self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                        self.pubsub_tx
                            .send(PubSub::MountArchivesForJob(self.job.clone()))
                            .unwrap();
                    }
                }

                if self.btn_skip_rect.contains(mouse_position) {
                    self.focus_position = 1;

                    if let MouseButton::Left = button {
                        self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                        self.pubsub_tx.send(PubSub::NextPendingJob).unwrap();
                    }
                }

                if self.btn_abort_rect.contains(mouse_position) {
                    self.focus_position = 2;

                    if let MouseButton::Left = button {
                        self.on_abort();
                    }
                }
            }
            MouseButton::WheelUp => {
                self.first_line = self.first_line.saturating_sub(1);
            }
            MouseButton::WheelDown => {
                self.first_line = self.first_line.saturating_add(1);
                self.clamp_first_line();
            }
            _ => {}
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let area = centered_rect(
            (((chunk.width as usize) * 3) / 4) as u16,
            (((chunk.height as usize) * 3) / 4) as u16,
            chunk,
        );

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.palette.dialog), area);
        if let Some(shadow) = self.palette.shadow {
            render_shadow(f, &area, &shadow);
        }

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let upper_block = Block::default()
            .title_top(
                Line::from(Span::styled(
                    tilde_layout(" Interrupted Job ", sections[0].width as usize),
                    self.palette.dialog_title,
                ))
                .centered(),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let upper_area = upper_block.inner(sections[0]);

        self.rect = upper_area;
        self.clamp_first_line();

        let items: Vec<ListItem> = self
            .messages
            .iter()
            .skip(self.first_line)
            .take(upper_area.height.into())
            .map(|message| ListItem::new::<&str>(message))
            .collect();

        let list = List::new(items);

        f.render_widget(upper_block, sections[0]);
        f.render_widget(list, upper_area);

        // Lower section

        let lower_block = Block::default()
            .borders(Borders::ALL)
            .border_set(MIDDLE_BORDER_SET)
            .style(self.palette.dialog);

        let lower_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(self.btn_continue.width() as u16),
                Constraint::Length(1),
                Constraint::Length(self.btn_skip.width() as u16),
                Constraint::Length(1),
                Constraint::Length(self.btn_abort.width() as u16),
            ])
            .split(centered_rect(
                (self.btn_continue.width() + 1 + self.btn_skip.width() + 1 + self.btn_abort.width())
                    as u16,
                1,
                &lower_block.inner(sections[1]),
            ));

        self.btn_continue_rect = lower_area[0];
        self.btn_skip_rect = lower_area[2];
        self.btn_abort_rect = lower_area[4];

        f.render_widget(lower_block, sections[1]);
        self.btn_continue.render(
            f,
            &self.btn_continue_rect,
            match self.focus_position {
                0 => match focus {
                    Focus::Focused => Focus::Focused,
                    _ => Focus::Active,
                },
                _ => Focus::Normal,
            },
        );
        self.btn_skip.render(
            f,
            &self.btn_skip_rect,
            match self.focus_position {
                1 => match focus {
                    Focus::Focused => Focus::Focused,
                    _ => Focus::Active,
                },
                _ => Focus::Normal,
            },
        );
        self.btn_abort.render(
            f,
            &self.btn_abort_rect,
            match self.focus_position {
                2 => match focus {
                    Focus::Focused => Focus::Focused,
                    _ => Focus::Active,
                },
                _ => Focus::Normal,
            },
        );
    }
}
