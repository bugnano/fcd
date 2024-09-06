use std::{fmt, io, path::PathBuf, thread};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use ratatui::{prelude::*, widgets::*};
use termion::{event::*, input::TermRead};

use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

use crate::{
    fm::{
        archive_mounter::ArchiveEntry,
        cp_mv_rm::database::{DBDirListEntry, DBFileEntry, DBJobEntry, DBJobOperation, OnConflict},
        entry::{Entry, HiddenFiles, SortBy, SortOrder},
    },
    viewer::{dlg_goto::GotoType, dlg_hex_search::HexSearch, dlg_text_search::TextSearch},
};

#[derive(Debug, Clone)]
pub enum Events {
    Input(Event),
    Signal(i32),
}

#[derive(Debug, Clone, Copy)]
pub enum Inputs {
    Stop,
    Start,
}

#[derive(Debug, Clone)]
pub enum PubSub {
    // App-wide events
    Error(String, Option<Box<PubSub>>),
    Warning(String, String),
    Info(String, String),
    CloseDialog,
    ComponentThreadEvent,
    Esc,
    Redraw,
    Question(String, String, Box<PubSub>),
    NextPendingJob,
    NextPendingArchive,

    // Button bar events
    ButtonLabels(Vec<String>),

    // File viewer events
    FileInfo(String, String, String),
    ToggleHex,

    // Hex viewer events
    FromHexOffset(u64),
    ToHexOffset(u64),
    HVStartSearch,
    HVSearchNext,
    HVSearchPrev,

    // Dialog goto events
    DlgGoto(GotoType),
    Goto(GotoType, String),

    // Dialog text search events
    DlgTextSearch(TextSearch),
    TextSearch(TextSearch),

    // Dialog hex search events
    DlgHexSearch(HexSearch),
    HexSearch(HexSearch),

    // File panel events
    SelectedEntry(Option<Entry>),
    ChangedDirectory(PathBuf),
    DirCreated(PathBuf),
    ViewFile(PathBuf, PathBuf),
    EditFile(PathBuf, PathBuf),
    Leader(Option<char>),
    SortFiles(SortBy, SortOrder),
    ToggleHidden,
    Reload,
    PromptFileFilter(String),
    PromptTagGlob,
    PromptUntagGlob,
    PromptMkdir,
    PromptRename(String, usize),
    PromptShell(PathBuf),
    MountArchive(PathBuf),
    Rm(PathBuf, Vec<Entry>),
    Cp(PathBuf, Vec<Entry>),
    Mv(PathBuf, Vec<Entry>),
    Fzf(PathBuf, Vec<Entry>, HiddenFiles),
    SelectFile(PathBuf),

    // Quick view events
    ToggleQuickView(Option<Entry>),

    // Command bar events
    CloseCommandBar,
    CommandBarError(String),
    FileFilter(String),
    TagGlob(String),
    UntagGlob(String),
    Mkdir(String),
    Rename(String),
    Shell(PathBuf, String),
    SaveReport(PathBuf, String),

    // Dialog MountArchive events
    ArchiveMounted(PathBuf, PathBuf),
    ArchiveMountError(PathBuf, String),
    ArchiveMountCancel(PathBuf),

    // Dialog DirScan events
    DoRm(DBJobEntry, Vec<DBFileEntry>, Vec<ArchiveEntry>),
    DoCp(DBJobEntry, Vec<DBFileEntry>, Vec<ArchiveEntry>),
    DoMv(DBJobEntry, Vec<DBFileEntry>, Vec<ArchiveEntry>),

    // Dialog CpMv events
    DoDirscan(PathBuf, Vec<Entry>, String, OnConflict, DBJobOperation),

    // Dialog Progress events
    JobCompleted(DBJobEntry, Vec<DBFileEntry>, Vec<DBDirListEntry>),

    // Dialog Report events
    PromptSaveReport(PathBuf, PathBuf),
    DoSaveReport(PathBuf),

    // Dialog PendingJob events
    MountArchivesForJob(DBJobEntry),
}

#[derive(Debug, Copy, Clone)]
pub enum Action {
    Continue,
    NextLoop,
    Redraw,
    Quit,
    CtrlC,
    SigTerm,
    CtrlZ,
    SigCont,
    CtrlO,
    ExitCtrlO,
}

pub const MIDDLE_BORDER_SET: symbols::border::Set = symbols::border::Set {
    top_left: symbols::line::NORMAL.vertical_right,
    top_right: symbols::line::NORMAL.vertical_left,
    ..symbols::border::PLAIN
};

pub trait App {
    fn handle_events(&mut self, events_rx: &mut Receiver<Events>) -> Action;
    fn render(&mut self, f: &mut Frame);
}

impl fmt::Debug for dyn App + '_ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dyn App")
    }
}

pub fn init_events(stop_inputs_rx: Receiver<Inputs>) -> Result<(Sender<Events>, Receiver<Events>)> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let events_tx = tx.clone();
    let signals_tx = tx.clone();

    start_inputs(events_tx, stop_inputs_rx);

    let mut signals = Signals::new([SIGWINCH, SIGINT, SIGTERM, SIGCONT])?;

    thread::spawn(move || {
        for signal in &mut signals {
            if let Err(err) = signals_tx.send(Events::Signal(signal)) {
                eprintln!("{}", err);
                return;
            }
        }
    });

    Ok((tx, rx))
}

pub fn start_inputs(events_tx: Sender<Events>, stop_inputs_rx: Receiver<Inputs>) {
    thread::spawn(move || {
        let stdin = io::stdin();

        for event in stdin.events() {
            let mut stop = false;
            let mut send_event = true;

            // The problem here is that stdin.events() is blocking, so given that
            // stop_inputs_rx is used for stopping the inputs before running a command,
            // it is possible that the command returns before the stdin.events() iterator
            // generated a new value.
            // So:
            // - If we receive Inputs::Stop, and immediately after Inputs::Start, it means
            //   that the command has completed before generating the value.
            //   In this case we want to send the event.
            // - If we recieve Inputs::Stop, but nothing else, it means that this is an input
            //   during the command execution.
            //   In this case we do *not* want to send the event.
            if let Ok(Inputs::Stop) = stop_inputs_rx.try_recv() {
                stop = true;
                send_event = false;

                if let Ok(Inputs::Start) = stop_inputs_rx.try_recv() {
                    send_event = true;
                }
            }

            if send_event {
                if let Err(err) = event.map(|event| events_tx.send(Events::Input(event))) {
                    eprintln!("{}", err);

                    #[cfg(debug_assertions)]
                    log::debug!("{:?}", err);
                }
            }

            if stop {
                return;
            }
        }
    });
}

pub fn centered_rect(width: u16, height: u16, r: &Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(height) + 1) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(*r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((r.width.saturating_sub(width) + 1) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(popup_layout[1])[1]
}

pub fn render_shadow(f: &mut Frame, r: &Rect, s: &Style) {
    let area1 = Rect::new(r.x + 2, r.y + r.height, r.width, 1).intersection(f.area());
    let area2 =
        Rect::new(r.x + r.width, r.y + 1, 2, r.height.saturating_sub(1)).intersection(f.area());

    let block = Block::default().style(*s);

    f.render_widget(block.clone(), area1);
    f.render_widget(block, area2);
}
