use std::{
    fs::File,
    io::{self, Write},
    panic,
    time::Duration,
};

use anyhow::{Context, Result};
use ratatui::prelude::*;
use termion::{input::MouseTerminal, raw::IntoRawMode, screen::IntoAlternateScreen};

use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

use env_logger::{Builder, Env, Target};

mod app;
mod button_bar;
mod component;
mod events;
mod text_viewer;
mod top_bar;

use crate::{app::App, component::Component};

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
    Builder::from_env(Env::default().default_filter_or("trace"))
        .target(Target::Pipe(Box::new(File::create("fcv.log")?)))
        .init();

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

    let mut app = App::new()?;
    app.init()?;

    let signals = Signals::new(&[SIGWINCH])?;

    let events_rx = events::init_events(Duration::from_millis(5000), signals);

    loop {
        terminal.draw(|f| app.render(f, &f.size()))?;

        let should_quit = events::handle_events(&events_rx, &mut app)?;

        if should_quit {
            break;
        }
    }

    Ok(())
}
