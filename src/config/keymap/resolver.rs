//! Turns key presses into [`Action`]s, one [`KeyChord`] at a time, so that key sequences such
//! as `g g` can be typed one key after the other.

use crate::config::keymap::action::{Action, Context};
use crate::config::keymap::key::{Key, KeyChord, Modifiers};
use crate::config::keymap::keymap::{Keymap, Lookup};

/// What a key press means after [`KeyResolver::feed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resolution {
    /// The chords typed so far complete the binding of this action.
    Action(Action),
    /// The chords typed so far begin at least one binding; more chords are needed.
    Pending,
    /// `esc` cancelled the pending sequence; the key press has no further effect.
    Cancelled,
    /// The key is not bound in the context; the app may handle it otherwise, for example as
    /// text input.
    NoMatch,
}

/// Collects the chords of a key sequence until they resolve to an [`Action`].
///
/// No timeout is needed: a [`Keymap`] never binds a key sequence that is the beginning of
/// another one, so a sequence is complete as soon as it is bound. A pending sequence never
/// carries over into a different [`Context`].
///
/// # Examples
///
/// ```
/// use rudo::config::keymap::action::{Action, Context};
/// use rudo::config::keymap::key::KeyChord;
/// use rudo::config::keymap::keymap::{KeyBindings, Keymap};
/// use rudo::config::keymap::resolver::{KeyResolver, Resolution};
///
/// let keymap = Keymap::new(&KeyBindings::default()).expect("the defaults are valid");
/// let g: KeyChord = "g".parse()?;
/// let mut resolver = KeyResolver::default();
///
/// assert_eq!(resolver.feed(&keymap, Context::Tasks, g), Resolution::Pending);
/// assert_eq!(resolver.pending(), [g]);
/// assert_eq!(
///     resolver.feed(&keymap, Context::Tasks, g),
///     Resolution::Action(Action::MoveToTop)
/// );
/// assert_eq!(resolver.pending(), []);
/// # Ok::<(), rudo::config::keymap::key::KeyNotationError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyResolver {
    context: Option<Context>,
    chords: Vec<KeyChord>,
}

impl KeyResolver {
    /// Resolves `chord` typed in `context`, following the chords that are pending.
    ///
    /// A chord that breaks a pending sequence is retried once as the start of a new sequence,
    /// so typing `d` then `j` moves down. Plain `esc` breaking a pending sequence cancels it
    /// instead. Pending chords of a different context are discarded first.
    #[must_use]
    pub fn feed(&mut self, keymap: &Keymap, context: Context, chord: KeyChord) -> Resolution {
        if self.context != Some(context) {
            self.reset();
            self.context = Some(context);
        }
        self.chords.push(chord);
        match keymap.lookup(context, &self.chords) {
            Lookup::Action(action) => {
                self.chords.clear();
                Resolution::Action(action)
            }
            Lookup::Prefix => Resolution::Pending,
            Lookup::None => {
                let broke_pending_sequence = self.chords.len() > 1;
                self.chords.clear();
                if !broke_pending_sequence {
                    Resolution::NoMatch
                } else if chord == KeyChord::new(Key::Esc, Modifiers::NONE) {
                    Resolution::Cancelled
                } else {
                    // Terminates: with no chords pending the retry cannot break a sequence.
                    self.feed(keymap, context, chord)
                }
            }
        }
    }

    /// Returns the chords of the sequence typed so far, empty if none is pending.
    #[must_use]
    pub fn pending(&self) -> &[KeyChord] {
        &self.chords
    }

    /// Discards the pending sequence; called whenever the context changes.
    pub fn reset(&mut self) {
        self.chords.clear();
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::config::keymap::keymap::KeyBindings;

    fn default_keymap() -> Keymap {
        Keymap::new(&KeyBindings::default()).unwrap()
    }

    fn chords(notation: &str) -> Vec<KeyChord> {
        notation
            .split_whitespace()
            .map(|chord| chord.parse().unwrap())
            .collect()
    }

    fn feed_all(
        resolver: &mut KeyResolver,
        keymap: &Keymap,
        context: Context,
        notation: &str,
    ) -> Vec<Resolution> {
        chords(notation)
            .into_iter()
            .map(|chord| resolver.feed(keymap, context, chord))
            .collect()
    }

    fn resolve(context: Context, notation: &str) -> (Vec<Resolution>, KeyResolver) {
        let mut resolver = KeyResolver::default();
        let resolutions = feed_all(&mut resolver, &default_keymap(), context, notation);
        (resolutions, resolver)
    }

    const PENDING: Resolution = Resolution::Pending;
    const CANCELLED: Resolution = Resolution::Cancelled;
    const NO_MATCH: Resolution = Resolution::NoMatch;

    #[rstest]
    #[case::single_key(Context::Tasks, "j", &[Resolution::Action(Action::MoveDown)])]
    #[case::named_key(Context::Lists, "down", &[Resolution::Action(Action::MoveDown)])]
    #[case::modified_key(Context::Tasks, "ctrl-d", &[Resolution::Action(Action::HalfPageDown)])]
    #[case::same_key_other_context(Context::Lists, "o", &[Resolution::Action(Action::NewList)])]
    #[case::esc_without_pending(Context::Tasks, "esc", &[Resolution::Action(Action::ClearFilter)])]
    #[case::esc_in_help(Context::Help, "esc", &[Resolution::Action(Action::CloseHelp)])]
    #[case::esc_in_input(Context::Input, "esc", &[Resolution::Action(Action::Cancel)])]
    #[case::repeated(
        Context::Tasks,
        "j j",
        &[Resolution::Action(Action::MoveDown), Resolution::Action(Action::MoveDown)],
    )]
    fn single_keys_resolve_at_once(
        #[case] context: Context,
        #[case] notation: &str,
        #[case] expected: &[Resolution],
    ) {
        let (system_under_test, _) = resolve(context, notation);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::move_to_top(Context::Tasks, "g g", &[PENDING, Resolution::Action(Action::MoveToTop)])]
    #[case::delete(Context::Tasks, "d d", &[PENDING, Resolution::Action(Action::Delete)])]
    #[case::toggle_fold(Context::Tasks, "z a", &[PENDING, Resolution::Action(Action::ToggleFold)])]
    #[case::sort(Context::Tasks, "s p", &[PENDING, Resolution::Action(Action::SortByPriority)])]
    #[case::in_help(Context::Help, "g g", &[PENDING, Resolution::Action(Action::MoveToTop)])]
    #[case::consecutive(
        Context::Tasks,
        "g g d d",
        &[
            PENDING,
            Resolution::Action(Action::MoveToTop),
            PENDING,
            Resolution::Action(Action::Delete),
        ],
    )]
    fn sequences_resolve_on_their_last_chord(
        #[case] context: Context,
        #[case] notation: &str,
        #[case] expected: &[Resolution],
    ) {
        let (system_under_test, _) = resolve(context, notation);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::into_single_key(Context::Tasks, "d j", &[PENDING, Resolution::Action(Action::MoveDown)])]
    #[case::into_new_sequence(
        Context::Tasks,
        "d g g",
        &[PENDING, PENDING, Resolution::Action(Action::MoveToTop)],
    )]
    #[case::into_other_prefix(
        Context::Tasks,
        "s z a",
        &[PENDING, PENDING, Resolution::Action(Action::ToggleFold)],
    )]
    #[case::into_unknown_key(Context::Tasks, "d w", &[PENDING, NO_MATCH])]
    #[case::retried_only_once(
        Context::Tasks,
        "g w j",
        &[PENDING, NO_MATCH, Resolution::Action(Action::MoveDown)],
    )]
    fn breaking_chord_is_retried_as_new_sequence(
        #[case] context: Context,
        #[case] notation: &str,
        #[case] expected: &[Resolution],
    ) {
        let (system_under_test, _) = resolve(context, notation);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::cancels(Context::Tasks, "d esc", &[PENDING, CANCELLED])]
    #[case::consumed_not_clear_filter(
        Context::Tasks,
        "g esc esc",
        &[PENDING, CANCELLED, Resolution::Action(Action::ClearFilter)],
    )]
    #[case::next_key_starts_fresh(
        Context::Tasks,
        "d esc d",
        &[PENDING, CANCELLED, PENDING],
    )]
    #[case::modified_esc_is_retried(Context::Tasks, "d ctrl-esc", &[PENDING, NO_MATCH])]
    fn esc_cancels_pending_sequence(
        #[case] context: Context,
        #[case] notation: &str,
        #[case] expected: &[Resolution],
    ) {
        let (system_under_test, _) = resolve(context, notation);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::unbound(Context::Tasks, "w")]
    #[case::bound_in_other_context(Context::Lists, "y")]
    #[case::printable_in_input(Context::Input, "x")]
    #[case::sequence_of_other_context(Context::Dialog, "g")]
    fn unknown_keys_do_not_match(#[case] context: Context, #[case] notation: &str) {
        let (system_under_test, _) = resolve(context, notation);

        assert_eq!(system_under_test, [NO_MATCH]);
    }

    #[rstest]
    #[case::nothing_typed("", "")]
    #[case::first_chord("g", "g")]
    #[case::completed("g g", "")]
    #[case::retried_prefix("d s", "s")]
    #[case::cancelled("d esc", "")]
    #[case::unknown("w", "")]
    #[case::broken_by_unknown("d w", "")]
    fn pending_returns_chords_typed_so_far(#[case] notation: &str, #[case] expected: &str) {
        let expected = chords(expected);

        let (_, system_under_test) = resolve(Context::Tasks, notation);

        assert_eq!(system_under_test.pending(), expected);
    }

    #[test]
    fn reset_clears_pending_sequence() {
        let keymap = default_keymap();
        let mut resolver = KeyResolver::default();
        feed_all(&mut resolver, &keymap, Context::Tasks, "d");
        resolver.reset();
        let expected = [PENDING];

        let system_under_test = feed_all(&mut resolver, &keymap, Context::Tasks, "d");

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn pending_sequence_does_not_leak_into_other_context() {
        let keymap = default_keymap();
        let mut resolver = KeyResolver::default();
        feed_all(&mut resolver, &keymap, Context::Tasks, "g");
        let expected_resolutions = [PENDING];
        let expected_pending = chords("g");

        let system_under_test = feed_all(&mut resolver, &keymap, Context::Help, "g");

        assert_eq!(system_under_test, expected_resolutions);
        assert_eq!(resolver.pending(), expected_pending);
    }
}
