use std::{
    cmp::min,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender};
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use termion::event::*;

use thousands::Separable;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    fm::{
        app::format_seconds,
        archive_mounter::ArchiveEntry,
        cp_mv_rm::{
            database::{DBFileEntry, DBJobEntry, DBJobStatus, DataBase},
            rm::{rm, RmEvent, RmInfo},
        },
    },
    palette::Palette,
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgRmProgress {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    job: DBJobEntry,
    files: Vec<DBFileEntry>,
    archive_dirs: Vec<ArchiveEntry>,
    db_file: Option<PathBuf>,
    ev_tx: Sender<RmEvent>,
    info_rx: Receiver<RmInfo>,
    result_rx: Receiver<(Vec<DBFileEntry>, DBJobStatus)>,
    btn_suspend: Button,
    btn_skip: Button,
    btn_abort: Button,
    current: String,
    num_files: usize,
    total_time: Duration,
    focus_position: usize,
    suspend_tx: Option<Sender<()>>,
    btn_suspend_rect: Rect,
    btn_skip_rect: Rect,
    btn_abort_rect: Rect,
}

impl DlgRmProgress {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        job: &DBJobEntry,
        files: &[DBFileEntry],
        archive_dirs: &[ArchiveEntry],
        db_file: Option<&Path>,
    ) -> DlgRmProgress {
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (info_tx, info_rx) = crossbeam_channel::unbounded();
        let (result_tx, result_rx) = crossbeam_channel::unbounded();

        let mut dlg = DlgRmProgress {
            palette: Rc::clone(palette),
            pubsub_tx,
            job: job.clone(),
            files: Vec::from(files),
            archive_dirs: Vec::from(archive_dirs),
            db_file: db_file.map(PathBuf::from),
            ev_tx,
            info_rx,
            result_rx,
            btn_suspend: Button::new(
                "Suspend ",
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
            current: String::from(""),
            num_files: 0,
            total_time: Duration::ZERO,
            focus_position: 0,
            suspend_tx: None,
            btn_suspend_rect: Rect::default(),
            btn_skip_rect: Rect::default(),
            btn_abort_rect: Rect::default(),
        };

        dlg.rm_thread(ev_rx, info_tx, result_tx);

        dlg
    }

    fn rm_thread(
        &mut self,
        ev_rx: Receiver<RmEvent>,
        info_tx: Sender<RmInfo>,
        result_tx: Sender<(Vec<DBFileEntry>, DBJobStatus)>,
    ) {
        let entries = self.files.clone();
        let archive_dirs = self.archive_dirs.clone();
        let pubsub_tx = self.pubsub_tx.clone();

        thread::spawn(move || {
            let result = rm(&entries, ev_rx, info_tx, pubsub_tx.clone(), &archive_dirs);

            let _ = result_tx.send(result);
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        });
    }

    fn suspend(&mut self) {
        if self.suspend_tx.is_none() {
            let (suspend_tx, suspend_rx) = crossbeam_channel::unbounded();

            let _ = self.ev_tx.send(RmEvent::Suspend(suspend_rx));

            self.btn_suspend.set_label("Continue");

            self.suspend_tx = Some(suspend_tx);
        }
    }

    fn resume(&mut self) {
        if let Some(suspend_tx) = &self.suspend_tx {
            let _ = suspend_tx.send(());

            self.btn_suspend.set_label("Suspend ");

            self.suspend_tx = None;
        }
    }
}

impl Component for DlgRmProgress {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                self.resume();

                let _ = self.ev_tx.send(RmEvent::Abort);
            }
            Key::Char('\n') | Key::Char(' ') => match self.focus_position {
                0 => match &self.suspend_tx {
                    Some(_suspend_tx) => self.resume(),
                    None => self.suspend(),
                },
                1 => {
                    self.resume();

                    let _ = self.ev_tx.send(RmEvent::Skip);
                }
                2 => {
                    self.resume();

                    let _ = self.ev_tx.send(RmEvent::Abort);
                }
                _ => unreachable!(),
            },
            Key::Left | Key::Char('h') => {
                self.focus_position = self.focus_position.saturating_sub(1);
            }
            Key::Right | Key::Char('l') => self.focus_position = min(self.focus_position + 1, 2),
            Key::Ctrl('c') => key_handled = false,
            Key::Ctrl('l') => key_handled = false,
            Key::Ctrl('z') => key_handled = false,
            Key::Ctrl('o') => key_handled = false,
            _ => (),
        }

        key_handled
    }

    fn handle_mouse(&mut self, button: MouseButton, mouse_position: layout::Position) {
        if matches!(button, MouseButton::Left | MouseButton::Right) {
            if self.btn_suspend_rect.contains(mouse_position) {
                self.focus_position = 0;

                if let MouseButton::Left = button {
                    match &self.suspend_tx {
                        Some(_suspend_tx) => self.resume(),
                        None => self.suspend(),
                    }
                }
            }

            if self.btn_skip_rect.contains(mouse_position) {
                self.focus_position = 1;

                if let MouseButton::Left = button {
                    self.resume();

                    let _ = self.ev_tx.send(RmEvent::Skip);
                }
            }

            if self.btn_abort_rect.contains(mouse_position) {
                self.focus_position = 2;

                if let MouseButton::Left = button {
                    self.resume();

                    let _ = self.ev_tx.send(RmEvent::Abort);
                }
            }
        }
    }

    fn handle_pubsub(&mut self, event: &PubSub) {
        #[allow(clippy::single_match)]
        match event {
            PubSub::ComponentThreadEvent => {
                if let Ok(info) = self.info_rx.try_recv() {
                    self.current = info.current.to_string_lossy().to_string();
                    self.num_files = info.num_files;
                    self.total_time = info.total_time;
                }

                if let Ok((files, status)) = self.result_rx.try_recv() {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    self.job.status = status;

                    self.db_file
                        .as_deref()
                        .and_then(|db_file| DataBase::new(db_file).ok())
                        .map(|mut db| {
                            db.update_file_list(&files);
                            db.set_job_status(self.job.id, status);
                        });

                    self.pubsub_tx
                        .send(PubSub::JobCompleted(self.job.clone(), files, Vec::new()))
                        .unwrap();
                }
            }
            _ => (),
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect((((chunk.width as usize) * 3) / 4) as u16, 11, chunk);

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.palette.dialog), area);
        if let Some(shadow) = self.palette.shadow {
            render_shadow(f, &area, &shadow);
        }

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(4),
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
                Title::from(Span::styled(
                    tilde_layout(" Delete ", sections[0].width as usize),
                    self.palette.dialog_title,
                ))
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let upper_area = upper_block.inner(sections[0]);

        let current = Paragraph::new(Span::raw(tilde_layout(
            &self.current,
            upper_area.width as usize,
        )));

        f.render_widget(upper_block, sections[0]);
        f.render_widget(current, upper_area);

        // Middle section

        let middle_block = Block::default()
            .title(
                Title::from(Span::raw(tilde_layout(
                    &format!(
                        " Total: {}/{} ",
                        self.num_files.separate_with_commas(),
                        self.files.len().separate_with_commas()
                    ),
                    sections[0].width as usize,
                )))
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_set(MIDDLE_BORDER_SET)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let middle_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(middle_block.inner(sections[1]));

        let gauge_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(middle_area[0]);

        let ratio = match self.files.len() {
            0 => 0.0,
            len => (self.num_files as f64) / (len as f64),
        };

        let gauge = Gauge::default()
            .gauge_style(self.palette.dialog)
            .label(tilde_layout(
                &format!("{} %", (ratio * 100.0) as usize),
                gauge_area[1].width as usize,
            ))
            .ratio(ratio);

        let gauge_left = Paragraph::new(Span::raw("["));
        let gauge_right = Paragraph::new(Span::raw("]"));

        let num_files = Paragraph::new(Span::raw(tilde_layout(
            &format!(
                "Files processed: {}/{}",
                self.num_files.separate_with_commas(),
                self.files.len().separate_with_commas()
            ),
            middle_area[1].width as usize,
        )));

        let total_fps = match self.total_time.as_secs_f64() {
            0.0 => 0.0,
            secs => (self.files.len() as f64) / secs,
        };

        let total_eta = match total_fps {
            0.0 => 0,
            _ => (((self.files.len() - self.num_files) as f64) / total_fps).round() as u64,
        };

        let total_time = Paragraph::new(Span::raw(tilde_layout(
            &format!(
                "Time: {} ETA {}",
                format_seconds(self.total_time.as_secs()),
                format_seconds(total_eta)
            ),
            middle_area[2].width as usize,
        )));

        f.render_widget(middle_block, sections[1]);
        f.render_widget(gauge_left, gauge_area[0]);
        f.render_widget(gauge, gauge_area[1]);
        f.render_widget(gauge_right, gauge_area[2]);
        f.render_widget(num_files, middle_area[1]);
        f.render_widget(total_time, middle_area[2]);

        // Lower section

        let lower_block = Block::default()
            .borders(Borders::ALL)
            .border_set(MIDDLE_BORDER_SET)
            .style(self.palette.dialog);

        let lower_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(self.btn_suspend.width() as u16),
                Constraint::Length(1),
                Constraint::Length(self.btn_skip.width() as u16),
                Constraint::Length(1),
                Constraint::Length(self.btn_abort.width() as u16),
            ])
            .split(centered_rect(
                (self.btn_suspend.width() + 1 + self.btn_skip.width() + 1 + self.btn_abort.width())
                    as u16,
                1,
                &lower_block.inner(sections[2]),
            ));

        self.btn_suspend_rect = lower_area[0];
        self.btn_skip_rect = lower_area[2];
        self.btn_abort_rect = lower_area[4];

        f.render_widget(lower_block, sections[2]);
        self.btn_suspend.render(
            f,
            &self.btn_suspend_rect,
            match self.focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
        self.btn_skip.render(
            f,
            &self.btn_skip_rect,
            match self.focus_position {
                1 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
        self.btn_abort.render(
            f,
            &self.btn_abort_rect,
            match self.focus_position {
                2 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
    }
}
