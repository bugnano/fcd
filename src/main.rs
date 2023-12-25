use std::{
    io::{self, Write},
    panic,
    path::PathBuf,
};

use anyhow::{Context, Result};
use ratatui::prelude::*;
use termion::{input::MouseTerminal, raw::IntoRawMode, screen::IntoAlternateScreen};

use clap::Parser;

mod app;
mod button_bar;
mod component;
mod config;
mod dlg_error;
mod dlg_goto;
mod dlg_text_search;
mod fnmatch;
mod text_viewer;
mod tilde_layout;
mod top_bar;
mod widgets;

use crate::app::{Action, App};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// set tab size
    #[arg(short, long, default_value_t = 0)]
    tabsize: u8,

    /// the file to view
    file: PathBuf,
}

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
    let cli = Cli::parse();

    #[cfg(debug_assertions)]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace"))
        .target(env_logger::Target::Pipe(Box::new(std::fs::File::create(
            "fcv.log",
        )?)))
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

    let mut app = App::new(&cli.file, cli.tabsize)?;

    loop {
        terminal.draw(|f| app.render(f))?;

        match app.handle_events()? {
            Action::Continue => (),
            Action::Redraw => {
                terminal.clear()?;
            }
            Action::Quit => break,
            Action::CtrlC => {
                write!(io::stdout(), "{}", termion::screen::ToMainScreen)?;
                println!("Ctrl+C");
                break;
            }
            Action::SigTerm => {
                write!(io::stdout(), "{}", termion::screen::ToMainScreen)?;
                println!("Kill");
                break;
            }
            Action::CtrlZ => {
                let mut output = io::stdout();
                write!(
                    output,
                    "{}{}",
                    termion::screen::ToMainScreen,
                    termion::cursor::Show
                )?;
                output.into_raw_mode()?.suspend_raw_mode()?;
                io::stdout().flush()?;

                println!("Ctrl+Z");

                unsafe {
                    libc::kill(libc::getpid(), libc::SIGSTOP);
                }
            }
            Action::SigCont => {
                let mut output = io::stdout();
                write!(output, "{}", termion::screen::ToAlternateScreen)?;
                output.into_raw_mode()?;
                io::stdout().flush()?;

                terminal.clear()?;
            }
        };
    }

    Ok(())
}
