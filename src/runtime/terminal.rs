//! Ownership of the terminal: [`TerminalGuard`] switches it to the interface and restores it
//! on every exit path.

use std::io;
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, enable_raw_mode, supports_keyboard_enhancement,
};
use ratatui::layout::Size;

use crate::app::App;
use crate::ui;

/// Whether the keyboard enhancement flags are pushed onto the terminal right now.
///
/// The panic hook pops them as well, so every pop goes through [`pop_keyboard_flags`], which
/// pops only once.
static KEYBOARD_FLAGS_PUSHED: AtomicBool = AtomicBool::new(false);

/// Whether the terminal reports keys with the kitty keyboard protocol, which tells
/// `shift-enter` apart from `enter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyboardEnhancement {
    /// The terminal understands the protocol; its flags are pushed while RuDo draws.
    Enabled,
    /// The terminal does not understand the protocol, or did not answer the query; keys are
    /// read as legacy escape sequences.
    Unsupported,
}

impl KeyboardEnhancement {
    /// Chooses the keyboard mode from the answer of [`supports_keyboard_enhancement`].
    ///
    /// Windows always answers `false`: it reports `shift-enter` without the protocol.
    fn from_support(support: io::Result<bool>) -> Self {
        match support {
            Ok(true) => Self::Enabled,
            Ok(false) => Self::Unsupported,
            Err(error) => {
                tracing::warn!(%error, "cannot query the keyboard enhancement of the terminal");
                Self::Unsupported
            }
        }
    }
}

/// Asks the terminal to report keys that are ambiguous otherwise, such as `shift-enter`, as
/// escape codes. Key releases and all keys as escape codes are not requested.
fn push_keyboard_flags() -> io::Result<()> {
    execute!(
        io::stdout(),
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )?;
    KEYBOARD_FLAGS_PUSHED.store(true, Ordering::SeqCst);
    Ok(())
}

/// Pops the flags of [`push_keyboard_flags`] if they are pushed, so that the shell and an
/// external editor get the keys as before.
fn pop_keyboard_flags() -> io::Result<()> {
    if KEYBOARD_FLAGS_PUSHED.swap(false, Ordering::SeqCst) {
        execute!(io::stdout(), PopKeyboardEnhancementFlags)?;
    }
    Ok(())
}

/// Pops the keyboard flags when a panic occurs, before the hook of [`ratatui::try_init`]
/// leaves the alternate screen: the protocol keeps one stack of flags per screen, and the
/// guard is only dropped after the hook has run.
fn pop_keyboard_flags_on_panic() {
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if let Err(error) = pop_keyboard_flags() {
            tracing::error!(%error, "cannot pop the keyboard enhancement flags");
        }
        previous_hook(info);
    }));
}

/// The terminal in raw mode on the alternate screen, with bracketed paste enabled and, where
/// the terminal supports it, the kitty keyboard protocol.
///
/// Dropping the guard restores the terminal, so it is restored before an error is reported
/// however the runtime ends. A panic restores it as well, through the hook installed by
/// [`ratatui::try_init`]. Mouse capture stays off so that text can still be selected.
#[derive(Debug)]
pub struct TerminalGuard {
    terminal: DefaultTerminal,
    keyboard: KeyboardEnhancement,
}

impl TerminalGuard {
    /// Switches the terminal to raw mode and the alternate screen, enables bracketed paste
    /// and, if the terminal supports it, the kitty keyboard protocol.
    ///
    /// Call it before terminal input is read: asking the terminal for the protocol reads its
    /// answer from the input.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the terminal cannot be set up; whatever was already set up
    /// is restored.
    pub fn init() -> io::Result<Self> {
        let mut guard = Self {
            terminal: ratatui::try_init()?,
            keyboard: KeyboardEnhancement::Unsupported,
        };
        execute!(io::stdout(), EnableBracketedPaste)?;
        guard.keyboard = KeyboardEnhancement::from_support(supports_keyboard_enhancement());
        pop_keyboard_flags_on_panic();
        guard.push_keyboard_flags()?;
        Ok(guard)
    }

    /// Pushes the keyboard flags if the terminal supports them.
    fn push_keyboard_flags(&self) -> io::Result<()> {
        match self.keyboard {
            KeyboardEnhancement::Enabled => push_keyboard_flags(),
            KeyboardEnhancement::Unsupported => Ok(()),
        }
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
        pop_keyboard_flags()?;
        execute!(io::stdout(), DisableBracketedPaste)?;
        ratatui::try_restore()?;
        let result = program();
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        self.push_keyboard_flags()?;
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
        if let Err(error) = pop_keyboard_flags() {
            tracing::error!(%error, "cannot pop the keyboard enhancement flags");
        }
        if let Err(error) = execute!(io::stdout(), DisableBracketedPaste) {
            tracing::error!(%error, "cannot disable bracketed paste");
        }
        if let Err(error) = ratatui::try_restore() {
            tracing::error!(%error, "cannot restore the terminal");
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::supported(Ok(true), KeyboardEnhancement::Enabled)]
    #[case::unsupported(Ok(false), KeyboardEnhancement::Unsupported)]
    #[case::no_answer(
        Err(io::Error::from(io::ErrorKind::TimedOut)),
        KeyboardEnhancement::Unsupported
    )]
    fn keyboard_enhancement_is_enabled_only_where_supported(
        #[case] support: io::Result<bool>,
        #[case] expected: KeyboardEnhancement,
    ) {
        let system_under_test = KeyboardEnhancement::from_support(support);

        assert_eq!(system_under_test, expected);
    }
}
