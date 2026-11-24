//! The event loop: wait for a batch of events, let the [`App`] decide, execute the effects and
//! draw once per batch.

use std::io;
use std::sync::mpsc;
use std::time::Duration;

use jiff::ZonedDateTime;

use crate::app::{App, execute};
use crate::runtime::event::{Event, InputThread, next_batch};
use crate::runtime::terminal::TerminalGuard;

/// How long the loop waits at most if the start of the next day cannot be computed.
const FALLBACK_WAIT: Duration = Duration::from_hours(1);

/// Why the event loop stopped before the user quit.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimeError {
    /// The terminal cannot be switched to the interface.
    #[error("cannot set up the terminal")]
    Setup(#[source] io::Error),
    /// The thread reading terminal input cannot be started.
    #[error("cannot start reading terminal input")]
    SpawnInput(#[source] io::Error),
    /// Reading terminal input failed.
    #[error("cannot read terminal input")]
    Input(#[source] io::Error),
    /// The thread reading terminal input stopped without reporting an error.
    #[error("terminal input stopped unexpectedly")]
    InputClosed,
    /// Drawing the interface failed.
    #[error("cannot draw the interface")]
    Draw(#[source] io::Error),
}

/// Runs RuDo in the terminal until the user quits.
///
/// The terminal is restored when this function returns, also with an error or by a panic.
/// While the loop waits for input, it wakes up at the start of the next day to update
/// [`App::today`].
///
/// # Errors
///
/// Returns a [`RuntimeError`] if the terminal cannot be set up, read or drawn.
pub fn run(app: &mut App) -> Result<(), RuntimeError> {
    let mut terminal = TerminalGuard::init().map_err(RuntimeError::Setup)?;
    let (sender, receiver) = mpsc::channel();
    // Declared after the terminal so that it is dropped, and stops reading, first.
    let _input = InputThread::spawn(sender).map_err(RuntimeError::SpawnInput)?;
    loop {
        let now = ZonedDateTime::now();
        app.set_today(now.date());
        terminal.draw(app).map_err(RuntimeError::Draw)?;
        let batch = next_batch(&receiver, time_until_next_day(&now))
            .map_err(|_| RuntimeError::InputClosed)?;
        for event in batch {
            handle_event(app, event.map_err(RuntimeError::Input)?);
            if app.should_quit() {
                return Ok(());
            }
        }
    }
}

/// Lets `app` handle `event` and executes the effects it returns.
fn handle_event(app: &mut App, event: Event) {
    let effects = match event {
        Event::Key(chord) => app.handle_key(chord),
        Event::Paste(text) => app.handle_paste(&text),
        Event::Resize => Vec::new(),
    };
    for effect in effects {
        execute(app, effect);
    }
}

/// Returns how long it is from `now` until the next day starts in its time zone, taking
/// daylight saving time into account.
fn time_until_next_day(now: &ZonedDateTime) -> Duration {
    match now.tomorrow().and_then(|tomorrow| tomorrow.start_of_day()) {
        // The next day always starts after `now`, so the duration is positive.
        Ok(next_day) => now.duration_until(&next_day).unsigned_abs(),
        Err(error) => {
            tracing::warn!(%error, "cannot compute the start of the next day");
            FALLBACK_WAIT
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil::{Date, date};
    use jiff::tz::TimeZone;
    use rstest::rstest;

    use super::*;

    const HOUR: u64 = 60 * 60;

    /// POSIX time zone strings, which need no time zone database.
    const UTC: &str = "UTC0";
    /// Chile skips midnight at the start of daylight saving time.
    const CHILE: &str = "<-04>4<-03>,M9.1.6/24,M4.1.6/24";
    /// Central Europe repeats an hour at the end of daylight saving time.
    const CENTRAL_EUROPE: &str = "CET-1CEST,M3.5.0,M10.5.0/3";

    #[rstest]
    #[case::evening(UTC, date(2026, 3, 14), 23, HOUR)]
    #[case::midnight(UTC, date(2026, 3, 14), 0, 24 * HOUR)]
    #[case::skipped_midnight(CHILE, date(2026, 9, 5), 22, 2 * HOUR)]
    #[case::repeated_hour(CENTRAL_EUROPE, date(2026, 10, 25), 0, 25 * HOUR)]
    fn waits_until_the_next_day_starts(
        #[case] time_zone: &str,
        #[case] day: Date,
        #[case] hour: i8,
        #[case] expected_seconds: u64,
    ) {
        let now = day
            .at(hour, 0, 0, 0)
            .to_zoned_datetime(TimeZone::posix(time_zone).unwrap())
            .unwrap();
        let expected = Duration::from_secs(expected_seconds);

        let system_under_test = time_until_next_day(&now);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn last_representable_day_waits_the_fallback_time() {
        let now = date(9999, 12, 30)
            .at(12, 0, 0, 0)
            .to_zoned_datetime(TimeZone::UTC)
            .unwrap();

        let system_under_test = time_until_next_day(&now);

        assert_eq!(system_under_test, FALLBACK_WAIT);
    }
}
