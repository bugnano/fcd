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
        archive_mounter::{self, ArchiveMounterCommand},
        cp_mv_rm::{
            database::{DBFileEntry, DBJobEntry},
            dirscan::{dirscan, DirScanEvent, DirScanInfo, ReadMetadata},
        },
        entry::Entry,
    },
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgRmProgress {
    config: Rc<Config>,
    pubsub_tx: Sender<PubSub>,
    job: DBJobEntry,
    files: Vec<DBFileEntry>,
    btn_suspend: Button,
    btn_skip: Button,
    btn_abort: Button,
    btn_no_db: Button,
    current: String,
    num_files: usize,
    is_suspended: bool,
    focus_position: usize,
    archive_mounter_command_tx: Option<Sender<ArchiveMounterCommand>>,
}

impl DlgRmProgress {
    pub fn new(
        config: &Rc<Config>,
        pubsub_tx: Sender<PubSub>,
        job: &DBJobEntry,
        files: &[DBFileEntry],
        archive_mounter_command_tx: Option<Sender<ArchiveMounterCommand>>,
    ) -> DlgRmProgress {
        let mut dlg = DlgRmProgress {
            config: Rc::clone(config),
            pubsub_tx,
            job: job.clone(),
            files: Vec::from(files),
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
            btn_no_db: Button::new(
                "No DB",
                &Style::default().fg(config.dialog.fg).bg(config.dialog.bg),
                &Style::default()
                    .fg(config.dialog.focus_fg)
                    .bg(config.dialog.focus_bg),
                &Style::default()
                    .fg(config.dialog.title_fg)
                    .bg(config.dialog.bg),
            ),
            current: String::from(""),
            num_files: 0,
            is_suspended: false,
            focus_position: 0,
            archive_mounter_command_tx,
        };

        //dlg.dirscan_thread(cwd, ev_rx, info_tx, result_tx);

        dlg
    }

    // fn dirscan_thread(
    //     &mut self,
    //     cwd: &Path,
    //     ev_rx: Receiver<DirScanEvent>,
    //     info_tx: Sender<DirScanInfo>,
    //     result_tx: Sender<DirScanResult>,
    // ) {
    //     let (entries, read_metadata) = match &self.dirscan_type {
    //         DirscanType::Cp => todo!(),
    //         DirscanType::Mv => todo!(),
    //         DirscanType::Rm(entries) => (entries.clone(), ReadMetadata::No),
    //     };

    //     let pubsub_tx = self.pubsub_tx.clone();
    //     let cwd = PathBuf::from(cwd);

    //     thread::spawn(move || {
    //         let result = dirscan(
    //             &entries,
    //             &cwd,
    //             read_metadata,
    //             ev_rx,
    //             info_tx,
    //             pubsub_tx.clone(),
    //         );
    //         let _ = result_tx.send(result);
    //         let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
    //     });
    // }

    fn archive_path(&self, file: &Path) -> PathBuf {
        match &self.archive_mounter_command_tx {
            Some(command_tx) => archive_mounter::archive_path(command_tx, file),
            None => PathBuf::from(file),
        }
    }
}

impl Component for DlgRmProgress {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                // let _ = self.ev_tx.send(DirScanEvent::Abort);
            }
            Key::Char('\n') | Key::Char(' ') => match self.focus_position {
                0 => {
                    todo!();
                }
                1 => {
                    todo!();
                }
                2 => {
                    todo!();
                }
                3 => {
                    todo!();
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

    fn handle_pubsub(&mut self, event: &PubSub) {
        // match event {
        //     PubSub::ComponentThreadEvent => {
        //         if let Ok(info) = self.info_rx.try_recv() {
        //             self.current = self
        //                 .archive_path(&info.current)
        //                 .to_string_lossy()
        //                 .to_string();
        //             self.num_files = info.files;
        //             self.total_size = info.bytes;
        //         }
        //         if let Ok(result) = self.result_rx.try_recv() {
        //             todo!();
        //         }
        //     }
        //     _ => (),
        // }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let area = centered_rect((((chunk.width as usize) * 3) / 4) as u16, 11, chunk);

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
            .border_set(middle_border_set)
            .padding(Padding::horizontal(1))
            .style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            );

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

        let ratio = (self.num_files as f64) / (self.files.len() as f64);
        let gauge = Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(self.config.dialog.fg)
                    .bg(self.config.dialog.bg),
            )
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
        let time = Paragraph::new(Span::raw(tilde_layout(
            &format!("Time: {} ETA {}", "00:00:00", "00:00:00"),
            middle_area[2].width as usize,
        )));

        f.render_widget(middle_block, sections[1]);
        f.render_widget(gauge_left, gauge_area[0]);
        f.render_widget(gauge, gauge_area[1]);
        f.render_widget(gauge_right, gauge_area[2]);
        f.render_widget(num_files, middle_area[1]);
        f.render_widget(time, middle_area[2]);

        // Lower section

        self.btn_suspend.set_label(match self.is_suspended {
            true => "Continue",
            false => "Suspend ",
        });

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

        f.render_widget(lower_block, sections[2]);
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
        self.btn_no_db.render(
            f,
            &lower_area[6],
            match self.focus_position {
                3 => Focus::Focused,
                _ => Focus::Normal,
            },
        );
    }
}
