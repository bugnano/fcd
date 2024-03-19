use std::{
    cell::RefCell,
    fs::{self, read_dir},
    path::{Path, PathBuf},
    rc::Rc,
    thread,
};

use anyhow::{anyhow, bail, Result};
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
    app::PubSub,
    component::{Component, Focus},
    config::Config,
    fm::{
        app::human_readable_size,
        bookmarks::Bookmarks,
        entry::{
            count_directories, filter_file_list, get_file_list, sort_by_function,
            style_from_palette, Entry, HiddenFiles, SortBy, SortOrder,
        },
        panel::{Panel, PanelComponent},
    },
    shutil::disk_usage,
    tilde_layout::tilde_layout,
};

#[derive(Debug, Clone)]
enum ComponentPubSub {
    FileList(Vec<Entry>),
}

#[derive(Debug)]
pub struct FilePanel {
    config: Rc<Config>,
    bookmarks: Rc<RefCell<Bookmarks>>,
    pubsub_tx: Sender<PubSub>,
    rect: Rect,
    component_pubsub_tx: Sender<ComponentPubSub>,
    component_pubsub_rx: Receiver<ComponentPubSub>,
    file_list_tx: Sender<PathBuf>,
    file_list_rx: Receiver<PathBuf>,
    cwd: PathBuf,
    old_cwd: PathBuf,
    leader: Option<char>,
    free: u64,
    is_loading: bool,
    file_list: Vec<Entry>,
    shown_file_list: Vec<Entry>,
    tagged_files: Vec<Entry>,
    cursor_position: usize,
    first_line: usize,
    hidden_files: HiddenFiles,
    file_filter: String,
    sort_method: SortBy,
    sort_order: SortOrder,
}

impl FilePanel {
    pub fn new(
        config: &Rc<Config>,
        bookmarks: &Rc<RefCell<Bookmarks>>,
        pubsub_tx: Sender<PubSub>,
        initial_path: &Path,
    ) -> Result<FilePanel> {
        let (component_pubsub_tx, component_pubsub_rx) = crossbeam_channel::unbounded();
        let (file_list_tx, file_list_rx) = crossbeam_channel::unbounded();

        let mut panel = FilePanel {
            config: Rc::clone(config),
            bookmarks: Rc::clone(bookmarks),
            pubsub_tx,
            rect: Rect::default(),
            component_pubsub_tx,
            component_pubsub_rx,
            file_list_tx,
            file_list_rx,
            cwd: PathBuf::new(),
            old_cwd: PathBuf::new(),
            leader: None,
            free: 0,
            is_loading: false,
            file_list: Vec::new(),
            shown_file_list: Vec::new(),
            tagged_files: Vec::new(),
            cursor_position: 0,
            first_line: 0,
            hidden_files: HiddenFiles::Hide,
            file_filter: String::from(""),
            sort_method: SortBy::Name,
            sort_order: SortOrder::Normal,
        };

        panel.file_list_thread();
        panel.chdir(initial_path)?;
        panel.old_cwd = panel.cwd.clone();

        Ok(panel)
    }

    fn handle_component_pubsub(&mut self) -> Result<()> {
        if let Ok(event) = self.component_pubsub_rx.try_recv() {
            match event {
                ComponentPubSub::FileList(file_list) => {
                    self.is_loading = false;

                    self.file_list = file_list;

                    self.shown_file_list =
                        filter_file_list(&self.file_list, self.hidden_files, &self.file_filter);

                    self.shown_file_list
                        .sort_by(|a, b| sort_by_function(self.sort_method)(a, b, self.sort_order));

                    self.tagged_files
                        .retain(|entry| self.file_list.contains(entry));

                    if !self.shown_file_list.is_empty() {
                        self.pubsub_tx
                            .send(PubSub::UpdateQuickView(self.get_selected_file()))
                            .unwrap();
                    }
                }
            }
        }

        Ok(())
    }

    fn file_list_thread(&mut self) {
        let file_list_rx = self.file_list_rx.clone();
        let component_pubsub_tx = self.component_pubsub_tx.clone();
        let pubsub_tx = self.pubsub_tx.clone();

        thread::spawn(move || {
            loop {
                let cwd = match file_list_rx.is_empty() {
                    // Block this thread until we recevie something
                    true => match file_list_rx.recv() {
                        Ok(cwd) => cwd,

                        // When the main thread exits, the channel returns an error
                        Err(_) => return,
                    },

                    // We're only interested in the latest message in the queue
                    false => file_list_rx.try_iter().last().unwrap(),
                };

                // Step 1: Get the current file list without counting the directories
                let file_list = get_file_list(&cwd, Some(file_list_rx.clone())).unwrap_or_default();

                // Send the current result only if there are no newer file list requests in the queue,
                // otherwise discard the current result
                if file_list_rx.is_empty() {
                    // First send the component event
                    let _ = component_pubsub_tx.send(ComponentPubSub::FileList(file_list.clone()));

                    // Then notify the app that there is an component event
                    let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);

                    // Step 2: Get the current file list counting the directories
                    let file_list = count_directories(&file_list, Some(file_list_rx.clone()));

                    // Send the current result only if there are no newer file list requests in the queue,
                    // otherwise discard the current result
                    if file_list_rx.is_empty() {
                        // First send the component event
                        let _ = component_pubsub_tx.send(ComponentPubSub::FileList(file_list));

                        // Then notify the app that there is an component event
                        let _ = pubsub_tx.send(PubSub::ComponentThreadEvent);
                    }
                }
            }
        });
    }

    fn chdir(&mut self, cwd: &Path) -> Result<()> {
        let new_cwd = cwd
            .ancestors()
            .find(|d| read_dir(d).is_ok())
            .ok_or_else(|| anyhow!("failed to change directory"))?
            .to_path_buf();

        if new_cwd != self.cwd {
            self.old_cwd = self.cwd.clone();
            self.cwd = new_cwd;

            self.file_filter.clear();
            self.tagged_files.clear();
            self.cursor_position = 0;
            self.first_line = 0;

            self.load_file_list()?;
        }

        Ok(())
    }

    fn chdir_old_cwd(&mut self) -> Result<()> {
        let old_cwd = self.old_cwd.clone();

        self.chdir(&old_cwd)
    }

    fn load_file_list(&mut self) -> Result<()> {
        self.free = disk_usage(&self.cwd)?.free;

        self.is_loading = true;
        self.file_list_tx.send(self.cwd.clone())?;

        Ok(())
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.shown_file_list.len().saturating_sub(1))
    }

    fn clamp_first_line(&mut self) {
        if (self.first_line + (self.rect.height as usize)) > self.shown_file_list.len() {
            self.first_line = self
                .shown_file_list
                .len()
                .saturating_sub(self.rect.height as usize);
        }
    }

    fn handle_up(&mut self) {
        let old_cursor_position = self.cursor_position;

        self.cursor_position = self.clamp_cursor(self.cursor_position.saturating_sub(1));

        if self.cursor_position != old_cursor_position {
            self.pubsub_tx
                .send(PubSub::UpdateQuickView(self.get_selected_file()))
                .unwrap();
        }
    }

    fn handle_down(&mut self) {
        let old_cursor_position = self.cursor_position;

        self.cursor_position = self.clamp_cursor(self.cursor_position.saturating_add(1));

        if self.cursor_position != old_cursor_position {
            self.pubsub_tx
                .send(PubSub::UpdateQuickView(self.get_selected_file()))
                .unwrap();
        }
    }
}

impl Component for FilePanel {
    fn handle_key(&mut self, key: &Key) -> Result<bool> {
        let mut key_handled = true;

        if let Some(c) = self.leader {
            match (c, key) {
                ('`', Key::Char('\'')) | ('`', Key::Char('`')) => {
                    self.chdir_old_cwd()?;
                }
                ('`', Key::Char(c)) => {
                    let bookmark =
                        self.bookmarks
                            .borrow()
                            .get(*c)
                            .and_then(|cwd| match read_dir(&cwd) {
                                Ok(_) => Some(cwd),
                                Err(_) => None,
                            });

                    if let Some(cwd) = bookmark {
                        self.chdir(&cwd)?;
                    }
                }
                ('m', Key::Char(c)) => self.bookmarks.borrow_mut().insert(*c, &self.cwd),
                _ => key_handled = false,
            }

            // When pressing a key after a leader, the leader is automatically reset
            self.leader = None;
            self.pubsub_tx.send(PubSub::Leader(self.leader)).unwrap();
        } else {
            match key {
                Key::Char(c) if *c == '\'' || *c == '`' => {
                    self.leader = Some('`');
                    self.pubsub_tx.send(PubSub::Leader(Some(*c))).unwrap();
                }
                Key::Char('m') => {
                    // TODO: Cannot bookmark inside an archive
                    self.leader = Some('m');
                    self.pubsub_tx.send(PubSub::Leader(self.leader)).unwrap();
                }
                Key::Left | Key::Char('h') => {
                    let cwd = self.cwd.clone();

                    if let Some(new_cwd) = cwd.parent() {
                        self.chdir(new_cwd)?
                    }
                }
                Key::Right | Key::Char('\n') | Key::Char('l') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = self.shown_file_list[self.cursor_position].clone();

                        if entry.stat.is_dir() {
                            self.chdir(&entry.file)?;
                        } else if let Some(path) = &entry.link_target {
                            let _ = path
                                .try_exists()
                                .map_err(anyhow::Error::new)
                                .and_then(|exists| {
                                    if !exists {
                                        bail!("!exists")
                                    }

                                    Ok(fs::metadata(path)?)
                                })
                                .and_then(|metadata| {
                                    match metadata.is_dir() {
                                        true => {
                                            // Change directory only if we can change to that exact directory
                                            read_dir(path)?;

                                            self.chdir(path)?
                                        }
                                        false => {
                                            let parent = path.parent().ok_or_else(|| {
                                                anyhow!("failed to read link target parent")
                                            })?;

                                            // Change directory only if we can change to that exact directory
                                            read_dir(parent)?;

                                            self.chdir(parent)?;

                                            if self.cwd == parent {
                                                let old_cursor_position = self.cursor_position;
                                                let diff_cursor_first = self
                                                    .cursor_position
                                                    .saturating_sub(self.first_line);

                                                self.cursor_position = self.clamp_cursor(
                                                    self.shown_file_list
                                                        .iter()
                                                        .position(|entry| &entry.file == path)
                                                        .unwrap_or(old_cursor_position),
                                                );

                                                if self.cursor_position != old_cursor_position {
                                                    self.first_line = self
                                                        .cursor_position
                                                        .saturating_sub(diff_cursor_first);
                                                    self.clamp_first_line();

                                                    self.pubsub_tx
                                                        .send(PubSub::UpdateQuickView(
                                                            self.get_selected_file(),
                                                        ))
                                                        .unwrap();
                                                }
                                            }
                                        }
                                    }

                                    Ok(())
                                });
                        }
                        // TODO: Handle archives and regular files
                    }
                }
                Key::Up | Key::Char('k') => {
                    self.handle_up();
                }
                Key::Down | Key::Char('j') => {
                    self.handle_down();
                }
                Key::Home | Key::Char('g') => {
                    let old_cursor_position = self.cursor_position;

                    self.cursor_position = 0;

                    if self.cursor_position != old_cursor_position {
                        self.pubsub_tx
                            .send(PubSub::UpdateQuickView(self.get_selected_file()))
                            .unwrap();
                    }
                }
                Key::End | Key::Char('G') => {
                    let old_cursor_position = self.cursor_position;

                    self.cursor_position = self.clamp_cursor(self.shown_file_list.len());

                    if self.cursor_position != old_cursor_position {
                        self.pubsub_tx
                            .send(PubSub::UpdateQuickView(self.get_selected_file()))
                            .unwrap();
                    }
                }
                Key::PageUp | Key::Ctrl('b') => {
                    let rect_height = (self.rect.height as usize).saturating_sub(1);
                    let old_cursor_position = self.cursor_position;

                    self.cursor_position =
                        self.clamp_cursor(self.cursor_position.saturating_sub(rect_height));

                    self.first_line = self.first_line.saturating_sub(rect_height);
                    self.clamp_first_line();

                    if self.cursor_position != old_cursor_position {
                        self.pubsub_tx
                            .send(PubSub::UpdateQuickView(self.get_selected_file()))
                            .unwrap();
                    }
                }
                Key::PageDown | Key::Ctrl('f') => {
                    let rect_height = (self.rect.height as usize).saturating_sub(1);
                    let old_cursor_position = self.cursor_position;

                    self.cursor_position =
                        self.clamp_cursor(self.cursor_position.saturating_add(rect_height));

                    self.first_line = self.first_line.saturating_add(rect_height);
                    self.clamp_first_line();

                    if self.cursor_position != old_cursor_position {
                        self.pubsub_tx
                            .send(PubSub::UpdateQuickView(self.get_selected_file()))
                            .unwrap();
                    }
                }
                Key::Char('v') | Key::F(3) => {
                    if !self.shown_file_list.is_empty() {
                        let entry = self.shown_file_list[self.cursor_position].clone();

                        match entry.stat.is_dir() {
                            true => self.chdir(&entry.file)?,
                            false => self.pubsub_tx.send(PubSub::ViewFile(entry.file)).unwrap(),
                        }
                    }
                }
                Key::Insert | Key::Char(' ') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = &self.shown_file_list[self.cursor_position];

                        if let Some(i) = self.tagged_files.iter().position(|x| x == entry) {
                            self.tagged_files.swap_remove(i);
                        } else {
                            self.tagged_files.push(entry.clone());
                        }
                    }

                    self.handle_down();
                }
                Key::Char('t') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = &self.shown_file_list[self.cursor_position];

                        if !self.tagged_files.contains(entry) {
                            self.tagged_files.push(entry.clone());
                        }
                    }

                    self.handle_down();
                }
                Key::Char('u') => {
                    if !self.shown_file_list.is_empty() {
                        let entry = &self.shown_file_list[self.cursor_position];

                        if let Some(i) = self.tagged_files.iter().position(|x| x == entry) {
                            self.tagged_files.swap_remove(i);
                        }
                    }

                    self.handle_down();
                }
                Key::Char('*') => {
                    if !self.shown_file_list.is_empty() {
                        for entry in self.shown_file_list.iter() {
                            if let Some(i) = self.tagged_files.iter().position(|x| x == entry) {
                                self.tagged_files.swap_remove(i);
                            } else {
                                self.tagged_files.push(entry.clone());
                            }
                        }
                    }
                }
                Key::Char('T') => {
                    if !self.shown_file_list.is_empty() {
                        for entry in self.shown_file_list.iter() {
                            if !self.tagged_files.contains(entry) {
                                self.tagged_files.push(entry.clone());
                            }
                        }
                    }
                }
                Key::Char('U') => {
                    if !self.shown_file_list.is_empty() {
                        for entry in self.shown_file_list.iter() {
                            if let Some(i) = self.tagged_files.iter().position(|x| x == entry) {
                                self.tagged_files.swap_remove(i);
                            }
                        }
                    }
                }
                _ => key_handled = false,
            }
        }

        Ok(key_handled)
    }

    fn handle_pubsub(&mut self, event: &PubSub) -> Result<()> {
        match event {
            PubSub::ComponentThreadEvent => self.handle_component_pubsub()?,
            _ => (),
        }

        Ok(())
    }

    fn render(&mut self, f: &mut Frame, chunk: &Rect, focus: Focus) {
        let middle_border_set = symbols::border::Set {
            top_left: symbols::line::NORMAL.vertical_right,
            top_right: symbols::line::NORMAL.vertical_left,
            ..symbols::border::PLAIN
        };

        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(*chunk);

        let upper_block = Block::default()
            .title(
                Title::from(Line::from(vec![
                    Span::raw(symbols::line::NORMAL.horizontal),
                    Span::styled(
                        tilde_layout(
                            &format!(" {} ", self.cwd.to_string_lossy()),
                            chunk.width.saturating_sub(4).into(),
                        ),
                        match focus {
                            Focus::Focused => Style::default()
                                .fg(self.config.panel.reverse_fg)
                                .bg(self.config.panel.reverse_bg),
                            _ => Style::default()
                                .fg(self.config.panel.fg)
                                .bg(self.config.panel.bg),
                        },
                    ),
                    Span::raw(symbols::line::NORMAL.horizontal),
                ]))
                .position(Position::Top)
                .alignment(Alignment::Left),
            )
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .style(
                Style::default()
                    .fg(self.config.panel.fg)
                    .bg(self.config.panel.bg),
            );

        let upper_inner = upper_block.inner(sections[0]);
        let upper_height = (upper_inner.height as usize).saturating_sub(1);

        self.rect = upper_inner;
        self.clamp_first_line();

        if self.first_line > self.cursor_position {
            self.first_line = self.cursor_position;
        }

        if (self.cursor_position - self.first_line) > upper_height {
            self.first_line = self.cursor_position.saturating_sub(upper_height);
        }

        f.render_widget(upper_block, sections[0]);

        match self.is_loading {
            true => {
                f.render_widget(
                    Block::default()
                        .title("Loading...")
                        .style(Style::default().fg(self.config.panel.fg)),
                    upper_inner,
                );
            }
            false => {
                let items: Vec<ListItem> = self
                    .shown_file_list
                    .iter()
                    .skip(self.first_line)
                    .take(upper_inner.height.into())
                    .enumerate()
                    .map(|(i, entry)| {
                        let filename_max_width = (upper_inner.width as usize)
                            .saturating_sub(entry.shown_size.width())
                            .saturating_sub(9);

                        let is_selected = self.first_line + i == self.cursor_position;

                        let filename = if is_selected && !matches!(focus, Focus::Focused) {
                            tilde_layout(
                                &std::iter::once('\u{2192}')
                                    .chain(entry.label.chars().skip(1))
                                    .collect::<String>(),
                                filename_max_width,
                            )
                        } else {
                            tilde_layout(&entry.label, filename_max_width)
                        };

                        let filename_width = filename.width();

                        // The reason why I add {:width$} whitespaces after the
                        // filename instead of putting the filename directly
                        // inside {:width$} is because the {:width$} formatting
                        // has a bug with some 0-width Unicode characters
                        Span::styled(
                            format!(
                                "{}{:width$} {} {}",
                                &filename,
                                "",
                                &entry.shown_size,
                                &entry.shown_mtime,
                                width = filename_max_width.saturating_sub(filename_width)
                            ),
                            match (self.tagged_files.contains(entry), is_selected) {
                                (true, true) => Style::default().fg(self.config.ui.markselect_fg),
                                (true, false) => Style::default().fg(self.config.ui.marked_fg),
                                _ => style_from_palette(&self.config, entry.palette),
                            },
                        )
                        .into()
                    })
                    .collect();

                let items = List::new(items).highlight_style(match focus {
                    Focus::Focused => Style::default()
                        .fg(self.config.ui.selected_fg)
                        .bg(self.config.ui.selected_bg),
                    _ => Style::default(),
                });

                let mut state = ListState::default();
                state.select(Some(self.cursor_position - self.first_line));

                f.render_stateful_widget(items, upper_inner, &mut state);
            }
        }

        let lower_block = Block::default()
            .title(
                Title::from(Line::from(vec![
                    Span::raw(symbols::line::NORMAL.horizontal),
                    Span::raw(tilde_layout(
                        &format!(" Free: {} ", human_readable_size(self.free)),
                        chunk.width.saturating_sub(4).into(),
                    )),
                    Span::raw(symbols::line::NORMAL.horizontal),
                ]))
                .position(Position::Bottom)
                .alignment(Alignment::Right),
            )
            .title(
                Title::from(match self.tagged_files.is_empty() {
                    true => Span::raw(symbols::line::NORMAL.horizontal),
                    false => Span::styled(
                        tilde_layout(
                            &format!(
                                " {} in {} file{} ",
                                human_readable_size(
                                    self.tagged_files
                                        .iter()
                                        .map(|entry| {
                                            match entry.lstat.is_dir() {
                                                true => 0,
                                                false => entry.lstat.len(),
                                            }
                                        })
                                        .sum()
                                ),
                                self.tagged_files.len(),
                                if self.tagged_files.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                }
                            ),
                            chunk.width.saturating_sub(4).into(),
                        ),
                        Style::default().fg(self.config.ui.marked_fg),
                    ),
                })
                .position(Position::Top)
                .alignment(Alignment::Center),
            )
            .borders(Borders::ALL)
            .border_set(middle_border_set)
            .style(
                Style::default()
                    .fg(self.config.panel.fg)
                    .bg(self.config.panel.bg),
            );

        let lower_inner = lower_block.inner(sections[1]);

        f.render_widget(lower_block, sections[1]);

        if (!self.is_loading) && (!self.shown_file_list.is_empty()) {
            f.render_widget(
                Block::new()
                    .title(tilde_layout(
                        &self.shown_file_list[self.cursor_position].details,
                        lower_inner.width.into(),
                    ))
                    .style(
                        Style::default()
                            .fg(self.config.panel.fg)
                            .bg(self.config.panel.bg),
                    ),
                lower_inner,
            );
        }
    }
}

impl Panel for FilePanel {
    fn get_selected_file(&self) -> Option<PathBuf> {
        match self.shown_file_list.is_empty() {
            true => None,
            false => Some(self.shown_file_list[self.cursor_position].file.clone()),
        }
    }

    fn get_cwd(&self) -> Option<PathBuf> {
        Some(self.cwd.clone())
    }
}

impl PanelComponent for FilePanel {}
