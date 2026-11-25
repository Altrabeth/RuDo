//! The event loop: wait for a batch of events, let the [`App`] decide, execute the effects and
//! draw once per batch.

use std::env::{self, VarError};
use std::io;
use std::num::NonZeroUsize;
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use jiff::ZonedDateTime;

use crate::app::editor::{EditorCommand, EditorError, choose_editor};
use crate::app::{App, Effect, execute};
use crate::runtime::event::{Event, InputThread, next_batch};
use crate::runtime::terminal::TerminalGuard;
use crate::storage::watch::WorkspaceWatcher;
use crate::storage::workspace::ListPath;

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
    /// The terminal cannot be handed to the editor or taken back from it.
    #[error("cannot hand the terminal to the editor")]
    Suspend(#[source] io::Error),
}

/// Runs RuDo in the terminal until the user quits.
///
/// The terminal is restored when this function returns, also with an error or by a panic.
/// Before every draw the loop passes the current time to [`App::tick`]; while it waits for
/// input, it wakes up at [`App::next_deadline`] and at the start of the next day to update
/// [`App::today`]. For [`Effect::OpenInEditor`] it hands the terminal to the editor. Changes
/// to the workspace by other programs arrive as [`Event::FsChanged`]; if the workspace cannot
/// be watched, RuDo runs without live reload and says so.
///
/// # Errors
///
/// Returns a [`RuntimeError`] if the terminal cannot be set up, read, drawn or handed to the
/// editor.
pub fn run(app: &mut App) -> Result<(), RuntimeError> {
    let mut terminal = TerminalGuard::init().map_err(RuntimeError::Setup)?;
    app.resize(terminal.size().map_err(RuntimeError::Setup)?);
    let (sender, receiver) = mpsc::channel();
    let _watcher = watch(app, sender.clone());
    // Declared after the terminal so that it is dropped, and stops reading, first.
    let mut input = InputThread::spawn(sender).map_err(RuntimeError::SpawnInput)?;
    loop {
        let instant = Instant::now();
        let now = ZonedDateTime::now();
        app.tick(instant);
        app.set_today(now.date());
        terminal.draw(app).map_err(RuntimeError::Draw)?;
        let timeout = time_until_wake_up(instant, app.next_deadline(), time_until_next_day(&now));
        let batch = next_batch(&receiver, timeout).map_err(|_| RuntimeError::InputClosed)?;
        for event in batch {
            for effect in handle_event(app, event.map_err(RuntimeError::Input)?) {
                input = perform(app, effect, &mut terminal, input)?;
            }
            if app.should_quit() {
                return Ok(());
            }
        }
    }
}

/// Starts watching the workspace of `app`, sending its changes to `sender`, or reports on
/// `app` that live reload is disabled.
fn watch(app: &mut App, sender: Sender<io::Result<Event>>) -> Option<WorkspaceWatcher> {
    let started = WorkspaceWatcher::start(app.workspace().root(), move |changes| {
        if sender.send(Ok(Event::FsChanged(changes))).is_err() {
            tracing::debug!("event loop stopped, dropping file system changes");
        }
    });
    started.inspect_err(|error| app.watch_failed(error)).ok()
}

/// Lets `app` handle `event` and returns the effects to perform.
fn handle_event(app: &mut App, event: Event) -> Vec<Effect> {
    match event {
        Event::Key(chord) => app.handle_key(chord),
        Event::Paste(text) => app.handle_paste(&text),
        Event::FsChanged(changes) => app.on_fs_changed(&changes),
        Event::Resize(size) => {
            app.resize(size);
            Vec::new()
        }
    }
}

/// Performs `effect` and returns the thread that reads terminal input from now on: effects
/// that need the terminal are performed here, all others by [`execute`].
fn perform(
    app: &mut App,
    effect: Effect,
    terminal: &mut TerminalGuard,
    input: InputThread,
) -> Result<InputThread, RuntimeError> {
    match effect {
        Effect::OpenInEditor { list, line } => edit(app, terminal, input, &list, line),
        effect @ (Effect::Save(_)
        | Effect::CreateEntry { .. }
        | Effect::RenameEntry { .. }
        | Effect::DeleteEntries(_)
        | Effect::MoveEntries { .. }
        | Effect::ReloadList(_)
        | Effect::Rescan
        | Effect::StepEntries { .. }) => {
            execute(app, effect);
            Ok(input)
        }
    }
}

/// Opens the list at `list` in the editor at `line`, waits for it and reloads the list.
///
/// Input is stopped while the editor runs, so that the keys typed in the editor never reach
/// RuDo, and resumed on the same channel afterwards. Editor failures become a message.
fn edit(
    app: &mut App,
    terminal: &mut TerminalGuard,
    input: InputThread,
    list: &ListPath,
    line: Option<NonZeroUsize>,
) -> Result<InputThread, RuntimeError> {
    let visual = environment_variable("VISUAL");
    let editor = environment_variable("EDITOR");
    let command_line = choose_editor(app.config().editor(), visual.as_deref(), editor.as_deref());
    let file = app.workspace().absolute_path(list);
    let command = match EditorCommand::new(command_line, &file, line) {
        Ok(command) => command,
        Err(error) => {
            app.editor_failed(&error);
            return Ok(input);
        }
    };
    let sender = input.stop().ok_or(RuntimeError::InputClosed)?;
    let outcome = terminal
        .suspend(|| run_editor(&command))
        .map_err(RuntimeError::Suspend)?;
    let input = InputThread::spawn(sender).map_err(RuntimeError::SpawnInput)?;
    if let Err(error) = outcome {
        app.editor_failed(&error);
    }
    app.reload_list(list);
    Ok(input)
}

/// Runs `command` in the current terminal and waits until it exits.
fn run_editor(command: &EditorCommand) -> Result<(), EditorError> {
    let program = command.program();
    tracing::info!(?command, "starting the editor");
    let status = Command::new(program)
        .args(command.arguments())
        .status()
        .map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => EditorError::NotFound {
                program: program.to_owned(),
            },
            _ => EditorError::Start {
                program: program.to_owned(),
                source,
            },
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(EditorError::Failed {
            program: program.to_owned(),
            status,
        })
    }
}

/// Returns the value of the environment variable `name`, or `None` if it is not set or not
/// valid Unicode, which is logged.
fn environment_variable(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) => Some(value),
        Err(VarError::NotPresent) => None,
        Err(error @ VarError::NotUnicode(_)) => {
            tracing::warn!(name, %error, "ignoring environment variable");
            None
        }
    }
}

/// Returns how long the loop may wait at `now` for input: until `deadline` of the app if it
/// has one and comes first, otherwise for `until_next_day`.
fn time_until_wake_up(
    now: Instant,
    deadline: Option<Instant>,
    until_next_day: Duration,
) -> Duration {
    deadline.map_or(until_next_day, |deadline| {
        deadline.saturating_duration_since(now).min(until_next_day)
    })
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

    #[rstest]
    #[case::no_deadline(None, 10 * HOUR)]
    #[case::deadline_first(Some(3), 3)]
    #[case::next_day_first(Some(11 * HOUR), 10 * HOUR)]
    #[case::deadline_passed(Some(0), 0)]
    fn wakes_up_at_the_earlier_of_deadline_and_next_day(
        #[case] seconds_until_deadline: Option<u64>,
        #[case] expected_seconds: u64,
    ) {
        let now = Instant::now();
        let deadline = seconds_until_deadline.map(|seconds| now + Duration::from_secs(seconds));
        let until_next_day = Duration::from_secs(10 * HOUR);
        let expected = Duration::from_secs(expected_seconds);

        let system_under_test = time_until_wake_up(now, deadline, until_next_day);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn deadline_in_the_past_wakes_up_at_once() {
        let deadline = Instant::now();
        let now = deadline + Duration::from_secs(1);

        let system_under_test = time_until_wake_up(now, Some(deadline), Duration::from_secs(HOUR));

        assert_eq!(system_under_test, Duration::ZERO);
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
