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
    btn_close: Button,
    btn_save: Button,
    first_line: usize,
    focus_position: usize,
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
        let messages = files
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

        DlgReport {
            config: Rc::clone(config),
            pubsub_tx,
            job: job.clone(),
            db_file: db_file.map(PathBuf::from),
            messages,
            btn_close: Button::new(
                "Close",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
            btn_save: Button::new(
                "Save",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
            first_line: 0,
            focus_position: 0,
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
                    todo!();
                }
                _ => unreachable!(),
            },
            Key::Left | Key::Char('h') => self.focus_position = 0,
            Key::Right | Key::Char('l') => self.focus_position = 1,
            Key::Ctrl('c') => key_handled = false,
            Key::Ctrl('l') => key_handled = false,
            Key::Ctrl('z') => key_handled = false,
            Key::Ctrl('o') => key_handled = false,
            _ => (),
        }

        key_handled
    }

    fn handle_pubsub(&mut self, event: &PubSub) {
        match event {
            PubSub::ComponentThreadEvent => {}
            _ => (),
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect(
            (((chunk.width as usize) * 3) / 4) as u16,
            (((chunk.height as usize) * 3) / 4) as u16,
            chunk,
        );

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

        let upper_area = upper_block.inner(sections[0]);

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
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

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
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
        self.btn_save.render(
            f,
            &lower_area[2],
            match self.focus_position {
                1 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
    }
}
