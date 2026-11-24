//! Ownership of the terminal: [`TerminalGuard`] switches it to the interface and restores it
//! on every exit path.

use std::io;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use ratatui::layout::Size;

use crate::app::App;
use crate::ui;

/// The terminal in raw mode on the alternate screen, with bracketed paste enabled.
///
/// Dropping the guard restores the terminal, so it is restored before an error is reported
/// however the runtime ends. A panic restores it as well, through the hook installed by
/// [`ratatui::try_init`]. Mouse capture stays off so that text can still be selected.
#[derive(Debug)]
pub struct TerminalGuard {
    terminal: DefaultTerminal,
}

impl TerminalGuard {
    /// Switches the terminal to raw mode and the alternate screen and enables bracketed paste.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the terminal cannot be set up; whatever was already set up
    /// is restored.
    pub fn init() -> io::Result<Self> {
        let guard = Self {
            terminal: ratatui::try_init()?,
        };
        execute!(io::stdout(), EnableBracketedPaste)?;
        Ok(guard)
    }

    /// Returns the size of the terminal.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the terminal cannot report its size.
    pub fn size(&self) -> io::Result<Size> {
        self.terminal.size()
    }

    /// Restores the terminal, runs `program` with it, such as an external editor, and switches
    /// it back to the interface, which is drawn completely at the next draw.
    ///
    /// Stop reading terminal input before, so that no keystroke meant for `program` is taken.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the terminal cannot be restored or set up again; `program`
    /// has not run if restoring failed. Dropping the guard still restores the terminal.
    pub fn suspend<T>(&mut self, program: impl FnOnce() -> T) -> io::Result<T> {
        execute!(io::stdout(), DisableBracketedPaste)?;
        ratatui::try_restore()?;
        let result = program();
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        self.terminal.clear()?;
        Ok(result)
    }

    /// Draws `app` with [`ui::view`].
    ///
    /// # Errors
    ///
    /// Returns the I/O error if writing to the terminal fails.
    pub fn draw(&mut self, app: &App) -> io::Result<()> {
        self.terminal.draw(|frame| ui::view(app, frame))?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Err(error) = execute!(io::stdout(), DisableBracketedPaste) {
            tracing::error!(%error, "cannot disable bracketed paste");
        }
        if let Err(error) = ratatui::try_restore() {
            tracing::error!(%error, "cannot restore the terminal");
        }
    }
}
