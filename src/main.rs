use std::{
    io::{self, Write},
    panic,
    time::Duration,
};

use anyhow::{Context, Result};
use ratatui::prelude::*;
use termion::{input::MouseTerminal, raw::IntoRawMode, screen::IntoAlternateScreen};

use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

mod component;
mod events;
mod text_viewer;
mod ui;

use crate::component::Component;
use crate::text_viewer::TextViewer;

pub fn initialize_panic_handler() {
    let panic_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic| {
        let panic_cleanup = || -> Result<()> {
            let mut output = io::stdout();
            write!(
                output,
                "{}{}{}",
                termion::clear::All,
                termion::screen::ToMainScreen,
                termion::cursor::Show
            )?;
            output.into_raw_mode()?.suspend_raw_mode()?;
            io::stdout().flush()?;
            Ok(())
        };
        panic_cleanup().expect("failed to clean up for panic");
        panic_hook(panic);
    }));
}

fn main() -> Result<()> {
    initialize_panic_handler();

    let stdout = MouseTerminal::from(
        io::stdout()
            .into_raw_mode()
            .context("failed to enable raw mode")?
            .into_alternate_screen()
            .context("unable to enter alternate screen")?,
    );

    // Terminal<TermionBackend<MouseTerminal<AlternateScreen<RawTerminal<Stdout>>>>>
    let mut terminal =
        Terminal::new(TermionBackend::new(stdout)).context("creating terminal failed")?;

    let mut component = TextViewer::new()?;
    component.init()?;

    let signals = Signals::new(&[SIGWINCH])?;
    let handle = signals.handle();

    let events_rx = events::init_events(Duration::from_millis(5000), signals);

    loop {
        let should_quit = events::handle_events(&events_rx, &mut component)?;

        if should_quit {
            break;
        }

        terminal.draw(|f| ui::render_app(f, &mut component))?;
    }

    handle.close();

    Ok(())
}
