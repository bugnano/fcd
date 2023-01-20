use std::{io, thread, time::Duration};
use tui::{
	backend::TermionBackend,
	widgets::{Widget, Block, Borders},
	layout::{Layout, Constraint, Direction},
	Terminal
};
use termion::{
	event::Key,
	input::{MouseTerminal, TermRead},
	raw::IntoRawMode,
	screen::IntoAlternateScreen,
};

fn main() -> Result<(), io::Error> {
	// setup terminal
	let stdout = io::stdout().into_raw_mode()?.into_alternate_screen()?;
	let stdout = MouseTerminal::from(stdout);
	let backend = TermionBackend::new(stdout);
	let mut terminal = Terminal::new(backend)?;

	terminal.draw(|f| {
		let size = f.size();
		let block = Block::default()
			.title("Block")
			.borders(Borders::ALL);
		f.render_widget(block, size);
	})?;

	thread::sleep(Duration::from_millis(5000));

	Ok(())
}

