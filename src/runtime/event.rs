//! Events from the terminal: an [`InputThread`] reads them and sends them over a channel, and
//! [`next_batch`] collects them in batches, so the loop draws once per batch.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvError, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ratatui::crossterm::event::{self as terminal_event, KeyEventKind};

use crate::config::keymap::key::KeyChord;

/// How long the input thread waits for terminal input before it checks whether it should stop.
///
/// It bounds how long dropping an [`InputThread`] takes.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Something that happened in the terminal and that the application reacts to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Event {
    /// A key was pressed or is held down.
    Key(KeyChord),
    /// The terminal was resized; the next draw adapts to the new size.
    Resize,
    /// Text was pasted with bracketed paste.
    Paste(String),
}

impl Event {
    /// Converts a terminal event into the [`Event`] RuDo handles, or `None` for events it
    /// ignores: key releases, keys RuDo has no notation for, focus changes and mouse events.
    #[must_use]
    pub fn from_terminal(event: terminal_event::Event) -> Option<Self> {
        match event {
            terminal_event::Event::Key(key) => match key.kind {
                KeyEventKind::Press | KeyEventKind::Repeat => {
                    KeyChord::from_key_event(key).map(Self::Key)
                }
                KeyEventKind::Release => None,
            },
            terminal_event::Event::Resize(_, _) => Some(Self::Resize),
            terminal_event::Event::Paste(text) => Some(Self::Paste(text)),
            terminal_event::Event::FocusGained
            | terminal_event::Event::FocusLost
            | terminal_event::Event::Mouse(_) => None,
        }
    }
}

/// A thread that reads terminal input and sends it as [`Event`]s over a channel.
///
/// Dropping it stops the thread and waits for it, so that no keystroke is read afterwards.
/// To hand the terminal to another program, such as an external editor, drop the
/// [`InputThread`] and spawn a new one on the same channel when the program has exited.
#[derive(Debug)]
pub struct InputThread {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl InputThread {
    /// Starts reading terminal input and sending it to `sender`.
    ///
    /// The thread stops after it has sent a read error, or when the receiver is gone.
    ///
    /// # Errors
    ///
    /// Returns the I/O error of the operating system if the thread cannot be created.
    pub fn spawn(sender: Sender<io::Result<Event>>) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let handle = thread::Builder::new()
            .name("terminal input".into())
            .spawn({
                let stop = Arc::clone(&stop);
                move || read_events(&sender, &stop)
            })?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }
}

impl Drop for InputThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            tracing::error!("terminal input thread panicked");
        }
    }
}

/// Sends terminal events to `sender` until `stop` is set, reading fails or the receiver is
/// gone.
fn read_events(sender: &Sender<io::Result<Event>>, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        let event = match poll_event() {
            Ok(None) => continue,
            Ok(Some(event)) => Ok(event),
            Err(error) => Err(error),
        };
        let failed = event.is_err();
        if sender.send(event).is_err() || failed {
            return;
        }
    }
}

/// Waits up to [`POLL_INTERVAL`] for a terminal event and returns it if RuDo handles it.
fn poll_event() -> io::Result<Option<Event>> {
    if !terminal_event::poll(POLL_INTERVAL)? {
        return Ok(None);
    }
    Ok(Event::from_terminal(terminal_event::read()?))
}

/// Waits up to `timeout` for the first item on `receiver`, then takes every item already
/// queued behind it without waiting.
///
/// Returns an empty batch if nothing arrived within `timeout`.
///
/// # Errors
///
/// Returns [`RecvError`] if the channel is empty and every sender is gone.
pub fn next_batch<T>(receiver: &Receiver<T>, timeout: Duration) -> Result<Vec<T>, RecvError> {
    let first = match receiver.recv_timeout(timeout) {
        Ok(first) => first,
        Err(RecvTimeoutError::Timeout) => return Ok(Vec::new()),
        Err(RecvTimeoutError::Disconnected) => return Err(RecvError),
    };
    Ok(std::iter::once(first).chain(receiver.try_iter()).collect())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyEventState, KeyModifiers, MouseEvent, MouseEventKind,
    };
    use rstest::rstest;

    use super::*;

    fn key_event(kind: KeyEventKind) -> terminal_event::Event {
        terminal_event::Event::Key(KeyEvent {
            code: KeyCode::Char('j'),
            modifiers: KeyModifiers::NONE,
            kind,
            state: KeyEventState::NONE,
        })
    }

    #[rstest]
    #[case::press(KeyEventKind::Press)]
    #[case::repeat(KeyEventKind::Repeat)]
    fn pressed_and_repeated_keys_become_key_events(#[case] kind: KeyEventKind) {
        let expected = Event::Key("j".parse().unwrap());

        let system_under_test = Event::from_terminal(key_event(kind));

        assert_eq!(system_under_test, Some(expected));
    }

    #[rstest]
    #[case::release(key_event(KeyEventKind::Release))]
    #[case::key_without_notation(terminal_event::Event::Key(KeyEvent::new(
        KeyCode::CapsLock,
        KeyModifiers::NONE
    )))]
    #[case::focus_gained(terminal_event::Event::FocusGained)]
    #[case::focus_lost(terminal_event::Event::FocusLost)]
    #[case::mouse(terminal_event::Event::Mouse(MouseEvent {
        kind: MouseEventKind::Moved,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }))]
    fn ignored_terminal_events_are_dropped(#[case] event: terminal_event::Event) {
        let system_under_test = Event::from_terminal(event);

        assert_eq!(system_under_test, None);
    }

    #[test]
    fn resize_becomes_a_resize_event() {
        let event = terminal_event::Event::Resize(80, 24);

        let system_under_test = Event::from_terminal(event);

        assert_eq!(system_under_test, Some(Event::Resize));
    }

    #[test]
    fn paste_keeps_the_pasted_text() {
        let text = "Buy milk\nBake bread";
        let event = terminal_event::Event::Paste(text.into());

        let system_under_test = Event::from_terminal(event);

        assert_eq!(system_under_test, Some(Event::Paste(text.into())));
    }

    #[test]
    fn batch_contains_every_queued_item_in_order() {
        let (sender, receiver) = mpsc::channel();
        for item in 1..=3 {
            sender.send(item).unwrap();
        }
        let expected = vec![1, 2, 3];

        let system_under_test = next_batch(&receiver, Duration::ZERO);

        assert_eq!(system_under_test, Ok(expected));
    }

    #[test]
    fn batch_is_empty_after_the_timeout() {
        let (_sender, receiver) = mpsc::channel::<u8>();

        let system_under_test = next_batch(&receiver, Duration::from_millis(1));

        assert_eq!(system_under_test, Ok(Vec::new()));
    }

    #[test]
    fn batch_keeps_items_sent_before_the_sender_was_dropped() {
        let (sender, receiver) = mpsc::channel();
        sender.send(1).unwrap();
        drop(sender);

        let system_under_test = next_batch(&receiver, Duration::ZERO);

        assert_eq!(system_under_test, Ok(vec![1]));
    }

    #[test]
    fn batch_fails_when_the_channel_is_empty_and_closed() {
        let (sender, receiver) = mpsc::channel::<u8>();
        drop(sender);

        let system_under_test = next_batch(&receiver, Duration::ZERO);

        assert_eq!(system_under_test, Err(RecvError));
    }
}
