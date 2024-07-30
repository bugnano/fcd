use std::{fmt, io, path::PathBuf, thread};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use ratatui::{prelude::*, widgets::*};
use termion::{event::*, input::TermRead};

use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

use crate::{
    fm::{
        cp_mv_rm::{database::OnConflict, dirscan::DirScanResult, dlg_cp_mv::DlgCpMvType},
        entry::{Entry, SortBy, SortOrder},
    },
    viewer::{dlg_goto::GotoType, dlg_hex_search::HexSearch, dlg_text_search::TextSearch},
};

#[derive(Debug, Clone)]
pub enum Events {
    Input(Event),
    Signal(i32),
}

#[derive(Debug, Clone)]
pub enum PubSub {
    // App-wide events
    Error(String),
    Warning(String, String),
    Info(String, String),
    CloseDialog,
    ComponentThreadEvent,
    Esc,
    Question(String, String, Box<PubSub>),

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
    ViewFile(PathBuf),
    Leader(Option<char>),
    SortFiles(SortBy, SortOrder),
    ToggleHidden,
    Reload,
    PromptFileFilter(String),
    PromptTagGlob,
    PromptUntagGlob,
    PromptMkdir,
    PromptRename(String, usize),
    MountArchive(PathBuf),
    Rm(PathBuf, Vec<Entry>),
    Cp(PathBuf, Vec<Entry>),
    Mv(PathBuf, Vec<Entry>),

    // Quick view events
    ToggleQuickView(Option<Entry>),

    // Command bar events
    CloseCommandBar,
    FileFilter(String),
    TagGlob(String),
    UntagGlob(String),
    Mkdir(String),
    Rename(String),

    // Dialog MountArchive events
    ArchiveMounted(PathBuf, PathBuf),
    ArchiveMountError(PathBuf, String),
    ArchiveMountCancel(PathBuf),

    // Dialog DirScan events
    DoRm(PathBuf, Vec<Entry>, DirScanResult),
    DoCp(PathBuf, Vec<Entry>, PathBuf, OnConflict, DirScanResult),
    DoMv(PathBuf, Vec<Entry>, PathBuf, OnConflict, DirScanResult),

    // Dialog CpMv events
    DoDirscan(PathBuf, Vec<Entry>, String, OnConflict, DlgCpMvType),
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

pub trait App {
    fn handle_events(&mut self, events_rx: &mut Receiver<Events>) -> Action;
    fn render(&mut self, f: &mut Frame);
}

impl fmt::Debug for dyn App + '_ {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dyn App")
    }
}

pub fn init_events() -> Result<(Sender<Events>, Receiver<Events>)> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let input_tx = tx.clone();
    let signals_tx = tx.clone();

    thread::spawn(move || {
        let stdin = io::stdin();
        for event in stdin.events().flatten() {
            if let Err(err) = input_tx.send(Events::Input(event)) {
                eprintln!("{}", err);
                return;
            }
        }
    });

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
    let area1 = Rect::new(r.x + 2, r.y + r.height, r.width, 1).intersection(f.size());
    let area2 =
        Rect::new(r.x + r.width, r.y + 1, 2, r.height.saturating_sub(1)).intersection(f.size());

    let block = Block::default().style(*s);

    f.render_widget(block.clone(), area1);
    f.render_widget(block, area2);
}
