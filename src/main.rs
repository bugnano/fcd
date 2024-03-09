use std::{
    io::{self, Write},
    panic,
    path::PathBuf,
};

use anyhow::{Context, Result};
use ratatui::prelude::*;
use termion::{input::MouseTerminal, raw::IntoRawMode, screen::IntoAlternateScreen};

use clap::{crate_name, ArgAction, Parser};

mod app;
mod button_bar;
mod component;
mod config;
mod dlg_error;
mod fm;
mod fnmatch;
mod shutil;
mod stat;
mod tilde_layout;
mod viewer;
mod widgets;

use crate::{
    app::{init_events, Action, App},
    config::load_config,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Print last working directory to specified file
    #[arg(short = 'P', long, value_name = "FILE")]
    printwd: Option<PathBuf>,

    /// Specify database file to use
    #[arg(short = 'D', long, value_name = "FILE")]
    database: Option<PathBuf>,

    /// Do not use database
    #[arg(short = 'n', long = "nodb", action = ArgAction::SetFalse)]
    use_db: bool,

    /// file viewer
    #[arg(short, long, value_name = "FILE")]
    view: Option<PathBuf>,

    /// set tab size for the file viewer
    #[arg(short, long, default_value_t = 0)]
    tabsize: u8,
}

fn initialize_panic_handler() {
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

    let config = load_config().context("failed to load config")?;

    #[cfg(debug_assertions)]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace"))
        .target(env_logger::Target::Pipe(Box::new(std::fs::File::create(
            format!("{}/{}.log", std::env::var("HOME")?, crate_name!()),
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

    let (_events_tx, mut events_rx) = init_events().context("initializing events failed")?;

    let mut app = match cli.view {
        Some(file) => Box::new(viewer::app::App::new(&config, &file, cli.tabsize)?) as Box<dyn App>,
        None => Box::new(fm::app::App::new(
            &config,
            cli.printwd.as_deref(),
            cli.database.as_deref(),
            cli.use_db,
            cli.tabsize,
        )?) as Box<dyn App>,
    };

    loop {
        terminal.draw(|f| app.render(f))?;

        match app.handle_events(&mut events_rx)? {
            Action::Continue => (),
            Action::NextLoop => (),
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
