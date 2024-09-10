use std::io::{self, Write};

use anyhow::Result;

/// A sequence of escape codes to enable terminal mouse support.
pub const ENTER_MOUSE_SEQUENCE: &str = "\x1b[?1000h\x1b[?1002h\x1b[?1015h\x1b[?1006h";

/// A sequence of escape codes to disable terminal mouse support.
pub const EXIT_MOUSE_SEQUENCE: &str = "\x1b[?1006l\x1b[?1015l\x1b[?1002l\x1b[?1000l";

#[derive(Debug, Clone)]
pub struct TerminalRestorer {}

impl TerminalRestorer {
    pub fn new() -> Result<TerminalRestorer> {
        let mut output = io::stdout();

        write!(
            output,
            "{}{}",
            ENTER_MOUSE_SEQUENCE,
            termion::screen::ToAlternateScreen
        )
        .and_then(|_| output.flush())?;

        Ok(TerminalRestorer {})
    }
}

impl Drop for TerminalRestorer {
    fn drop(&mut self) {
        let mut output = io::stdout();

        let _ = write!(
            output,
            "{}{}",
            EXIT_MOUSE_SEQUENCE,
            termion::screen::ToMainScreen
        )
        .and_then(|_| output.flush());
    }
}
