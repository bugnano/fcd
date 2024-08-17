use std::{
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

use pathdiff::diff_paths;

use crate::{
    app::{centered_rect, render_shadow, PubSub},
    component::{Component, Focus},
    config::Config,
    dlg_error::DialogType,
    fm::cp_mv_rm::database::{
        DBDirListEntry, DBFileEntry, DBFileStatus, DBJobEntry, DBJobStatus, DataBase,
    },
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgReport {
    config: Rc<Config>,
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
}

impl DlgReport {
    pub fn new(
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        job: &DBJobEntry,
        files: &[DBFileEntry],
        dirs: Option<&[DBDirListEntry]>,
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
            .chain(
                dirs.unwrap_or_default()
                    .iter()
                    .filter_map(|entry| match entry.status {
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
                    }),
            )
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
            DialogType::Error => (
                Style::default().fg(config.error.fg).bg(config.error.bg),
                Style::default()
                    .fg(config.error.focus_fg)
                    .bg(config.error.focus_bg),
                Style::default()
                    .fg(config.error.title_fg)
                    .bg(config.error.bg),
            ),
            DialogType::Warning | DialogType::Info => (
                Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
        };

        DlgReport {
            config: Rc::clone(config),
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

        self.db_file
            .as_deref()
            .and_then(|db_file| DataBase::new(db_file).ok())
            .map(|db| db.delete_job(self.job.id));

        // TODO: Process next pending job
    }
}

impl Component for DlgReport {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                self.close();
            }
            Key::Char('\n') | Key::Char(' ') => match self.focus_position {
                0 => self.close(),
                1 => {
                    let mut path = self.job.cwd.clone();
                    path.push("fcd-report.txt");

                    self.pubsub_tx
                        .send(PubSub::PromptSaveReport(self.job.cwd.clone(), path))
                        .unwrap();
                }
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
            Key::Home | Key::Char('g') => {
                self.first_line = 0;
            }
            Key::End | Key::Char('G') => {
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

    fn handle_pubsub(&mut self, event: &PubSub) {
        #[allow(clippy::single_match)]
        match event {
            PubSub::DoSaveReport(path) => {
                todo!();
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
            DialogType::Error => (
                Style::default()
                    .fg(self.config.error.fg)
                    .bg(self.config.error.bg),
                Style::default()
                    .fg(self.config.error.title_fg)
                    .bg(self.config.error.bg),
            ),
            DialogType::Warning | DialogType::Info => (
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
                Style::default()
                    .fg(self.config.dialog.title_fg)
                    .bg(self.config.dialog.bg),
            ),
        };

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(style), area);
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
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let upper_block = Block::default()
            .title(
                Title::from(Span::styled(
                    tilde_layout(" Report ", sections[0].width as usize),
                    title_style,
                ))
                .position(Position::Top)
                .alignment(Alignment::Center),
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
            .border_set(middle_border_set)
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

        f.render_widget(lower_block, sections[1]);
        self.btn_close.render(
            f,
            &lower_area[0],
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
            &lower_area[2],
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
