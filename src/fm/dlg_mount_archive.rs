use std::{
    path::{Path, PathBuf},
    rc::Rc,
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use termion::event::*;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    fm::archive_mounter::{self, ArchiveMounterCommand},
    palette::Palette,
    tilde_layout::tilde_layout,
    widgets::button::Button,
};

#[derive(Debug)]
pub struct DlgMountArchive {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    archive: PathBuf,
    title: String,
    result_rx: Receiver<Result<PathBuf>>,
    cancel_tx: Sender<()>,
    btn_cancel: Button,
    btn_cancel_rect: Rect,
}

impl DlgMountArchive {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        archive: &Path,
        command_tx: &Sender<ArchiveMounterCommand>,
    ) -> DlgMountArchive {
        // All the archive_mounter queries must be done before mount_archive,
        // otherwise they will return only after the archive has been mounted,
        // thus making useless this dialog, that allows the archive mounting process
        // to be canceled
        let title = archive_mounter::get_exe_name(command_tx);
        let shown_archive = archive_mounter::archive_path(command_tx, archive);

        let (mount_archive_rx, cancel_tx) =
            archive_mounter::mount_archive(command_tx, &shown_archive);

        let (result_tx, result_rx) = crossbeam_channel::unbounded();
        let ps_tx = pubsub_tx.clone();

        thread::spawn(move || {
            let result = mount_archive_rx
                .recv()
                .unwrap_or(Err(anyhow!("receive error")));

            let _ = result_tx.send(result);
            let _ = ps_tx.send(PubSub::ComponentThreadEvent);
        });

        DlgMountArchive {
            palette: Rc::clone(palette),
            pubsub_tx,
            archive: PathBuf::from(archive),
            title,
            result_rx,
            cancel_tx,
            btn_cancel: Button::new(
                "Cancel",
                &palette.dialog,
                &palette.dialog_focus,
                &palette.dialog_title,
            ),
            btn_cancel_rect: Rect::default(),
        }
    }

    fn on_cancel(&mut self) {
        self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

        let _ = self.cancel_tx.send(());

        self.pubsub_tx
            .send(PubSub::ArchiveMountCancel(self.archive.clone()))
            .unwrap();
    }
}

impl Component for DlgMountArchive {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        match key {
            Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                self.on_cancel();
            }
            Key::Char('\n') | Key::Char(' ') => self.on_cancel(),
            Key::Ctrl('c') => key_handled = false,
            Key::Ctrl('l') => key_handled = false,
            Key::Ctrl('z') => key_handled = false,
            Key::Ctrl('o') => key_handled = false,
            _ => (),
        }

        key_handled
    }

    fn handle_mouse(&mut self, button: MouseButton, mouse_position: layout::Position) {
        if self.btn_cancel_rect.contains(mouse_position) {
            if let MouseButton::Left = button {
                self.on_cancel();
            }
        }
    }

    fn handle_pubsub(&mut self, event: &PubSub) {
        #[allow(clippy::single_match)]
        match event {
            PubSub::ComponentThreadEvent => {
                if let Ok(result) = self.result_rx.try_recv() {
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();

                    match result {
                        Ok(temp_dir) => {
                            self.pubsub_tx
                                .send(PubSub::ArchiveMounted(self.archive.clone(), temp_dir))
                                .unwrap();
                        }
                        Err(e) => {
                            self.pubsub_tx
                                .send(PubSub::ArchiveMountError(
                                    self.archive.clone(),
                                    e.to_string(),
                                ))
                                .unwrap();
                        }
                    }
                }
            }
            _ => (),
        }
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, _focus: Focus) {
        let message = "Opening archive...";

        let area = centered_rect((message.width() + 6) as u16, 7, chunk);

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.palette.dialog), area);
        if let Some(shadow) = self.palette.shadow {
            render_shadow(f, &area, &shadow);
        }

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Length(3)])
            .split(centered_rect(
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
                &area,
            ));

        // Upper section

        let upper_block = Block::default()
            .title(
                Title::from(Span::styled(
                    tilde_layout(&format!(" {} ", self.title), sections[0].width as usize),
                    self.palette.dialog_title,
                ))
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let upper_area = upper_block.inner(sections[0]);

        let message = Paragraph::new(Span::raw(tilde_layout(message, upper_area.width as usize)));

        f.render_widget(upper_block, sections[0]);
        f.render_widget(message, upper_area);

        // Lower section

        let lower_block = Block::default()
            .borders(Borders::ALL)
            .border_set(MIDDLE_BORDER_SET)
            .style(self.palette.dialog);

        let lower_area = centered_rect(
            self.btn_cancel.width() as u16,
            1,
            &lower_block.inner(sections[1]),
        );

        self.btn_cancel_rect = lower_area;

        f.render_widget(lower_block, sections[1]);
        self.btn_cancel
            .render(f, &self.btn_cancel_rect, Focus::Focused);
    }
}
