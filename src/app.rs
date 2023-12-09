use std::{io, panic, path::Path, rc::Rc, thread, time::Duration};

use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};
use ratatui::{prelude::*, widgets::*};
use termion::{event::*, input::TermRead, terminal_size};

use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

use crate::{
    button_bar::ButtonBar, component::Component, config::load_config, config::Config,
    dlg_goto::DlgGoto, text_viewer::TextViewer, top_bar::TopBar,
};

pub enum Events {
    Input(Event),
    Tick,
    Signal(i32),
    Highlight(Vec<Vec<(Color, String)>>),
}

pub enum Action {
    Continue,
    Redraw,
    Quit,
    CtrlC,
    Term,
    CtrlZ,
    SigCont,
}

#[derive(Debug)]
pub struct App {
    config: Config,
    events_rx: Receiver<Events>,
    top_bar: TopBar,
    text_viewer: TextViewer,
    button_bar: ButtonBar,
    dialog: Option<Box<dyn Component>>,
}

impl App {
    pub fn new(filename: &Path, tabsize: u8) -> Result<App> {
        let config = load_config()?;

        let (events_tx, events_rx) = init_events()?;

        let (w, h) = terminal_size().unwrap();
        let chunks = get_chunks(&Rect::new(0, 0, w, h));

        Ok(App {
            config: config,
            events_rx,
            top_bar: TopBar::new(&config, filename)?,
            text_viewer: TextViewer::new(
                &config,
                &chunks[1],
                filename,
                tabsize,
                events_tx.clone(),
            )?,
            button_bar: ButtonBar::new(&config)?,
            dialog: None,
        })
    }

    pub fn handle_events(&mut self) -> Result<Action> {
        let events = self.events_rx.recv()?;

        let event_handled = match &mut self.dialog {
            Some(dlg) => dlg.handle_events(&events)?,
            None => self.text_viewer.handle_events(&events)?,
        };

        if !event_handled {
            match events {
                Events::Input(event) => match event {
                    Event::Key(key) => match key {
                        Key::Char('q')
                        | Key::Char('Q')
                        | Key::Char('v')
                        | Key::F(3)
                        | Key::F(10) => return Ok(Action::Quit),
                        Key::Char('p') => panic!("at the disco"),
                        Key::Ctrl('c') => return Ok(Action::CtrlC),
                        Key::Ctrl('l') => return Ok(Action::Redraw),
                        Key::Ctrl('z') => return Ok(Action::CtrlZ),
                        Key::Char(':') | Key::F(5) => {
                            // TODO: Maybe check that there are no other open dialogs
                            self.dialog =
                                Some(Box::new(DlgGoto::new(&self.config, "Line number: ")?));
                        }
                        _ => log::debug!("{:?}", key),
                    },
                    Event::Mouse(_mouse) => (),
                    Event::Unsupported(_) => (),
                },
                Events::Tick => (),
                Events::Signal(signal) => match signal {
                    SIGWINCH => {
                        let (w, h) = terminal_size().unwrap();
                        let chunks = get_chunks(&Rect::new(0, 0, w, h));

                        self.text_viewer.resize(&chunks[1]);
                    }
                    SIGINT => return Ok(Action::CtrlC),
                    SIGTERM => return Ok(Action::Term),
                    SIGCONT => return Ok(Action::SigCont),
                    _ => unreachable!(),
                },
                Events::Highlight(_) => (),
            }
        }

        Ok(Action::Continue)
    }

    pub fn render(&mut self, f: &mut Frame) {
        let chunks = get_chunks(&f.size());

        self.top_bar.render(f, &chunks[0]);
        self.text_viewer.render(f, &chunks[1]);
        self.button_bar.render(f, &chunks[2]);

        if let Some(dlg) = &mut self.dialog {
            dlg.render(f, &chunks[1]);
        }
    }
}

fn init_events() -> Result<(Sender<Events>, Receiver<Events>)> {
    let (s, r) = unbounded();
    let input_tx = s.clone();
    let tick_tx = s.clone();
    let signals_tx = s.clone();

    thread::spawn(move || {
        let stdin = io::stdin();
        for event in stdin.events().flatten() {
            if let Err(err) = input_tx.send(Events::Input(event)) {
                eprintln!("{}", err);
                return;
            }
        }
    });

    let tick_rate = Duration::from_millis(5000);

    thread::spawn(move || loop {
        if let Err(err) = tick_tx.send(Events::Tick) {
            eprintln!("{}", err);
            break;
        }
        thread::sleep(tick_rate);
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

    Ok((s, r))
}

fn get_chunks(rect: &Rect) -> Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(*rect)
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
