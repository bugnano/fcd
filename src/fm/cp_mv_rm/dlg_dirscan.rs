use std::{
    cmp::min,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
};

use crossbeam_channel::{Receiver, Sender};
use ratatui::{prelude::*, widgets::*};
use termion::event::*;

use thousands::Separable;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    fm::{
        app::human_readable_size,
        archive_mounter::ArchiveEntry,
        cp_mv_rm::{
            database::{DBFileEntry, DBJobEntry, DBJobOperation, DBJobStatus, DataBase},
            dirscan::{dirscan, DirScanEvent, DirScanInfo, ReadMetadata},
        },
    },
    palette::Palette,
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgDirscan {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    job: DBJobEntry,
    archive_dirs: Vec<ArchiveEntry>,
    db_file: Option<PathBuf>,
    ev_tx: Sender<DirScanEvent>,
    info_rx: Receiver<DirScanInfo>,
    result_rx: Receiver<Option<Vec<DBFileEntry>>>,
    btn_suspend: Button,
    btn_skip: Button,
    btn_abort: Button,
    current: String,
    num_files: usize,
    total_size: Option<u64>,
    focus_position: usize,
    suspend_tx: Option<Sender<()>>,
    btn_suspend_rect: Rect,
    btn_skip_rect: Rect,
    btn_abort_rect: Rect,
}

impl DlgDirscan {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        job: &DBJobEntry,
        archive_dirs: &[ArchiveEntry],
        db_file: Option<&Path>,
    ) -> DlgDirscan {
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (info_tx, info_rx) = crossbeam_channel::unbounded();
        let (result_tx, result_rx) = crossbeam_channel::unbounded();

        let mut dlg = DlgDirscan {
            palette: Rc::clone(palette),
            pubsub_tx,
            job: job.clone(),
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
            current: job.cwd.to_string_lossy().to_string(),
            num_files: 0,
            total_size: None,
            focus_position: 0,
            suspend_tx: None,
            btn_suspend_rect: Rect::default(),
            btn_skip_rect: Rect::default(),
            btn_abort_rect: Rect::default(),
        };

        dlg.dirscan_thread(ev_rx, info_tx, result_tx);

        dlg
    }

    fn dirscan_thread(
        &mut self,
        ev_rx: Receiver<DirScanEvent>,
        info_tx: Sender<DirScanInfo>,
        result_tx: Sender<Option<Vec<DBFileEntry>>>,
    ) {
        let cwd = self.job.cwd.clone();
        let entries = self.job.entries.clone();
        let archive_dirs = self.archive_dirs.clone();

        let read_metadata = match &self.job.operation {
            DBJobOperation::Cp => ReadMetadata::Yes,
            DBJobOperation::Mv => ReadMetadata::Yes,
            DBJobOperation::Rm => ReadMetadata::No,
        };

        let pubsub_tx = self.pubsub_tx.clone();

        thread::spawn(move || {
            let result = dirscan(
                &cwd,
                &entries,
                &archive_dirs,
                read_metadata,
                ev_rx,
                info_tx,
                pubsub_tx.clone(),
            );

            let _ = result_tx.send(result);
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        });
    }

    fn suspend(&mut self) {
        if self.suspend_tx.is_none() {
            let (suspend_tx, suspend_rx) = crossbeam_channel::unbounded();

            let _ = self.ev_tx.send(DirScanEvent::Suspend(suspend_rx));

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

impl Component for DlgDirscan {
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

                    let _ = self.ev_tx.send(DirScanEvent::Skip);
                }
                2 => {
                    self.resume();

                    let _ = self.ev_tx.send(DirScanEvent::Abort);
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

                    let _ = self.ev_tx.send(DirScanEvent::Skip);
                }
            }

            if self.btn_abort_rect.contains(mouse_position) {
                self.focus_position = 2;

                if let MouseButton::Left = button {
                    self.resume();

                    let _ = self.ev_tx.send(DirScanEvent::Abort);
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
                    self.total_size = info.total_size;
                }

                if let Ok(result) = self.result_rx.try_recv() {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    match result {
                        Some(mut files) => {
                            self.db_file
                                .as_deref()
                                .and_then(|db_file| DataBase::new(db_file).ok())
                                .map(|mut db| db.set_file_list(self.job.id, &mut files));

                            self.job.status = DBJobStatus::InProgress;

                            match &self.job.operation {
                                DBJobOperation::Cp => {
                                    self.pubsub_tx
                                        .send(PubSub::DoCp(
                                            self.job.clone(),
                                            files,
                                            self.archive_dirs.clone(),
                                        ))
                                        .unwrap();
                                }
                                DBJobOperation::Mv => {
                                    self.pubsub_tx
                                        .send(PubSub::DoMv(
                                            self.job.clone(),
                                            files,
                                            self.archive_dirs.clone(),
                                        ))
                                        .unwrap();
                                }
                                DBJobOperation::Rm => {
                                    self.pubsub_tx
                                        .send(PubSub::DoRm(
                                            self.job.clone(),
                                            files,
                                            self.archive_dirs.clone(),
                                        ))
                                        .unwrap();
                                }
                            }
                        }
                        None => {
                            // If result is None it means that the operation has been aborted
                            self.db_file
                                .as_deref()
                                .and_then(|db_file| DataBase::new(db_file).ok())
                                .map(|db| db.delete_job(self.job.id));

                            self.pubsub_tx.send(PubSub::NextPendingJob).unwrap();
                        }
                    }
                }
            }
            _ => (),
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect((((chunk.width as usize) * 3) / 4) as u16, 9, chunk);

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.palette.dialog), area);
        if let Some(shadow) = self.palette.shadow {
            render_shadow(f, &area, &shadow);
        }

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Length(3)])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let upper_block = Block::default()
            .title_top(
                Line::from(Span::styled(
                    tilde_layout(" Directory scanning ", sections[0].width as usize),
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
            ])
            .split(upper_block.inner(sections[0]));

        let current = Paragraph::new(Span::raw(tilde_layout(
            &self.current,
            upper_area[0].width as usize,
        )));
        let num_files = Paragraph::new(Span::raw(tilde_layout(
            &format!("Files: {}", self.num_files.separate_with_commas()),
            upper_area[1].width as usize,
        )));
        let total_size = Paragraph::new(Span::raw(tilde_layout(
            &format!(
                "Total size: {}",
                match self.total_size {
                    Some(bytes) => human_readable_size(bytes),
                    None => "n/a".to_string(),
                }
            ),
            upper_area[2].width as usize,
        )));

        f.render_widget(upper_block, sections[0]);
        f.render_widget(current, upper_area[0]);
        f.render_widget(num_files, upper_area[1]);
        f.render_widget(total_size, upper_area[2]);

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
                &lower_block.inner(sections[1]),
            ));

        self.btn_suspend_rect = lower_area[0];
        self.btn_skip_rect = lower_area[2];
        self.btn_abort_rect = lower_area[4];

        f.render_widget(lower_block, sections[1]);
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
