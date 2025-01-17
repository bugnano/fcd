use std::{
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    rc::Rc,
};

use crossbeam_channel::Sender;
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use atomicwrites::{AllowOverwrite, AtomicFile};
use pathdiff::diff_paths;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    dlg_error::DialogType,
    fm::cp_mv_rm::database::{
        DBDirListEntry, DBFileEntry, DBFileStatus, DBJobEntry, DBJobStatus, DataBase,
    },
    palette::Palette,
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgReport {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    job: DBJobEntry,
    db_file: Option<PathBuf>,
    messages: Vec<String>,
    dialog_type: DialogType,
    btn_close: Button,
    btn_save: Button,
    first_line: usize,
    focus_position: usize,
    rect: Rect,
    btn_close_rect: Rect,
    btn_save_rect: Rect,
}

impl DlgReport {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        job: &DBJobEntry,
        files: &[DBFileEntry],
        dirs: &[DBDirListEntry],
        db_file: Option<&Path>,
    ) -> DlgReport {
        let mut messages: Vec<String> = files
            .iter()
            .filter_map(|entry| match entry.status {
                DBFileStatus::ToDo | DBFileStatus::InProgress => Some(format!(
                    "ABORTED [{}] {}",
                    entry.message,
                    diff_paths(&entry.file, &job.cwd).unwrap().to_string_lossy()
                )),
                DBFileStatus::Error => Some(format!(
                    "ERROR [{}] {}",
                    entry.message,
                    diff_paths(&entry.file, &job.cwd).unwrap().to_string_lossy()
                )),
                DBFileStatus::Skipped => Some(format!(
                    "SKIPPED [{}] {}",
                    entry.message,
                    diff_paths(&entry.file, &job.cwd).unwrap().to_string_lossy()
                )),
                DBFileStatus::Done => match entry.message.is_empty() {
                    true => match job.status {
                        DBJobStatus::Aborted => Some(format!(
                            "DONE [] {}",
                            diff_paths(&entry.file, &job.cwd).unwrap().to_string_lossy()
                        )),
                        _ => None,
                    },
                    false => Some(format!(
                        "WARNING [{}] {}",
                        entry.message,
                        diff_paths(&entry.file, &job.cwd).unwrap().to_string_lossy()
                    )),
                },
            })
            .chain(dirs.iter().filter_map(|entry| match entry.status {
                DBFileStatus::ToDo | DBFileStatus::InProgress => Some(format!(
                            "ABORTED [{}] {}",
                            entry.message,
                            diff_paths(&entry.file.file, &job.cwd)
                                .unwrap()
                                .to_string_lossy()
                        )),
                DBFileStatus::Error => Some(format!(
                            "ERROR [{}] {}",
                            entry.message,
                            diff_paths(&entry.file.file, &job.cwd)
                                .unwrap()
                                .to_string_lossy()
                        )),
                DBFileStatus::Skipped => Some(format!(
                            "SKIPPED [{}] {}",
                            entry.message,
                            diff_paths(&entry.file.file, &job.cwd)
                                .unwrap()
                                .to_string_lossy()
                        )),
                DBFileStatus::Done => match entry.message.is_empty() {
                    true => match job.status {
                        DBJobStatus::Aborted => Some(format!(
                                    "DONE [] {}",
                                    diff_paths(&entry.file.file, &job.cwd)
                                        .unwrap()
                                        .to_string_lossy()
                                )),
                        _ => None,
                    },
                    false => Some(format!(
                                "WARNING [{}] {}",
                                entry.message,
                                diff_paths(&entry.file.file, &job.cwd)
                                    .unwrap()
                                    .to_string_lossy()
                            )),
                },
            }))
            .collect();

        // We want to show errors first
        messages.sort_by_cached_key(|message| match message.starts_with("ERROR") {
            true => 0,
            false => 1,
        });

        // Given that we show errors first, we only need to check if the first message is an error
        let dialog_type = match messages
            .first()
            .map(|message| message.starts_with("ERROR"))
            .unwrap_or(false)
        {
            true => DialogType::Error,
            false => DialogType::Warning,
        };

        let (style, focused_style, active_style) = match dialog_type {
            DialogType::Error => (palette.error, palette.error_focus, palette.error_title),
            DialogType::Warning | DialogType::Info => {
                (palette.dialog, palette.dialog_focus, palette.dialog_title)
            }
        };

        DlgReport {
            palette: Rc::clone(palette),
            pubsub_tx,
            job: job.clone(),
            db_file: db_file.map(PathBuf::from),
            messages,
            dialog_type,
            btn_close: Button::new("Close", &style, &focused_style, &active_style),
            btn_save: Button::new("Save", &style, &focused_style, &active_style),
            first_line: 0,
            focus_position: 0,
            rect: Rect::default(),
            btn_close_rect: Rect::default(),
            btn_save_rect: Rect::default(),
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

    fn close(&self) {
        self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
        self.pubsub_tx.send(PubSub::CloseCommandBar).unwrap();

        self.db_file
            .as_deref()
            .and_then(|db_file| DataBase::new(db_file).ok())
            .map(|db| db.delete_job(self.job.id));

        self.pubsub_tx.send(PubSub::NextPendingJob).unwrap();
    }

    fn on_save(&mut self) {
        let mut path = self.job.cwd.clone();
        path.push("fcd-report.txt");

        self.pubsub_tx
            .send(PubSub::PromptSaveReport(self.job.cwd.clone(), path))
            .unwrap();
    }
}

impl Component for DlgReport {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Char('\n') | Key::Char(' ') => match self.focus_position {
                0 => self.close(),
                1 => self.on_save(),
                _ => unreachable!(),
            },
            Key::Left | Key::Char('h') => self.focus_position = 0,
            Key::Right | Key::Char('l') => self.focus_position = 1,
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
                if self.btn_close_rect.contains(mouse_position) {
                    self.focus_position = 0;

                    if let MouseButton::Left = button {
                        self.close();
                    }
                }

                if self.btn_save_rect.contains(mouse_position) {
                    self.focus_position = 1;

                    if let MouseButton::Left = button {
                        self.on_save();
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

    fn handle_pubsub(&mut self, event: &PubSub) {
        #[allow(clippy::single_match)]
        match event {
            PubSub::DoSaveReport(path) => {
                let result = AtomicFile::new(path, AllowOverwrite).write(|f| {
                    let mut writer = BufWriter::new(f);

                    writeln!(writer, "Operation: {}", self.job.operation)?;
                    writeln!(writer, "From: {}", self.job.cwd.to_string_lossy())?;

                    if let Some(dest) = &self.job.dest {
                        writeln!(writer, "To: {}", dest.to_string_lossy())?;
                    }

                    writeln!(writer, "Files:")?;

                    for entry in &self.job.entries {
                        writeln!(
                            writer,
                            "{}",
                            diff_paths(&entry.file, &self.job.cwd).unwrap().to_string_lossy()
                        )?;
                    }

                    writeln!(writer)?;
                    writeln!(writer, "------------------------------------------------------------------------------")?;
                    writeln!(writer)?;

                    for message in &self.messages {
                        writeln!(writer, "{}", message)?;
                    }

                    Ok::<(), std::io::Error>(())
                });

                match result {
                    Ok(()) => {
                        self.pubsub_tx.send(PubSub::Reload).unwrap();
                        self.close();
                    }
                    Err(e) => {
                        self.pubsub_tx
                            .send(PubSub::CommandBarError(e.to_string()))
                            .unwrap();
                    }
                }
            }
            _ => (),
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let area = centered_rect(
            (((chunk.width as usize) * 3) / 4) as u16,
            (((chunk.height as usize) * 3) / 4) as u16,
            chunk,
        );

        let (style, title_style) = match self.dialog_type {
            DialogType::Error => (self.palette.error, self.palette.error_title),
            DialogType::Warning | DialogType::Info => {
                (self.palette.dialog, self.palette.dialog_title)
            }
        };

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(style), area);
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
                    tilde_layout(" Report ", sections[0].width as usize),
                    title_style,
                ))
                .centered(),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(style);

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
            .style(style);

        let lower_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(self.btn_close.width() as u16),
                Constraint::Length(1),
                Constraint::Length(self.btn_save.width() as u16),
            ])
            .split(centered_rect(
                (self.btn_close.width() + 1 + self.btn_save.width()) as u16,
                1,
                &lower_block.inner(sections[1]),
            ));

        self.btn_close_rect = lower_area[0];
        self.btn_save_rect = lower_area[2];

        f.render_widget(lower_block, sections[1]);
        self.btn_close.render(
            f,
            &self.btn_close_rect,
            match self.focus_position {
                0 => match focus {
                    Focus::Focused => Focus::Focused,
                    _ => Focus::Active,
                },
                _ => Focus::Normal,
            },
        );
        self.btn_save.render(
            f,
            &self.btn_save_rect,
            match self.focus_position {
                1 => match focus {
                    Focus::Focused => Focus::Focused,
                    _ => Focus::Active,
                },
                _ => Focus::Normal,
            },
        );
    }
}
