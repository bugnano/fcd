use std::{
    cmp::min,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender};
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use thousands::Separable;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    fm::{
        app::{format_seconds, human_readable_size},
        archive_mounter::ArchiveEntry,
        cp_mv_rm::{
            cp_mv::{cp_mv, CpMvEvent, CpMvInfo, CpMvResult},
            database::{DBFileEntry, DBJobEntry, DBJobOperation},
        },
    },
    palette::Palette,
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgCpMvProgress {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    job: DBJobEntry,
    files: Vec<DBFileEntry>,
    archive_dirs: Vec<ArchiveEntry>,
    db_file: Option<PathBuf>,
    operation: DBJobOperation,
    ev_tx: Sender<CpMvEvent>,
    info_rx: Receiver<CpMvInfo>,
    result_rx: Receiver<CpMvResult>,
    btn_suspend: Button,
    btn_skip: Button,
    btn_abort: Button,
    btn_no_db: Button,
    total_size: u64,
    cur_source: String,
    cur_target: String,
    cur_size: u64,
    cur_bytes: u64,
    cur_time: Duration,
    num_files: usize,
    total_bytes: u64,
    total_time: Duration,
    focus_position: usize,
    suspend_tx: Option<Sender<()>>,
    btn_suspend_rect: Rect,
    btn_skip_rect: Rect,
    btn_abort_rect: Rect,
    btn_no_db_rect: Rect,
}

impl DlgCpMvProgress {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        job: &DBJobEntry,
        files: &[DBFileEntry],
        archive_dirs: &[ArchiveEntry],
        db_file: Option<&Path>,
        operation: DBJobOperation,
    ) -> DlgCpMvProgress {
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (info_tx, info_rx) = crossbeam_channel::unbounded();
        let (result_tx, result_rx) = crossbeam_channel::unbounded();

        let mut dlg = DlgCpMvProgress {
            palette: Rc::clone(palette),
            pubsub_tx,
            job: job.clone(),
            files: Vec::from(files),
            archive_dirs: Vec::from(archive_dirs),
            db_file: db_file.map(PathBuf::from),
            operation,
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
            btn_no_db: Button::new(
                "No DB",
                &palette.dialog,
                &palette.dialog_focus,
                &palette.dialog_title,
            ),
            total_size: files.iter().map(|entry| entry.size).sum(),
            cur_source: String::from(""),
            cur_target: String::from(""),
            cur_size: 0,
            cur_bytes: 0,
            cur_time: Duration::ZERO,
            num_files: 0,
            total_bytes: 0,
            total_time: Duration::ZERO,
            focus_position: 0,
            suspend_tx: None,
            btn_suspend_rect: Rect::default(),
            btn_skip_rect: Rect::default(),
            btn_abort_rect: Rect::default(),
            btn_no_db_rect: Rect::default(),
        };

        dlg.cp_mv_thread(ev_rx, info_tx, result_tx);

        dlg
    }

    fn cp_mv_thread(
        &mut self,
        ev_rx: Receiver<CpMvEvent>,
        info_tx: Sender<CpMvInfo>,
        result_tx: Sender<CpMvResult>,
    ) {
        let job_id = self.job.id;
        let operation = self.operation;
        let entries = self.files.clone();
        let cwd = self.job.cwd.clone();

        let dest = self
            .job
            .dest
            .clone()
            .expect("BUG: CP/MV operation without dest");

        let on_conflict = self
            .job
            .on_conflict
            .expect("BUG: CP/MV operation without on_conflict");

        let replace_first_path = self.job.replace_first_path;

        let db_file = self.db_file.clone();
        let archive_dirs = self.archive_dirs.clone();

        let pubsub_tx = self.pubsub_tx.clone();

        thread::spawn(move || {
            let result = cp_mv(
                job_id,
                operation,
                &cwd,
                &dest,
                on_conflict,
                replace_first_path,
                &entries,
                ev_rx,
                info_tx,
                pubsub_tx.clone(),
                db_file.as_deref(),
                &archive_dirs,
            );

            let _ = result_tx.send(result);
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        });
    }

    fn suspend(&mut self) {
        if self.suspend_tx.is_none() {
            let (suspend_tx, suspend_rx) = crossbeam_channel::unbounded();

            let _ = self.ev_tx.send(CpMvEvent::Suspend(suspend_rx));

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

impl Component for DlgCpMvProgress {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Char('\n') | Key::Char(' ') => match self.focus_position {
                0 => match &self.suspend_tx {
                    Some(_suspend_tx) => self.resume(),
                    None => self.suspend(),
                },
                1 => {
                    self.resume();

                    let _ = self.ev_tx.send(CpMvEvent::Skip);
                }
                2 => {
                    self.resume();

                    let _ = self.ev_tx.send(CpMvEvent::Abort);
                }
                3 => {
                    self.resume();

                    let _ = self.ev_tx.send(CpMvEvent::NoDb);
                }
                _ => unreachable!(),
            },
            Key::Left | Key::Char('h') => {
                self.focus_position = self.focus_position.saturating_sub(1);
            }
            Key::Right | Key::Char('l') => self.focus_position = min(self.focus_position + 1, 3),
            Key::Ctrl('c') => key_handled = false,
            Key::Ctrl('l') => key_handled = false,
            Key::Ctrl('z') => key_handled = false,
            Key::Ctrl('o') => key_handled = false,
            _ => (),
        }

        key_handled
    }

    fn handle_mouse(&mut self, button: MouseButton, mouse_position: Position) {
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

                    let _ = self.ev_tx.send(CpMvEvent::Skip);
                }
            }

            if self.btn_abort_rect.contains(mouse_position) {
                self.focus_position = 2;

                if let MouseButton::Left = button {
                    self.resume();

                    let _ = self.ev_tx.send(CpMvEvent::Abort);
                }
            }

            if self.btn_no_db_rect.contains(mouse_position) {
                self.focus_position = 3;

                if let MouseButton::Left = button {
                    self.resume();

                    let _ = self.ev_tx.send(CpMvEvent::NoDb);
                }
            }
        }
    }

    fn handle_pubsub(&mut self, event: &PubSub) {
        #[allow(clippy::single_match)]
        match event {
            PubSub::ComponentThreadEvent => {
                if let Ok(info) = self.info_rx.try_recv() {
                    self.cur_source = info.cur_source.to_string_lossy().to_string();
                    self.cur_target = info.cur_target.to_string_lossy().to_string();
                    self.cur_size = info.cur_size;
                    self.cur_bytes = info.cur_bytes;
                    self.cur_time = info.cur_time;
                    self.num_files = info.num_files;
                    self.total_bytes = info.total_bytes;
                    self.total_time = info.total_time;
                }

                if let Ok(result) = self.result_rx.try_recv() {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    self.job.status = result.status;

                    self.pubsub_tx
                        .send(PubSub::JobCompleted(
                            self.job.clone(),
                            result.files,
                            result.dirs,
                        ))
                        .unwrap();
                }
            }
            _ => (),
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect((((chunk.width as usize) * 3) / 4) as u16, 16, chunk);

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.palette.dialog), area);
        if let Some(shadow) = self.palette.shadow {
            render_shadow(f, &area, &shadow);
        }

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
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
            .title_top(
                Line::from(Span::styled(
                    tilde_layout(&format!(" {} ", self.operation), sections[0].width as usize),
                    self.palette.dialog_title,
                ))
                .centered(),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let upper_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(upper_block.inner(sections[0]));

        let lbl_source = Paragraph::new(Span::raw(tilde_layout(
            "Source",
            upper_area[0].width as usize,
        )));

        let cur_source = Paragraph::new(Span::raw(tilde_layout(
            &self.cur_source,
            upper_area[1].width as usize,
        )));

        let lbl_target = Paragraph::new(Span::raw(tilde_layout(
            "Target",
            upper_area[2].width as usize,
        )));

        let cur_target = Paragraph::new(Span::raw(tilde_layout(
            &self.cur_target,
            upper_area[3].width as usize,
        )));

        let gauge_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(upper_area[4]);

        let ratio = match self.cur_size {
            0 => 0.0,
            cur_size => (self.cur_bytes as f64) / (cur_size as f64),
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

        let cur_bps = match self.cur_time.as_secs_f64() {
            0.0 => 0.0,
            secs => (self.cur_bytes as f64) / secs,
        };

        let cur_eta = match cur_bps {
            0.0 => 0,
            _ => (((self.cur_size - self.cur_bytes) as f64) / cur_bps).round() as u64,
        };

        let cur_stats = Paragraph::new(Span::raw(tilde_layout(
            &format!(
                "{}/{} ETA {} ({}/s)",
                human_readable_size(self.cur_bytes),
                human_readable_size(self.cur_size),
                format_seconds(cur_eta),
                human_readable_size(cur_bps.round() as u64)
            ),
            upper_area[5].width as usize,
        )));

        f.render_widget(upper_block, sections[0]);
        f.render_widget(lbl_source, upper_area[0]);
        f.render_widget(cur_source, upper_area[1]);
        f.render_widget(lbl_target, upper_area[2]);
        f.render_widget(cur_target, upper_area[3]);
        f.render_widget(gauge_left, gauge_area[0]);
        f.render_widget(gauge, gauge_area[1]);
        f.render_widget(gauge_right, gauge_area[2]);
        f.render_widget(cur_stats, upper_area[5]);

        // Middle section

        let middle_block = Block::default()
            .title_top(
                Line::from(Span::raw(tilde_layout(
                    &format!(
                        " Total: {}/{} ",
                        human_readable_size(self.total_bytes),
                        human_readable_size(self.total_size)
                    ),
                    sections[0].width as usize,
                )))
                .centered(),
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

        let ratio = match self.total_size {
            0 => 0.0,
            total_size => (self.total_bytes as f64) / (total_size as f64),
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

        let total_bps = match self.total_time.as_secs_f64() {
            0.0 => 0.0,
            secs => (self.total_bytes as f64) / secs,
        };

        let total_eta = match total_bps {
            0.0 => 0,
            _ => (((self.total_size - self.total_bytes) as f64) / total_bps).round() as u64,
        };

        let total_time = Paragraph::new(Span::raw(tilde_layout(
            &format!(
                "Time: {} ETA {} ({}/s)",
                format_seconds(self.total_time.as_secs()),
                format_seconds(total_eta),
                human_readable_size(total_bps.round() as u64)
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
                Constraint::Length(1),
                Constraint::Length(self.btn_no_db.width() as u16),
            ])
            .split(centered_rect(
                (self.btn_suspend.width()
                    + 1
                    + self.btn_skip.width()
                    + 1
                    + self.btn_abort.width()
                    + 1
                    + self.btn_no_db.width()) as u16,
                1,
                &lower_block.inner(sections[2]),
            ));

        self.btn_suspend_rect = lower_area[0];
        self.btn_skip_rect = lower_area[2];
        self.btn_abort_rect = lower_area[4];
        self.btn_no_db_rect = lower_area[6];

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
        self.btn_no_db.render(
            f,
            &self.btn_no_db_rect,
            match self.focus_position {
                3 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
    }
}
