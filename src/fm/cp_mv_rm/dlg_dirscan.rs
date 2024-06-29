use std::{
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
        archive_mounter::{self, ArchiveMounterCommand},
        cp_mv_rm::{
            dirscan::{dirscan, DirScanEvent, DirScanInfo, DirScanResult, ReadMetadata},
            dlg_cp_mv::OnConflict,
        },
        entry::Entry,
    },
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug, Clone)]
pub enum DirscanType {
    Cp(PathBuf, OnConflict),
    Mv(PathBuf, OnConflict),
    Rm,
}

#[derive(Debug)]
pub struct DlgDirscan {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    entries: Vec<Entry>,
    dirscan_type: DirscanType,
    ev_tx: Sender<DirScanEvent>,
    info_rx: Receiver<DirScanInfo>,
    result_rx: Receiver<DirScanResult>,
    btn_abort: Button,
    btn_skip: Button,
    current: String,
    files: usize,
    total_size: Option<u64>,
    focus_position: usize,
    archive_mounter_command_tx: Option<Sender<ArchiveMounterCommand>>,
}

impl DlgDirscan {
    pub fn new(
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        cwd: &Path,
        entries: &[Entry],
        dirscan_type: DirscanType,
        archive_mounter_command_tx: Option<Sender<ArchiveMounterCommand>>,
    ) -> DlgDirscan {
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (info_tx, info_rx) = crossbeam_channel::unbounded();
        let (result_tx, result_rx) = crossbeam_channel::unbounded();

        let current = match &archive_mounter_command_tx {
            Some(command_tx) => archive_mounter::archive_path(command_tx, cwd)
                .to_string_lossy()
                .to_string(),
            None => cwd.to_string_lossy().to_string(),
        };

        let mut dlg = DlgDirscan {
            config: Rc::clone(config),
            pubsub_tx,
            entries: Vec::from(entries),
            dirscan_type,
            ev_tx,
            info_rx,
            result_rx,
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
            current,
            files: 0,
            total_size: None,
            focus_position: 0,
            archive_mounter_command_tx,
        };

        dlg.dirscan_thread(cwd, ev_rx, info_tx, result_tx);

        dlg
    }

    fn dirscan_thread(
        &mut self,
        cwd: &Path,
        ev_rx: Receiver<DirScanEvent>,
        info_tx: Sender<DirScanInfo>,
        result_tx: Sender<DirScanResult>,
    ) {
        let entries = self.entries.clone();

        let read_metadata = match &self.dirscan_type {
            DirscanType::Cp(_dest, _on_conflict) => ReadMetadata::Yes,
            DirscanType::Mv(_dest, _on_conflict) => ReadMetadata::Yes,
            DirscanType::Rm => ReadMetadata::No,
        };

        let pubsub_tx = self.pubsub_tx.clone();
        let cwd = PathBuf::from(cwd);

        thread::spawn(move || {
            let result = dirscan(
                &entries,
                &cwd,
                read_metadata,
                ev_rx,
                info_tx,
                pubsub_tx.clone(),
            );
            let _ = result_tx.send(result);
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        });
    }

    fn archive_path(&self, file: &Path) -> PathBuf {
        match &self.archive_mounter_command_tx {
            Some(command_tx) => archive_mounter::archive_path(command_tx, file),
            None => PathBuf::from(file),
        }
    }
}

impl Component for DlgDirscan {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                let _ = self.ev_tx.send(DirScanEvent::Abort);
            }
            Key::Char('\n') | Key::Char(' ') => match self.focus_position {
                0 => {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    let _ = self.ev_tx.send(DirScanEvent::Abort);
                }
                1 => {
                    let _ = self.ev_tx.send(DirScanEvent::Skip);
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
            PubSub::ComponentThreadEvent => {
                if let Ok(info) = self.info_rx.try_recv() {
                    self.current = self
                        .archive_path(&info.current)
                        .to_string_lossy()
                        .to_string();
                    self.files = info.files;
                    self.total_size = info.bytes;
                }

                if let Ok(result) = self.result_rx.try_recv() {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    match &self.dirscan_type {
                        DirscanType::Cp(dest, on_conflict) => {
                            self.pubsub_tx
                                .send(PubSub::DoCp(
                                    self.entries.clone(),
                                    result,
                                    dest.clone(),
                                    *on_conflict,
                                ))
                                .unwrap();
                        }
                        DirscanType::Mv(dest, on_conflict) => {
                            self.pubsub_tx
                                .send(PubSub::DoMv(
                                    self.entries.clone(),
                                    result,
                                    dest.clone(),
                                    *on_conflict,
                                ))
                                .unwrap();
                        }
                        DirscanType::Rm => {
                            self.pubsub_tx
                                .send(PubSub::DoRm(self.entries.clone(), result))
                                .unwrap();
                        }
                    }
                }
            }
            _ => (),
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect((chunk.width + 1) / 2, 9, chunk);

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
        let files = Paragraph::new(Span::raw(tilde_layout(
            &format!("Files: {}", self.files.separate_with_commas()),
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
        f.render_widget(files, upper_area[1]);
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
                Constraint::Length(self.btn_abort.width() as u16),
                Constraint::Length(1),
                Constraint::Length(self.btn_skip.width() as u16),
            ])
            .split(centered_rect(
                (self.btn_abort.width() + 1 + self.btn_skip.width()) as u16,
                1,
                &lower_block.inner(sections[1]),
            ));

        f.render_widget(lower_block, sections[1]);
        self.btn_abort.render(
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
    }
}
