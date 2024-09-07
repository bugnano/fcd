use std::{
    cell::RefCell,
    env,
    io::{self, Write},
    panic,
    path::PathBuf,
    rc::Rc,
};

use anyhow::{Context, Result};
use ratatui::prelude::*;
use termion::{input::MouseTerminal, raw::IntoRawMode, screen::IntoAlternateScreen};

use clap::{crate_name, ArgAction, Parser};
use path_clean::PathClean;

mod app;
mod button_bar;
mod component;
mod config;
mod dlg_error;
mod fm;
mod fnmatch;
mod palette;
mod shutil;
mod stat;
mod template;
mod tilde_layout;
mod viewer;
mod widgets;

use crate::{
    app::{init_events, Action, App},
    config::load_config,
    fm::bookmarks::Bookmarks,
    palette::{get_monochrome_palette, get_palette},
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Print last working directory to specified file
    #[arg(short = 'P', long, value_name = "FILE")]
    printwd: Option<PathBuf>,

    /// Specify database file to use
    #[arg(short = 'D', long = "database", value_name = "FILE")]
    db_file: Option<PathBuf>,

    /// Do not use database
    #[arg(short = 'n', long = "nodb", action = ArgAction::SetFalse)]
    use_db: bool,

    /// Requests to run in black and white
    #[arg(short = 'b', long = "nocolor")]
    monochrome: bool,

    /// Use vertical panel layout
    #[arg(long)]
    vertical: bool,

    /// file viewer
    #[arg(short, long, value_name = "FILE")]
    view: Option<PathBuf>,

    /// set tab size for the file viewer
    #[arg(short, long, default_value_t = 0)]
    tabsize: u8,
}

fn initialize_panic_handler() -> Result<()> {
    let raw_output = io::stdout().into_raw_mode()?;

    raw_output.suspend_raw_mode()?;

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
            output.flush()?;

            raw_output.suspend_raw_mode()?;

            Ok(())
        };
        panic_cleanup().expect("failed to clean up for panic");
        panic_hook(panic);
    }));

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut config = load_config().context("failed to load config")?;

    let palette = Rc::new(match cli.monochrome {
        true => get_monochrome_palette(),
        false => get_palette(&config),
    });

    if cli.monochrome {
        config.options.use_shadows = false;
    }

    let config = Rc::new(config);

    #[cfg(debug_assertions)]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace"))
        .target(env_logger::Target::Pipe(Box::new(std::fs::File::create(
            format!("{}/{}.log", std::env::var("HOME")?, crate_name!()),
        )?)))
        .init();

    initialize_panic_handler().context("failed to initialize panic handler")?;

    let raw_output = Rc::new(
        io::stdout()
            .into_raw_mode()
            .context("failed to activate raw mode")?,
    );

    raw_output
        .suspend_raw_mode()
        .context("failed to suspend raw mode")?;

    let output = MouseTerminal::from(
        io::stdout()
            .into_raw_mode()
            .context("failed to activate raw mode")?
            .into_alternate_screen()
            .context("unable to enter alternate screen")?,
    );

    // Terminal<TermionBackend<MouseTerminal<AlternateScreen<RawTerminal<Stdout>>>>>
    let mut terminal =
        Terminal::new(TermionBackend::new(output)).context("creating terminal failed")?;

    let (stop_inputs_tx, stop_inputs_rx) = crossbeam_channel::unbounded();

    let (events_tx, mut events_rx) =
        init_events(stop_inputs_rx.clone()).context("initializing events failed")?;

    let tab_size = match cli.tabsize {
        0 => config.viewer.tab_size,
        tabsize => tabsize,
    };

    let mut app = match cli.view {
        Some(file) => Box::new(viewer::app::App::new(
            &config, &palette, &events_tx, &file, tab_size,
        )?) as Box<dyn App>,
        None => {
            let bookmark_path = xdg::BaseDirectories::with_prefix(crate_name!())
                .ok()
                .and_then(|xdg_dirs| xdg_dirs.place_config_file("bookmarks").ok());

            let bookmarks = Rc::new(RefCell::new(Bookmarks::new(bookmark_path.as_deref())));

            let h_bookmark = bookmarks.borrow().get('h');
            if let (None, Some(home_dir)) = (h_bookmark, home::home_dir()) {
                bookmarks.borrow_mut().insert('h', &home_dir);
            }

            let initial_path = match PathBuf::from(env::var("PWD").unwrap_or(String::from("."))) {
                cwd if cwd.is_absolute() => cwd.clean(),
                _ => env::current_dir().context("failed to get current working directory")?,
            };

            let db_file = cli.use_db.then_some(true).and_then(|_| {
                cli.db_file.or_else(|| {
                    xdg::BaseDirectories::with_prefix(crate_name!())
                        .ok()
                        .and_then(|xdg_dirs| {
                            xdg_dirs
                                .place_state_file(&format!("{}.db", crate_name!()))
                                .ok()
                        })
                })
            });

            Box::new(fm::app::App::new(
                &config,
                &palette,
                &events_tx,
                &bookmarks,
                &raw_output,
                &stop_inputs_tx,
                &stop_inputs_rx,
                &initial_path,
                cli.printwd.as_deref(),
                db_file.as_deref(),
                cli.vertical,
                tab_size,
            )?) as Box<dyn App>
        }
    };

    let mut ctrl_o = false;

    terminal.clear()?;
    loop {
        if !ctrl_o {
            terminal.draw(|f| app.render(f))?;
        }

        match app.handle_events(&mut events_rx) {
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

                match ctrl_o {
                    true => {
                        write!(
                            output,
                            "{}{}",
                            termion::cursor::Restore,
                            termion::clear::UntilNewline,
                        )?;
                    }
                    false => {
                        write!(
                            output,
                            "{}{}",
                            termion::screen::ToMainScreen,
                            termion::cursor::Show
                        )?;
                    }
                }

                output.flush()?;

                raw_output.suspend_raw_mode()?;

                println!("Ctrl+Z");

                unsafe {
                    libc::kill(libc::getpid(), libc::SIGSTOP);
                }
            }
            Action::SigCont => {
                raw_output.activate_raw_mode()?;

                let mut output = io::stdout();

                match ctrl_o {
                    true => {
                        write!(
                            output,
                            "{}Press ENTER to continue...",
                            termion::cursor::Save
                        )?;
                        output.flush()?;
                    }
                    false => {
                        write!(output, "{}", termion::screen::ToAlternateScreen)?;
                        output.flush()?;

                        terminal.clear()?;
                    }
                }
            }
            Action::CtrlO => {
                ctrl_o = true;

                let mut output = io::stdout();

                write!(
                    output,
                    "{}{}{}Press ENTER to continue...",
                    termion::screen::ToMainScreen,
                    termion::cursor::Show,
                    termion::cursor::Save
                )?;
                output.flush()?;
            }
            Action::ExitCtrlO => {
                ctrl_o = false;

                let mut output = io::stdout();

                write!(
                    output,
                    "{}{}{}",
                    termion::cursor::Restore,
                    termion::clear::UntilNewline,
                    termion::screen::ToAlternateScreen
                )?;
                output.flush()?;

                terminal.clear()?;
            }
        };
    }

    Ok(())
}
