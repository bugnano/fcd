use std::{
    fs::File,
    io::{self, BufRead, BufReader, Write},
    panic,
    time::Duration,
};

use anyhow::{Context, Result};
use ratatui::{prelude::*, widgets::*};
use termion::{
    input::MouseTerminal,
    raw::IntoRawMode,
    screen::IntoAlternateScreen,
};

mod events;
mod ui;

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

    let file = File::open("lorem.txt")?;
    let buffered = BufReader::new(file);
    let lines: Vec<String> = buffered.lines().map(|e| String::from(e.unwrap())).collect();
    let items: Vec<Row> = lines
        .iter()
        .enumerate()
        .map(|(i, e)| {
            Row::new(vec![
                Span::styled(
                    format!("{:width$}", i + 1, width = lines.len().to_string().len()).to_string(),
                    Style::default().fg(Color::White),
                ),
                e.to_string().into(),
            ])
        })
        .collect();
    let mut state = TableState::default();
    state.select(Some(0));

    let events_rx = events::init_events(Duration::from_millis(5000));

    loop {
        terminal.draw(|f| ui::render_app(f, &items, &mut state))?;
        if events::should_quit(&events_rx, &items, &mut state)? {
            break;
        }
    }

    Ok(())
}
