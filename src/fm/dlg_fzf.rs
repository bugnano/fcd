use std::{
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::Instant,
};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use ratatui::{
    prelude::*,
    widgets::{
        block::{Position, Title},
        *,
    },
};
use termion::event::*;

use nucleo_matcher::{
    pattern::{CaseMatching, Normalization, Pattern},
    Config, Matcher, Utf32Str,
};
use pathdiff::diff_paths;
use thousands::Separable;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    fm::{
        app::natsort_key,
        entry::{Entry, HiddenFiles},
    },
    palette::Palette,
    tilde_layout::tilde_layout,
    widgets::input::Input,
};

const SPINNER: &[char] = &['-', '\\', '|', '/'];

#[derive(Debug)]
pub struct DlgFzf {
    palette: Rc<Palette>,
    pubsub_tx: Sender<PubSub>,
    cwd: PathBuf,
    entries: Vec<(PathBuf, bool)>,
    shown_entries: Vec<(PathBuf, bool)>,
    hidden_files: HiddenFiles,
    ev_tx: Sender<()>,
    info_rx: Receiver<Vec<(PathBuf, bool)>>,
    result_rx: Receiver<()>,
    i_spinner: Option<usize>,
    input: Input,
    cursor_position: usize,
    first_line: usize,
    rect: Rect,
}

impl DlgFzf {
    pub fn new(
        palette: &Rc<Palette>,
        pubsub_tx: Sender<PubSub>,
        cwd: &Path,
        file_list: &[Entry],
        hidden_files: HiddenFiles,
    ) -> DlgFzf {
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (info_tx, info_rx) = crossbeam_channel::unbounded();
        let (result_tx, result_rx) = crossbeam_channel::unbounded();

        let mut entries: Vec<(PathBuf, bool)> = file_list
            .iter()
            .filter(|entry| match hidden_files {
                HiddenFiles::Show => true,
                HiddenFiles::Hide => !entry.key.starts_with('.'),
            })
            .map(|entry| (entry.file.clone(), entry.stat.is_dir()))
            .collect();

        entries.sort_by_cached_key(|(file, _is_dir)| natsort_key(&file.to_string_lossy()));

        let mut dlg = DlgFzf {
            palette: Rc::clone(palette),
            pubsub_tx,
            cwd: PathBuf::from(cwd),
            entries,
            shown_entries: Vec::new(),
            hidden_files,
            ev_tx,
            info_rx,
            result_rx,
            i_spinner: Some(0),
            input: Input::new(&palette.dialog_input, "", 0),
            cursor_position: 0,
            first_line: 0,
            rect: Rect::default(),
        };

        dlg.filter_entries();

        dlg.fzf_thread(ev_rx, info_tx, result_tx);

        dlg
    }

    fn fzf_thread(
        &mut self,
        ev_rx: Receiver<()>,
        info_tx: Sender<Vec<(PathBuf, bool)>>,
        result_tx: Sender<()>,
    ) {
        let initial_entries = self.entries.clone();
        let hidden_files = self.hidden_files;

        let pubsub_tx = self.pubsub_tx.clone();

        thread::spawn(move || {
            fzf(
                &initial_entries,
                hidden_files,
                ev_rx,
                info_tx,
                pubsub_tx.clone(),
            );

            let _ = result_tx.send(());
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        });
    }

    fn filter_entries(&mut self) {
        if self.entries.is_empty() {
            return;
        }

        let filter = self.input.value();

        match filter.is_empty() {
            true => self.shown_entries = self.entries.clone(),
            false => {
                let mut matcher = Matcher::new(Config::DEFAULT.match_paths());

                let pattern = Pattern::parse(&filter, CaseMatching::Ignore, Normalization::Smart);

                let mut buf = Vec::new();

                let mut scores: Vec<(PathBuf, bool, usize, u32, usize)> = self
                    .entries
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (file, is_dir))| {
                        let file_name = diff_paths(file, &self.cwd)
                            .unwrap()
                            .to_string_lossy()
                            .to_string();

                        let utf32_str = Utf32Str::new(&file_name, &mut buf);
                        let len_utf32_str = utf32_str.len();

                        let score = pattern.score(utf32_str, &mut matcher);

                        score.map(|score| (file.clone(), *is_dir, i, score, len_utf32_str))
                    })
                    .collect();

                scores.sort_by(
                    |(_file1, _is_dir1, i1, score1, len1), (_file2, _is_dir2, i2, score2, len2)| {
                        score2.cmp(score1).then(len1.cmp(len2)).then(i1.cmp(i2))
                    },
                );

                self.shown_entries = scores
                    .iter()
                    .map(|(file, is_dir, _i, _score, _len)| (file.clone(), *is_dir))
                    .collect();

                self.cursor_position = self.clamp_cursor(self.cursor_position);
            }
        }
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.shown_entries.len().saturating_sub(1))
    }

    fn clamp_first_line(&mut self) {
        if (self.first_line + (self.rect.height as usize)) > self.shown_entries.len() {
            self.first_line = self
                .shown_entries
                .len()
                .saturating_sub(self.rect.height as usize);
        }
    }
}

impl Component for DlgFzf {
    fn handle_key(&mut self, key: &Key) -> bool {
        let mut key_handled = true;

        let old_filter = self.input.value();

        let input_handled = self.input.handle_key(key);

        let filter = self.input.value();

        if filter != old_filter {
            self.filter_entries();
        }

        if !input_handled {
            match key {
                Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                    let _ = self.ev_tx.send(());
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                }
                Key::Char('\n') | Key::Char(' ') => {
                    let _ = self.ev_tx.send(());
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                    if !self.shown_entries.is_empty() {
                        self.pubsub_tx
                            .send(PubSub::SelectFile(
                                self.shown_entries[self.cursor_position].clone(),
                            ))
                            .unwrap();
                    }
                }
                Key::Up | Key::Char('k') => {
                    self.cursor_position =
                        self.clamp_cursor(self.cursor_position.saturating_add(1));
                }
                Key::Down | Key::Char('j') => {
                    self.cursor_position =
                        self.clamp_cursor(self.cursor_position.saturating_sub(1));
                }
                Key::Home | Key::Char('g') => {
                    self.cursor_position = self.clamp_cursor(self.shown_entries.len());
                }
                Key::End | Key::Char('G') => {
                    self.cursor_position = 0;
                }
                Key::PageUp | Key::Ctrl('b') => {
                    let rect_height = (self.rect.height as usize).saturating_sub(1);

                    self.cursor_position =
                        self.clamp_cursor(self.cursor_position.saturating_add(rect_height));

                    self.first_line = self.first_line.saturating_add(rect_height);
                    self.clamp_first_line();
                }
                Key::PageDown | Key::Ctrl('f') => {
                    let rect_height = (self.rect.height as usize).saturating_sub(1);

                    self.cursor_position =
                        self.clamp_cursor(self.cursor_position.saturating_sub(rect_height));

                    self.first_line = self.first_line.saturating_sub(rect_height);
                    self.clamp_first_line();
                }
                Key::Ctrl('c') => key_handled = false,
                Key::Ctrl('l') => key_handled = false,
                Key::Ctrl('z') => key_handled = false,
                Key::Ctrl('o') => key_handled = false,
                _ => (),
            }
        }

        key_handled
    }

    fn handle_pubsub(&mut self, event: &PubSub) {
        #[allow(clippy::single_match)]
        match event {
            PubSub::ComponentThreadEvent => {
                if let Ok(info) = self.info_rx.try_recv() {
                    if !info.is_empty() {
                        self.entries.extend(info);

                        self.entries.sort_by_cached_key(|(file, _is_dir)| {
                            natsort_key(&file.to_string_lossy())
                        });

                        self.filter_entries();
                        self.i_spinner = self.i_spinner.map(|i| (i + 1) % SPINNER.len());
                    }
                }

                if let Ok(()) = self.result_rx.try_recv() {
                    self.i_spinner = None;
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

        f.render_widget(Clear, area);
        f.render_widget(Block::default().style(self.palette.dialog), area);
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
            .title(
                Title::from(Span::styled(
                    tilde_layout(" Find file ", sections[0].width as usize),
                    self.palette.dialog_title,
                ))
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let upper_area = upper_block.inner(sections[0]);

        let upper_height = (upper_area.height as usize).saturating_sub(1);

        self.rect = upper_area;
        self.clamp_first_line();

        if self.first_line > self.cursor_position {
            self.first_line = self.cursor_position;
        }

        if (self.cursor_position - self.first_line) > upper_height {
            self.first_line = self.cursor_position.saturating_sub(upper_height);
        }

        f.render_widget(upper_block, sections[0]);

        let items: Vec<ListItem> = self
            .shown_entries
            .iter()
            .skip(self.first_line)
            .take(upper_area.height.into())
            .map(|(file, _is_dir)| {
                ListItem::new::<String>(
                    diff_paths(file, &self.cwd)
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                )
            })
            .collect();

        let list = List::new(items)
            .direction(ListDirection::BottomToTop)
            .highlight_style(self.palette.dialog_focus);

        let mut state = ListState::default();
        state.select(Some(self.cursor_position - self.first_line));

        f.render_stateful_widget(list, upper_area, &mut state);

        // Lower section

        let lower_block = Block::default()
            .title(
                Title::from(Span::raw(tilde_layout(
                    &format!(
                        " {}{}/{} ",
                        match self.i_spinner {
                            Some(i) => format!("{} ", SPINNER[i]),
                            None => String::from(""),
                        },
                        self.shown_entries.len().separate_with_commas(),
                        self.entries.len().separate_with_commas()
                    ),
                    sections[0].width as usize,
                )))
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::ALL)
            .border_set(MIDDLE_BORDER_SET)
            .padding(Padding::horizontal(1))
            .style(self.palette.dialog);

        let lower_area = lower_block.inner(sections[1]);

        f.render_widget(lower_block, sections[1]);
        self.input.render(f, &lower_area, focus);
    }
}

fn fzf(
    initial_entries: &[(PathBuf, bool)],
    hidden_files: HiddenFiles,
    ev_rx: Receiver<()>,
    info_tx: Sender<Vec<(PathBuf, bool)>>,
    pubsub_tx: Sender<PubSub>,
) {
    let mut result = Vec::new();
    let mut last_write = Instant::now();

    for (file, is_dir) in initial_entries.iter() {
        if !is_dir {
            continue;
        }

        if let Ok((recursive_result, recursive_last_write)) =
            recursive_fzf(file, hidden_files, last_write, &ev_rx, &info_tx, &pubsub_tx)
        {
            result.extend(recursive_result);
            last_write = recursive_last_write;
        }

        if !ev_rx.is_empty() {
            let _ = ev_rx.try_recv();
            break;
        }
    }

    if !result.is_empty() {
        let _ = info_tx.send(result.clone());
        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
    }
}

fn recursive_fzf(
    cwd: &Path,
    hidden_files: HiddenFiles,
    old_last_write: Instant,
    ev_rx: &Receiver<()>,
    info_tx: &Sender<Vec<(PathBuf, bool)>>,
    pubsub_tx: &Sender<PubSub>,
) -> Result<(Vec<(PathBuf, bool)>, Instant)> {
    let mut result = Vec::new();

    let mut last_write = old_last_write;
    for entry in fs::read_dir(cwd)? {
        if !ev_rx.is_empty() {
            return Ok((result, last_write));
        }

        if last_write.elapsed().as_millis() >= 50 {
            last_write = Instant::now();
            let _ = info_tx.send(result.clone());
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);

            result.clear();
        }

        if let Ok(entry) = entry {
            let path = entry.path();

            if matches!(hidden_files, HiddenFiles::Hide)
                && matches!(path.file_name(), Some(file_name) if file_name.to_string_lossy().starts_with('.'))
            {
                continue;
            }

            match entry.file_type() {
                Ok(file_type) => match (file_type.is_dir(), file_type.is_symlink()) {
                    (true, _) => {
                        result.push((path.clone(), true));

                        if let Ok((recursive_result, recursive_last_write)) = recursive_fzf(
                            &path,
                            hidden_files,
                            last_write,
                            ev_rx,
                            info_tx,
                            pubsub_tx,
                        ) {
                            result.extend(recursive_result);
                            last_write = recursive_last_write;
                        }

                        if !ev_rx.is_empty() {
                            return Ok((result, last_write));
                        }
                    }
                    (_, true) => match fs::metadata(&path) {
                        Ok(metadata) if metadata.is_dir() => {
                            result.push((path.clone(), true));

                            if let Ok((recursive_result, recursive_last_write)) = recursive_fzf(
                                &path,
                                hidden_files,
                                last_write,
                                ev_rx,
                                info_tx,
                                pubsub_tx,
                            ) {
                                result.extend(recursive_result);
                                last_write = recursive_last_write;
                            }

                            if !ev_rx.is_empty() {
                                return Ok((result, last_write));
                            }
                        }
                        _ => result.push((path.clone(), false)),
                    },
                    _ => result.push((path.clone(), false)),
                },
                Err(_) => result.push((path.clone(), false)),
            }
        }
    }

    if last_write.elapsed().as_millis() >= 50 {
        last_write = Instant::now();
        let _ = info_tx.send(result.clone());
        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);

        result.clear();
    }

    Ok((result, last_write))
}
