use std::{
    fs::File,
    io::{self, Write},
    panic,
};

use anyhow::{Context, Result};
use ratatui::prelude::*;
use termion::{input::MouseTerminal, raw::IntoRawMode, screen::IntoAlternateScreen};

use env_logger::{Builder, Env, Target};

mod app;
mod button_bar;
mod component;
mod text_viewer;
mod top_bar;

use crate::app::{Action, App};

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

    loop {
        terminal.draw(|f| app.render(f))?;

        match app.handle_events()? {
            Action::Continue => (),
            Action::Quit => break,
            Action::CtrlC => {
                write!(io::stdout(), "{}", termion::screen::ToMainScreen)?;
                println!("Ctrl+C");
                break;
            }
            Action::Term => {
                write!(io::stdout(), "{}", termion::screen::ToMainScreen)?;
                println!("Kill");
                break;
            }
        };
    }

    Ok(())
}
