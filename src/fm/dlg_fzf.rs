use std::{
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    thread,
    time::Instant,
};

use anyhow::Result;
use crossbeam_channel::{select, Receiver, Sender};
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
    Config, Matcher, Utf32String,
};
use pathdiff::diff_paths;
use thousands::Separable;

use crate::{
    app::{centered_rect, render_shadow, PubSub, MIDDLE_BORDER_SET},
    component::{Component, Focus},
    fm::entry::{Entry, HiddenFiles},
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
    num_entries: usize,
    shown_entries: Vec<Utf32String>,
    hidden_files: HiddenFiles,
    stop_fzf_tx: Sender<()>,
    fzf_info_rx: Receiver<Vec<(PathBuf, bool)>>,
    fzf_result_rx: Receiver<()>,
    stop_filter_tx: Sender<()>,
    filter_entries_tx: Sender<(String, Vec<(PathBuf, bool)>)>,
    filter_info_rx: Receiver<(Vec<Utf32String>, usize)>,
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
        let (stop_fzf_tx, stop_fzf_rx) = crossbeam_channel::unbounded();
        let (fzf_info_tx, fzf_info_rx) = crossbeam_channel::unbounded();
        let (fzf_result_tx, fzf_result_rx) = crossbeam_channel::unbounded();
        let (stop_filter_tx, stop_filter_rx) = crossbeam_channel::unbounded();
        let (filter_entries_tx, filter_entries_rx) = crossbeam_channel::unbounded();
        let (filter_info_tx, filter_info_rx) = crossbeam_channel::unbounded();

        let initial_entries: Vec<(PathBuf, bool)> = file_list
            .iter()
            .filter(|entry| match hidden_files {
                HiddenFiles::Show => true,
                HiddenFiles::Hide => !entry.file_name.starts_with('.'),
            })
            .map(|entry| (entry.file.clone(), entry.stat.is_dir()))
            .collect();

        let mut dlg = DlgFzf {
            palette: Rc::clone(palette),
            pubsub_tx,
            cwd: PathBuf::from(cwd),
            num_entries: 0,
            shown_entries: Vec::new(),
            hidden_files,
            stop_fzf_tx,
            fzf_info_rx,
            fzf_result_rx,
            stop_filter_tx,
            filter_entries_tx,
            filter_info_rx,
            i_spinner: Some(0),
            input: Input::new(&palette.dialog_input, "", 0),
            cursor_position: 0,
            first_line: 0,
            rect: Rect::default(),
        };

        dlg.fzf_thread(&initial_entries, stop_fzf_rx, fzf_info_tx, fzf_result_tx);

        dlg.filter_thread(stop_filter_rx, filter_entries_rx, filter_info_tx);

        dlg
    }

    fn fzf_thread(
        &mut self,
        initial_entries: &[(PathBuf, bool)],
        stop_fzf_rx: Receiver<()>,
        fzf_info_tx: Sender<Vec<(PathBuf, bool)>>,
        fzf_result_tx: Sender<()>,
    ) {
        let initial_entries = Vec::from(initial_entries);
        let hidden_files = self.hidden_files;

        let pubsub_tx = self.pubsub_tx.clone();

        thread::spawn(move || {
            fzf(
                &initial_entries,
                hidden_files,
                stop_fzf_rx,
                fzf_info_tx,
                pubsub_tx.clone(),
            );

            let _ = fzf_result_tx.send(());
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
        });
    }

    fn filter_thread(
        &mut self,
        stop_filter_rx: Receiver<()>,
        filter_entries_rx: Receiver<(String, Vec<(PathBuf, bool)>)>,
        filter_info_tx: Sender<(Vec<Utf32String>, usize)>,
    ) {
        let cwd = self.cwd.clone();

        let pubsub_tx = self.pubsub_tx.clone();

        thread::spawn(move || {
            let mut filter;
            let mut entries = Vec::new();

            let mut matcher = Matcher::new(Config::DEFAULT.match_paths());

            loop {
                select! {
                    recv(stop_filter_rx) -> _ => break,
                    recv(filter_entries_rx) -> res => if let Ok((new_filter, entry)) = res {
                        let mut needs_sorting = false;

                        filter = new_filter;

                        if !entry.is_empty() {
                            needs_sorting = true;

                            entries.extend(
                                entry
                                    .iter()
                                    .map(|(file, is_dir)| {
                                        let mut file_name =
                                            diff_paths(file, &cwd).unwrap().to_string_lossy().to_string();

                                        if *is_dir {
                                            file_name.push('/');
                                        }

                                        Utf32String::from(file_name)
                                    })
                            );
                        }


                        for (new_filter, entry) in filter_entries_rx.try_iter() {
                            filter = new_filter;

                            if !entry.is_empty() {
                                needs_sorting = true;
                                entries.extend(
                                    entry
                                        .iter()
                                        .map(|(file, is_dir)| {
                                            let mut file_name =
                                                diff_paths(file, &cwd).unwrap().to_string_lossy().to_string();

                                            if *is_dir {
                                                file_name.push('/');
                                            }

                                            Utf32String::from(file_name)
                                        })
                                );
                            }
                        }

                        if needs_sorting {
                            entries.sort();
                        }

                        match filter.is_empty() {
                            true => {
                                let _ = filter_info_tx.send((entries.clone(), entries.len()));
                            }
                            false => {
                                filter_entries(&entries, &filter, &mut matcher, &filter_info_tx, &stop_filter_rx);
                            }
                        }

                        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
                    }
                }
            }
        });
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
            self.filter_entries_tx.send((filter, Vec::new())).unwrap();
        }

        if !input_handled {
            match key {
                Key::Esc | Key::Char('q') | Key::Char('Q') | Key::F(10) | Key::Char('0') => {
                    let _ = self.stop_fzf_tx.send(());
                    let _ = self.stop_filter_tx.send(());
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                }
                Key::Char('\n') | Key::Char(' ') => {
                    let _ = self.stop_fzf_tx.send(());
                    let _ = self.stop_filter_tx.send(());
                    self.pubsub_tx.send(PubSub::CloseDialog).unwrap();
                    if !self.shown_entries.is_empty() {
                        let mut selected_file = self.cwd.clone();
                        selected_file.push(self.shown_entries[self.cursor_position].to_string());

                        self.pubsub_tx
                            .send(PubSub::SelectFile(selected_file))
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
                Key::Home | Key::CtrlHome | Key::Char('g') => {
                    self.cursor_position = self.clamp_cursor(self.shown_entries.len());
                }
                Key::End | Key::CtrlEnd | Key::Char('G') => {
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

    fn handle_mouse(&mut self, button: MouseButton, _mouse_position: layout::Position) {
        match button {
            MouseButton::WheelUp => {
                self.first_line = self.first_line.saturating_add(1);
                self.clamp_first_line();

                if self.first_line > self.cursor_position {
                    self.cursor_position = self.first_line;
                }
            }
            MouseButton::WheelDown => {
                self.first_line = self.first_line.saturating_sub(1);

                let rect_height = (self.rect.height as usize).saturating_sub(1);

                if (self.cursor_position - self.first_line) > rect_height {
                    self.cursor_position = self.cursor_position.saturating_sub(1);
                }
            }
            _ => {}
        }
    }

    fn handle_pubsub(&mut self, event: &PubSub) {
        #[allow(clippy::single_match)]
        match event {
            PubSub::ComponentThreadEvent => {
                if let Ok(entries) = self.fzf_info_rx.try_recv() {
                    if !entries.is_empty() {
                        self.i_spinner = self.i_spinner.map(|i| (i + 1) % SPINNER.len());
                        self.filter_entries_tx
                            .send((self.input.value(), entries))
                            .unwrap();
                    }
                }

                if let Ok((entries, num_entries)) = self.filter_info_rx.try_recv() {
                    self.shown_entries = entries;
                    self.num_entries = num_entries;
                    self.cursor_position = self.clamp_cursor(self.cursor_position);
                }

                if let Ok(()) = self.fzf_result_rx.try_recv() {
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
            .map(|file| ListItem::new(tilde_layout(&file.to_string(), upper_area.width as usize)))
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
                        self.num_entries.separate_with_commas()
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

fn filter_entries(
    entries: &[Utf32String],
    filter: &str,
    matcher: &mut Matcher,
    filter_info_tx: &Sender<(Vec<Utf32String>, usize)>,
    stop_filter_rx: &Receiver<()>,
) {
    let pattern = Pattern::parse(filter, CaseMatching::Ignore, Normalization::Smart);

    let mut scores: Vec<(Utf32String, usize, u32, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, file_name)| {
            let score = pattern.score(file_name.slice(..), matcher);

            score.map(|score| (file_name.clone(), i, score, file_name.len()))
        })
        .take_while(|_| stop_filter_rx.is_empty())
        .collect();

    if !stop_filter_rx.is_empty() {
        return;
    }

    scores.sort_by(|(_file1, i1, score1, len1), (_file2, i2, score2, len2)| {
        score2.cmp(score1).then(len1.cmp(len2)).then(i1.cmp(i2))
    });

    let _ = filter_info_tx.send((
        scores
            .iter()
            .map(|(file_name, _i, _score, _len)| file_name.clone())
            .collect(),
        entries.len(),
    ));
}

fn fzf(
    initial_entries: &[(PathBuf, bool)],
    hidden_files: HiddenFiles,
    stop_fzf_rx: Receiver<()>,
    fzf_info_tx: Sender<Vec<(PathBuf, bool)>>,
    pubsub_tx: Sender<PubSub>,
) {
    let mut entries = Vec::from(initial_entries);
    let mut last_write = Instant::now();

    let mut directories: Vec<PathBuf> = initial_entries
        .iter()
        .filter_map(|(file, is_dir)| match is_dir {
            true => Some(file.clone()),
            false => None,
        })
        .collect();

    let mut new_directories = Vec::new();

    while !directories.is_empty() {
        new_directories.clear();

        for file in directories.iter() {
            if !stop_fzf_rx.is_empty() {
                return;
            }

            if last_write.elapsed().as_millis() >= 50 {
                last_write = Instant::now();
                let _ = fzf_info_tx.send(entries.clone());
                let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);

                entries.clear();
            }

            if let Ok((recursive_last_write, recursive_directories)) = recursive_fzf(
                file,
                hidden_files,
                &mut entries,
                last_write,
                &stop_fzf_rx,
                &fzf_info_tx,
                &pubsub_tx,
            ) {
                last_write = recursive_last_write;
                new_directories.extend(recursive_directories);
            }
        }

        (directories, new_directories) = (new_directories, directories);
    }

    if !entries.is_empty() {
        let _ = fzf_info_tx.send(entries.clone());
        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
    }
}

fn recursive_fzf(
    cwd: &Path,
    hidden_files: HiddenFiles,
    entries: &mut Vec<(PathBuf, bool)>,
    old_last_write: Instant,
    stop_fzf_rx: &Receiver<()>,
    fzf_info_tx: &Sender<Vec<(PathBuf, bool)>>,
    pubsub_tx: &Sender<PubSub>,
) -> Result<(Instant, Vec<PathBuf>)> {
    let mut last_write = old_last_write;
    let mut directories = Vec::new();

    for entry in fs::read_dir(cwd)? {
        if !stop_fzf_rx.is_empty() {
            return Ok((last_write, directories));
        }

        if last_write.elapsed().as_millis() >= 50 {
            last_write = Instant::now();
            let _ = fzf_info_tx.send(entries.clone());
            let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);

            entries.clear();
        }

        if let Ok(entry) = entry {
            let path = entry.path();

            if matches!(hidden_files, HiddenFiles::Hide)
                && matches!(path.file_name(), Some(file_name) if file_name.to_string_lossy().starts_with('.'))
            {
                continue;
            }

            match entry.file_type() {
                Ok(file_type) => match file_type.is_dir() {
                    true => {
                        entries.push((path.clone(), true));
                        directories.push(path.clone());
                    }
                    _ => entries.push((path.clone(), false)),
                },
                Err(_) => entries.push((path.clone(), false)),
            }
        }
    }

    if last_write.elapsed().as_millis() >= 50 {
        last_write = Instant::now();
        let _ = fzf_info_tx.send(entries.clone());
        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);

        entries.clear();
    }

    Ok((last_write, directories))
}
