//! Messages for the user in the bottom bar: [`StatusMessage`] with its [`Severity`], and when
//! it disappears again.
//!
//! The application never reads the clock: the runtime passes the current [`Instant`] to
//! [`App::tick`] before every draw and wakes up again at [`App::next_deadline`].

use std::time::{Duration, Instant};

use crate::app::state::App;

/// How long an [`Severity::Info`] message stays visible.
const INFO_DURATION: Duration = Duration::from_secs(3);

/// A message for the user, shown in the bottom bar instead of the key hints.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StatusMessage {
    text: String,
    severity: Severity,
}

impl StatusMessage {
    /// Creates a message that confirms something; it disappears after three seconds.
    #[must_use]
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: Severity::Info,
        }
    }

    /// Creates a message that tells what failed and why; it stays until the next key press,
    /// so that the user cannot miss it.
    #[must_use]
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            severity: Severity::Error,
        }
    }

    /// Returns the text of the message.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns how important the message is.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.severity
    }
}

/// How important a [`StatusMessage`] is; it decides the colour and when the message
/// disappears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
#[non_exhaustive]
pub enum Severity {
    /// Something worked; the message disappears after three seconds.
    Info,
    /// Something the user asked for failed; the message disappears at the next key press.
    Error,
}

/// The message currently shown, if any, and when it expires.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Status {
    message: Option<StatusMessage>,
    /// When the shown info message expires; `None` until the first tick after it was shown,
    /// and for error messages.
    expires_at: Option<Instant>,
}

impl Status {
    /// Shows `message`, replacing the one shown before.
    fn show(&mut self, message: StatusMessage) {
        self.message = Some(message);
        self.expires_at = None;
    }

    /// Starts the time of a newly shown info message at `now` and hides an expired one.
    fn tick(&mut self, now: Instant) {
        let Some(message) = &self.message else {
            return;
        };
        match message.severity {
            Severity::Info => match self.expires_at {
                None => self.expires_at = Some(now + INFO_DURATION),
                Some(expires_at) if now >= expires_at => *self = Self::default(),
                Some(_) => {}
            },
            Severity::Error => {}
        }
    }

    /// Hides an error message, which the user has seen once they press a key.
    pub(super) fn key_pressed(&mut self) {
        let Some(message) = &self.message else {
            return;
        };
        match message.severity {
            Severity::Info => {}
            Severity::Error => *self = Self::default(),
        }
    }
}

impl App {
    /// Returns the message shown in the bottom bar, if any.
    #[must_use]
    pub fn status_message(&self) -> Option<&StatusMessage> {
        self.status.message.as_ref()
    }

    /// Shows `message` in the bottom bar instead of the key hints, replacing the message
    /// shown before.
    ///
    /// An info message disappears three seconds after the next [`App::tick`], which the
    /// runtime performs right before it draws the message; an error message disappears at
    /// the next key press.
    pub fn show_message(&mut self, message: StatusMessage) {
        self.status.show(message);
    }

    /// Tells the application that the time is now `now`, so that timed messages expire; the
    /// runtime calls it before every draw.
    pub fn tick(&mut self, now: Instant) {
        self.status.tick(now);
    }

    /// Returns when the application must be ticked again even without input, or `None` if
    /// nothing is timed.
    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        self.status.expires_at
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    fn status_with(message: StatusMessage) -> Status {
        let mut status = Status::default();
        status.show(message);
        status
    }

    #[test]
    fn constructors_set_the_severity() {
        let expected = [Severity::Info, Severity::Error];

        let system_under_test = [
            StatusMessage::info("saved").severity(),
            StatusMessage::error("failed").severity(),
        ];

        assert_eq!(system_under_test, expected);
        assert_eq!(Severity::iter().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn showing_a_message_restarts_the_expiry() {
        let now = Instant::now();
        let mut status = status_with(StatusMessage::info("first"));
        status.tick(now);
        status.show(StatusMessage::info("second"));

        let system_under_test = status;

        assert_eq!(
            system_under_test.message,
            Some(StatusMessage::info("second"))
        );
        assert_eq!(system_under_test.expires_at, None);
    }

    #[test]
    fn first_tick_starts_the_time_of_an_info_message() {
        let now = Instant::now();
        let mut status = status_with(StatusMessage::info("saved"));
        status.tick(now);

        let system_under_test = status.expires_at;

        assert_eq!(system_under_test, Some(now + INFO_DURATION));
    }

    #[test]
    fn later_ticks_keep_the_expiry() {
        let now = Instant::now();
        let mut status = status_with(StatusMessage::info("saved"));
        status.tick(now);
        status.tick(now + Duration::from_secs(1));

        let system_under_test = status.expires_at;

        assert_eq!(system_under_test, Some(now + INFO_DURATION));
    }

    #[rstest]
    #[case::info(StatusMessage::info("saved"), Some(StatusMessage::info("saved")))]
    #[case::error(StatusMessage::error("failed"), None)]
    fn key_press_hides_only_errors(
        #[case] message: StatusMessage,
        #[case] expected: Option<StatusMessage>,
    ) {
        let mut status = status_with(message);
        status.key_pressed();

        let system_under_test = status.message;

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn empty_status_ignores_ticks_and_key_presses() {
        let mut status = Status::default();
        status.tick(Instant::now());
        status.key_pressed();

        let system_under_test = status;

        assert_eq!(system_under_test, Status::default());
    }
}
