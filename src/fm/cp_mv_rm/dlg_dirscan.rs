use std::{
    cmp::min,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
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
    app::{centered_rect, render_shadow, PubSub},
    component::{Component, Focus},
    config::Config,
    fm::{
        app::human_readable_size,
        archive_mounter::ArchiveEntry,
        cp_mv_rm::{
            database::{DBFileEntry, DBJobEntry, DBJobOperation, DBJobStatus, DataBase},
            dirscan::{dirscan, DirScanEvent, DirScanInfo, ReadMetadata},
        },
    },
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgDirscan {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    job: DBJobEntry,
    archive_dirs: Vec<ArchiveEntry>,
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
    db_file: Option<PathBuf>,
    suspend_tx: Option<Sender<()>>,
}

impl DlgDirscan {
    pub fn new(
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        job: &DBJobEntry,
        archive_dirs: &[ArchiveEntry],
        db_file: Option<&Path>,
    ) -> DlgDirscan {
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (info_tx, info_rx) = crossbeam_channel::unbounded();
        let (result_tx, result_rx) = crossbeam_channel::unbounded();

        let mut dlg = DlgDirscan {
            config: Rc::clone(config),
            pubsub_tx,
            job: job.clone(),
            archive_dirs: Vec::from(archive_dirs),
            ev_tx,
            info_rx,
            result_rx,
            btn_suspend: Button::new(
                "Suspend ",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
            btn_skip: Button::new(
                "Skip",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
            btn_abort: Button::new(
                "Abort",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
            current: job.cwd.to_string_lossy().to_string(),
            num_files: 0,
            total_size: None,
            focus_position: 0,
            db_file: db_file.map(PathBuf::from),
            suspend_tx: None,
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

    fn delete_job(&self) {
        self.db_file
            .as_deref()
            .and_then(|db_file| DataBase::new(db_file).ok())
            .map(|db| db.delete_job(self.job.id));
    }
}

impl Component for DlgDirscan {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                // Resume the thread before the abort
                self.suspend_tx.as_ref().map(|suspend_tx| {
                    let _ = suspend_tx.send(());
                });

                let _ = self.ev_tx.send(DirScanEvent::Abort);

                self.delete_job();
            }
            Key::Char('\n') | Key::Char(' ') => match self.focus_position {
                0 => match &self.suspend_tx {
                    Some(suspend_tx) => {
                        let _ = suspend_tx.send(());

                        self.btn_suspend.set_label("Suspend ");

                        self.suspend_tx = None;
                    }
                    None => {
                        let (suspend_tx, suspend_rx) = crossbeam_channel::unbounded();

                        let _ = self.ev_tx.send(DirScanEvent::Suspend(suspend_rx));

                        self.btn_suspend.set_label("Continue");

                        self.suspend_tx = Some(suspend_tx);
                    }
                },
                1 => {
                    let _ = self.ev_tx.send(DirScanEvent::Skip);
                }
                2 => {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    // Resume the thread before the abort
                    self.suspend_tx.as_ref().map(|suspend_tx| {
                        let _ = suspend_tx.send(());
                    });

                    let _ = self.ev_tx.send(DirScanEvent::Abort);

                    self.delete_job();
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

    fn handle_pubsub(&mut self, event: &PubSub) {
        match event {
            PubSub::ComponentThreadEvent => {
                if let Ok(info) = self.info_rx.try_recv() {
                    self.current = info.current.to_string_lossy().to_string();
                    self.num_files = info.num_files;
                    self.total_size = info.total_size;
                }

                if let Ok(result) = self.result_rx.try_recv() {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    // If result is None it means that the operation has been aborted, and the
                    // DB job has already been deleted, so there would be nothing to do here
                    if let Some(mut files) = result {
                        self.db_file
                            .as_deref()
                            .and_then(|db_file| DataBase::new(db_file).ok())
                            .map(|mut db| db.set_file_list(self.job.id, &mut files));

                        self.job.status = DBJobStatus::InProgress;

                        match &self.job.operation {
                            DBJobOperation::Cp => {
                                self.pubsub_tx
                                    .send(PubSub::DoCp(self.job.clone(), files))
                                    .unwrap();
                            }
                            DBJobOperation::Mv => {
                                self.pubsub_tx
                                    .send(PubSub::DoMv(self.job.clone(), files))
                                    .unwrap();
                            }
                            DBJobOperation::Rm => {
                                self.pubsub_tx
                                    .send(PubSub::DoRm(self.job.clone(), files))
                                    .unwrap();
                            }
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
            .constraints([Constraint::Length(6), Constraint::Length(3)])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let upper_block = Block::default()
            .title(
                Title::from(Span::styled(
                    tilde_layout(" Directory scanning ", sections[0].width as usize),
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
            .border_set(middle_border_set)
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

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

        f.render_widget(lower_block, sections[1]);
        self.btn_suspend.render(
            f,
            &lower_area[0],
            match self.focus_position {
                0 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
        self.btn_skip.render(
            f,
            &lower_area[2],
            match self.focus_position {
                1 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
        self.btn_abort.render(
            f,
            &lower_area[4],
            match self.focus_position {
                2 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
    }
}
