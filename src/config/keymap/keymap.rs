//! The effective keymap: the default keys of every [`Action`] with the user's
//! [`KeyBindings`] applied, validated per [`Context`] and looked up by the chords typed so far.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt::{self, Formatter};
use std::ops::Bound;

use serde::Deserialize;
use serde::de::{self, Deserializer, IntoDeserializer, SeqAccess, Visitor};
use strum::IntoEnumIterator;

use crate::config::keymap::action::{Action, Context};
use crate::config::keymap::key::{Key, KeyChord, KeySequence, Modifier};

/// The `keybindings` section of the configuration: the keys that replace the default keys of
/// an action.
///
/// Each action name maps to one key sequence or a list of them; `[]` unbinds the action.
/// Unknown action names are rejected with a list of the valid names, and a section without
/// entries means no overrides.
///
/// # Examples
///
/// ```
/// use rudo::config::keymap::action::Action;
/// use rudo::config::keymap::keymap::KeyBindings;
///
/// let yaml = "toggle-done: [x, ctrl-x]\nset-due-date: t d\nopen-in-editor: []\n";
/// let bindings: KeyBindings = serde_saphyr::from_str(yaml)?;
/// assert_eq!(bindings.get(Action::SetDueDate).map(<[_]>::len), Some(1));
/// assert_eq!(bindings.get(Action::OpenInEditor), Some([].as_slice()));
/// assert_eq!(bindings.get(Action::Quit), None);
/// # Ok::<(), serde_saphyr::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyBindings(BTreeMap<Action, Vec<KeySequence>>);

impl KeyBindings {
    /// Returns the keys that replace the default keys of `action`, empty if the action is
    /// unbound, or `None` if the action keeps its default keys.
    #[must_use]
    pub fn get(&self, action: Action) -> Option<&[KeySequence]> {
        self.0.get(&action).map(Vec::as_slice)
    }
}

impl FromIterator<(Action, Vec<KeySequence>)> for KeyBindings {
    fn from_iter<I: IntoIterator<Item = (Action, Vec<KeySequence>)>>(bindings: I) -> Self {
        Self(bindings.into_iter().collect())
    }
}

impl<'de> Deserialize<'de> for KeyBindings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bindings = BTreeMap::<Action, BindingKeys>::deserialize(deserializer)?;
        Ok(bindings
            .into_iter()
            .map(|(action, BindingKeys(keys))| (action, keys))
            .collect())
    }
}

/// The keys of one entry of the `keybindings` section: one key sequence or a list.
struct BindingKeys(Vec<KeySequence>);

impl<'de> Deserialize<'de> for BindingKeys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(BindingKeysVisitor)
    }
}

struct BindingKeysVisitor;

impl BindingKeysVisitor {
    fn single<'de, E: de::Error>(scalar: impl IntoDeserializer<'de, E>) -> Result<BindingKeys, E> {
        KeySequence::deserialize(scalar.into_deserializer())
            .map(|sequence| BindingKeys(vec![sequence]))
    }
}

impl<'de> Visitor<'de> for BindingKeysVisitor {
    type Value = BindingKeys;

    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter.write_str("a key sequence or a list of key sequences")
    }

    fn visit_str<E: de::Error>(self, notation: &str) -> Result<Self::Value, E> {
        Self::single(notation)
    }

    fn visit_u64<E: de::Error>(self, digits: u64) -> Result<Self::Value, E> {
        Self::single(digits)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequences: A) -> Result<Self::Value, A::Error> {
        let mut keys = Vec::with_capacity(sequences.size_hint().unwrap_or(0));
        while let Some(sequence) = sequences.next_element()? {
            keys.push(sequence);
        }
        Ok(BindingKeys(keys))
    }
}

/// Why a keymap cannot work.
///
/// Every message names the key sequence and the actions involved, so that the user knows
/// which entries of the `keybindings` section to change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeymapError {
    /// The same key sequence is bound to two actions in one context.
    #[error(
        "`{sequence}` is bound to both `{first}` and `{second}` in the {context}; \
         rebind or unbind (`[]`) one of them"
    )]
    Conflict {
        /// Context in which both actions are valid.
        context: Context,
        /// Key sequence bound to both actions.
        sequence: KeySequence,
        /// Action that comes first in the registry.
        first: Action,
        /// Action that comes later in the registry.
        second: Action,
    },
    /// A key sequence is the beginning of a longer one, so the longer one can never be typed.
    #[error(
        "`{prefix}` of `{prefix_action}` is the beginning of `{sequence}` of `{action}` in the \
         {context}; rebind or unbind (`[]`) one of them"
    )]
    PrefixConflict {
        /// Context in which both actions are valid.
        context: Context,
        /// The shorter key sequence.
        prefix: KeySequence,
        /// Action bound to the shorter key sequence.
        prefix_action: Action,
        /// The longer key sequence that starts with `prefix`.
        sequence: KeySequence,
        /// Action bound to the longer key sequence.
        action: Action,
    },
    /// A text input binding starts with a key that types a character.
    #[error(
        "`{sequence}` of `{action}` starts with a key that types text in the {}; \
         use a key with ctrl or alt or a named key such as `enter`",
        Context::Input
    )]
    PrintableInInput {
        /// Action valid in the text input.
        action: Action,
        /// Key sequence that starts with a printable key.
        sequence: KeySequence,
    },
}

/// What the chords typed so far mean in a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lookup {
    /// The chords are bound to this action.
    Action(Action),
    /// The chords are the beginning of at least one binding; more chords are needed.
    Prefix,
    /// No binding is or starts with these chords.
    None,
}

/// The effective keymap: the keys of every action and, per context, the action bound to each
/// key sequence.
///
/// A keymap is always valid: no key sequence is bound twice or is the beginning of another
/// one in the same context, and no text input binding starts with a printable key.
///
/// # Examples
///
/// ```
/// use rudo::config::keymap::action::{Action, Context};
/// use rudo::config::keymap::key::KeySequence;
/// use rudo::config::keymap::keymap::{KeyBindings, Keymap, Lookup};
///
/// let keymap = Keymap::new(&KeyBindings::default()).expect("the defaults are valid");
/// let g_g: KeySequence = "g g".parse()?;
/// assert_eq!(keymap.lookup(Context::Tasks, &g_g.chords()[..1]), Lookup::Prefix);
/// assert_eq!(keymap.lookup(Context::Tasks, g_g.chords()), Lookup::Action(Action::MoveToTop));
/// # Ok::<(), rudo::config::keymap::key::KeyNotationError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    keys: BTreeMap<Action, Vec<KeySequence>>,
    bindings: BTreeMap<Context, BTreeMap<KeySequence, Action>>,
}

impl Keymap {
    /// Builds the keymap from the default keys of every action, replacing those of the
    /// actions in `overrides`, and validates it per context.
    ///
    /// # Errors
    ///
    /// Returns every [`KeymapError`] found, ordered by context, then by action for
    /// [`KeymapError::Conflict`] and [`KeymapError::PrintableInInput`] and by key sequence for
    /// [`KeymapError::PrefixConflict`].
    pub fn new(overrides: &KeyBindings) -> Result<Self, Vec<KeymapError>> {
        let keys: BTreeMap<Action, Vec<KeySequence>> = Action::iter()
            .map(|action| {
                let keys = overrides
                    .get(action)
                    .map_or_else(|| action.default_keys(), <[_]>::to_vec);
                (action, keys)
            })
            .collect();
        let mut errors = Vec::new();
        let bindings = Context::iter()
            .map(|context| (context, bind_context(context, &keys, &mut errors)))
            .collect();
        if errors.is_empty() {
            Ok(Self { keys, bindings })
        } else {
            Err(errors)
        }
    }

    /// Returns the key sequences bound to `action`, empty if it is unbound.
    #[must_use]
    pub fn keys(&self, action: Action) -> &[KeySequence] {
        self.keys.get(&action).map_or(&[], Vec::as_slice)
    }

    /// Returns what the `chords` typed so far mean in `context`.
    ///
    /// No chords at all are the beginning of every binding, so they are a
    /// [`Lookup::Prefix`] in every context with bindings.
    #[must_use]
    pub fn lookup(&self, context: Context, chords: &[KeyChord]) -> Lookup {
        let Some(bindings) = self.bindings.get(&context) else {
            return Lookup::None;
        };
        if let Some(&action) = bindings.get(chords) {
            return Lookup::Action(action);
        }
        // A prefix sorts directly before its extensions, so only the next binding can extend it.
        let is_prefix = bindings
            .range::<[KeyChord], _>((Bound::Excluded(chords), Bound::Unbounded))
            .next()
            .is_some_and(|(sequence, _)| sequence.chords().starts_with(chords));
        if is_prefix {
            Lookup::Prefix
        } else {
            Lookup::None
        }
    }
}

/// Binds the keys of the actions valid in `context`, recording every [`KeymapError`] of the
/// context in `errors`.
fn bind_context(
    context: Context,
    keys: &BTreeMap<Action, Vec<KeySequence>>,
    errors: &mut Vec<KeymapError>,
) -> BTreeMap<KeySequence, Action> {
    let mut bindings = BTreeMap::new();
    let actions = keys
        .iter()
        .filter(|(action, _)| action.contexts().contains(&context));
    for (&action, sequences) in actions {
        for sequence in sequences {
            if types_text(context) && starts_with_printable_key(sequence) {
                errors.push(KeymapError::PrintableInInput {
                    action,
                    sequence: sequence.clone(),
                });
            }
            match bindings.entry(sequence.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(action);
                }
                Entry::Occupied(entry) if *entry.get() != action => {
                    errors.push(KeymapError::Conflict {
                        context,
                        sequence: sequence.clone(),
                        first: *entry.get(),
                        second: action,
                    });
                }
                Entry::Occupied(_) => {}
            }
        }
    }
    errors.extend(prefix_conflicts(context, &bindings));
    bindings
}

/// Returns a conflict for every binding that is the beginning of another one.
///
/// A prefix sorts directly before its extensions, so comparing neighbours finds every prefix.
fn prefix_conflicts(
    context: Context,
    bindings: &BTreeMap<KeySequence, Action>,
) -> impl Iterator<Item = KeymapError> {
    bindings
        .iter()
        .zip(bindings.iter().skip(1))
        .filter(|((prefix, _), (sequence, _))| sequence.chords().starts_with(prefix.chords()))
        .map(
            move |((prefix, &prefix_action), (sequence, &action))| KeymapError::PrefixConflict {
                context,
                prefix: prefix.clone(),
                prefix_action,
                sequence: sequence.clone(),
                action,
            },
        )
}

/// Returns whether printable keys type text in `context` instead of triggering actions.
const fn types_text(context: Context) -> bool {
    match context {
        Context::Input => true,
        Context::Lists | Context::Tasks | Context::Help | Context::Dialog => false,
    }
}

/// Returns whether the first chord of `sequence` types a character. Later chords do not type
/// text because the keymap is waiting for the rest of the sequence by then.
fn starts_with_printable_key(sequence: &KeySequence) -> bool {
    sequence.chords().first().is_some_and(|chord| {
        matches!(chord.key(), Key::Char(_))
            && !chord.modifiers().contains(Modifier::Ctrl)
            && !chord.modifiers().contains(Modifier::Alt)
    })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn sequence(notation: &str) -> KeySequence {
        notation.parse().unwrap()
    }

    fn sequences(notations: &[&str]) -> Vec<KeySequence> {
        notations
            .iter()
            .map(|notation| sequence(notation))
            .collect()
    }

    fn bindings(yaml: &str) -> KeyBindings {
        serde_saphyr::from_str(yaml).unwrap()
    }

    #[test]
    fn default_keymap_is_valid() {
        let overrides = KeyBindings::default();

        let system_under_test = Keymap::new(&overrides);

        assert_eq!(system_under_test.err(), None);
    }

    #[test]
    fn actions_without_override_keep_their_default_keys() {
        let overrides = KeyBindings::default();

        let system_under_test = Keymap::new(&overrides).unwrap();

        for action in Action::iter() {
            assert_eq!(
                system_under_test.keys(action),
                action.default_keys(),
                "{action}"
            );
        }
    }

    #[rstest]
    #[case::list("toggle-done: [x, space, ctrl-x]", Action::ToggleDone, &["x", "space", "ctrl-x"])]
    #[case::single("set-due-date: t d", Action::SetDueDate, &["t d"])]
    #[case::unbind("open-in-editor: []", Action::OpenInEditor, &[])]
    #[case::digit("focus-lists: 3", Action::FocusLists, &["3"])]
    fn override_replaces_the_default_keys(
        #[case] yaml: &str,
        #[case] action: Action,
        #[case] expected: &[&str],
    ) {
        let overrides = bindings(yaml);
        let expected = sequences(expected);

        let system_under_test = Keymap::new(&overrides).unwrap();

        assert_eq!(system_under_test.keys(action), expected);
    }

    #[test]
    fn override_leaves_other_actions_alone() {
        let overrides = bindings("quit: ctrl-q");
        let expected = Action::Help.default_keys();

        let system_under_test = Keymap::new(&overrides).unwrap();

        assert_eq!(system_under_test.keys(Action::Help), expected);
    }

    #[rstest]
    #[case::single_key(Context::Tasks, "j", Lookup::Action(Action::MoveDown))]
    #[case::named_key(Context::Lists, "down", Lookup::Action(Action::MoveDown))]
    #[case::sequence(Context::Tasks, "g g", Lookup::Action(Action::MoveToTop))]
    #[case::prefix(Context::Tasks, "g", Lookup::Prefix)]
    #[case::prefix_of_several(Context::Tasks, "s", Lookup::Prefix)]
    #[case::wrong_continuation(Context::Tasks, "g x", Lookup::None)]
    #[case::too_long(Context::Tasks, "g g g", Lookup::None)]
    #[case::unbound(Context::Tasks, "w", Lookup::None)]
    #[case::lists_context(Context::Lists, "o", Lookup::Action(Action::NewList))]
    #[case::tasks_context(Context::Tasks, "o", Lookup::Action(Action::NewTaskBelow))]
    #[case::tasks_only_prefix(Context::Lists, "s", Lookup::None)]
    #[case::help_context(Context::Help, "q", Lookup::Action(Action::CloseHelp))]
    #[case::dialog_context(Context::Dialog, "y", Lookup::Action(Action::ConfirmYes))]
    #[case::input_types_text(Context::Input, "j", Lookup::None)]
    #[case::input_action(Context::Input, "esc", Lookup::Action(Action::Cancel))]
    fn looks_up_chords_typed_so_far(
        #[case] context: Context,
        #[case] notation: &str,
        #[case] expected: Lookup,
    ) {
        let chords = sequence(notation);
        let keymap = Keymap::new(&KeyBindings::default()).unwrap();

        let system_under_test = keymap.lookup(context, chords.chords());

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn unbound_action_is_not_found() {
        let overrides = bindings("toggle-done: []");
        let chords = sequence("x");
        let keymap = Keymap::new(&overrides).unwrap();

        let system_under_test = keymap.lookup(Context::Tasks, chords.chords());

        assert_eq!(system_under_test, Lookup::None);
    }

    #[test]
    fn rebound_action_is_found_by_its_new_keys() {
        let overrides = bindings("toggle-done: ctrl-x");
        let chords = sequence("ctrl-x");
        let keymap = Keymap::new(&overrides).unwrap();

        let system_under_test = keymap.lookup(Context::Tasks, chords.chords());

        assert_eq!(system_under_test, Lookup::Action(Action::ToggleDone));
    }

    #[test]
    fn key_of_another_action_is_a_conflict_in_every_shared_context() {
        let overrides = bindings("undo: q");
        let expected: Vec<KeymapError> = [Context::Lists, Context::Tasks]
            .into_iter()
            .map(|context| KeymapError::Conflict {
                context,
                sequence: sequence("q"),
                first: Action::Quit,
                second: Action::Undo,
            })
            .collect();

        let system_under_test = Keymap::new(&overrides);

        assert_eq!(system_under_test.err(), Some(expected));
    }

    #[test]
    fn same_key_in_different_contexts_is_no_conflict() {
        let overrides = bindings("new-list: x");

        let system_under_test = Keymap::new(&overrides);

        assert_eq!(system_under_test.err(), None);
    }

    #[test]
    fn same_key_twice_for_one_action_is_no_conflict() {
        let overrides = bindings("quit: [q, q]");

        let system_under_test = Keymap::new(&overrides);

        assert_eq!(system_under_test.err(), None);
    }

    #[test]
    fn binding_that_begins_another_is_a_prefix_conflict() {
        let overrides = bindings("help: g");
        let expected: Vec<KeymapError> = [Context::Lists, Context::Tasks]
            .into_iter()
            .map(|context| KeymapError::PrefixConflict {
                context,
                prefix: sequence("g"),
                prefix_action: Action::Help,
                sequence: sequence("g g"),
                action: Action::MoveToTop,
            })
            .collect();

        let system_under_test = Keymap::new(&overrides);

        assert_eq!(system_under_test.err(), Some(expected));
    }

    #[test]
    fn every_level_of_nested_prefixes_is_reported() {
        let overrides = bindings("help: g\nundo: g g g");
        let prefix_conflict =
            |prefix: &str, prefix_action, extension: &str, action| KeymapError::PrefixConflict {
                context: Context::Tasks,
                prefix: sequence(prefix),
                prefix_action,
                sequence: sequence(extension),
                action,
            };
        let expected = [
            prefix_conflict("g", Action::Help, "g g", Action::MoveToTop),
            prefix_conflict("g g", Action::MoveToTop, "g g g", Action::Undo),
        ];

        let system_under_test = Keymap::new(&overrides).unwrap_err();

        assert_eq!(system_under_test[2..], expected);
    }

    #[rstest]
    #[case::character("submit: [enter, a]", Action::Submit, "a")]
    #[case::space("cancel: space", Action::Cancel, "space")]
    #[case::shifted("submit: A", Action::Submit, "A")]
    #[case::sequence("cancel: x ctrl-c", Action::Cancel, "x ctrl-c")]
    fn printable_key_in_text_input_is_rejected(
        #[case] yaml: &str,
        #[case] action: Action,
        #[case] notation: &str,
    ) {
        let overrides = bindings(yaml);
        let expected = vec![KeymapError::PrintableInInput {
            action,
            sequence: sequence(notation),
        }];

        let system_under_test = Keymap::new(&overrides);

        assert_eq!(system_under_test.err(), Some(expected));
    }

    #[rstest]
    #[case::ctrl("submit: ctrl-j")]
    #[case::alt("submit: alt-a")]
    #[case::named_key("submit: f2")]
    #[case::printable_after_ctrl("submit: ctrl-x a")]
    fn non_printable_key_in_text_input_is_accepted(#[case] yaml: &str) {
        let overrides = bindings(yaml);

        let system_under_test = Keymap::new(&overrides);

        assert_eq!(system_under_test.err(), None);
    }

    #[test]
    fn printable_key_outside_text_input_is_accepted() {
        let overrides = bindings("confirm-yes: j");

        let system_under_test = Keymap::new(&overrides);

        assert_eq!(system_under_test.err(), None);
    }

    #[test]
    fn collects_all_errors_by_context() {
        let overrides = bindings("submit: a\nundo: q\nhelp: g");
        let expected = vec![
            KeymapError::Conflict {
                context: Context::Lists,
                sequence: sequence("q"),
                first: Action::Quit,
                second: Action::Undo,
            },
            KeymapError::PrefixConflict {
                context: Context::Lists,
                prefix: sequence("g"),
                prefix_action: Action::Help,
                sequence: sequence("g g"),
                action: Action::MoveToTop,
            },
            KeymapError::Conflict {
                context: Context::Tasks,
                sequence: sequence("q"),
                first: Action::Quit,
                second: Action::Undo,
            },
            KeymapError::PrefixConflict {
                context: Context::Tasks,
                prefix: sequence("g"),
                prefix_action: Action::Help,
                sequence: sequence("g g"),
                action: Action::MoveToTop,
            },
            KeymapError::PrintableInInput {
                action: Action::Submit,
                sequence: sequence("a"),
            },
        ];

        let system_under_test = Keymap::new(&overrides);

        assert_eq!(system_under_test.err(), Some(expected));
    }

    #[rstest]
    #[case::conflict(
        KeymapError::Conflict {
            context: Context::Lists,
            sequence: sequence("q"),
            first: Action::Quit,
            second: Action::Undo,
        },
        "`q` is bound to both `quit` and `undo` in the lists pane; rebind or unbind (`[]`) one \
         of them"
    )]
    #[case::prefix_conflict(
        KeymapError::PrefixConflict {
            context: Context::Tasks,
            prefix: sequence("g"),
            prefix_action: Action::Help,
            sequence: sequence("g g"),
            action: Action::MoveToTop,
        },
        "`g` of `help` is the beginning of `g g` of `move-to-top` in the tasks pane; rebind or \
         unbind (`[]`) one of them"
    )]
    #[case::printable_in_input(
        KeymapError::PrintableInInput { action: Action::Submit, sequence: sequence("a") },
        "`a` of `submit` starts with a key that types text in the text input; use a key with \
         ctrl or alt or a named key such as `enter`"
    )]
    fn error_names_the_actions_and_the_key(#[case] error: KeymapError, #[case] expected: &str) {
        let system_under_test = error.to_string();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::empty_map("{}", KeyBindings::default())]
    #[case::several(
        "quit: [q, ctrl-q]\nsearch: /",
        KeyBindings::from_iter([
            (Action::Quit, sequences(&["q", "ctrl-q"])),
            (Action::Search, sequences(&["/"])),
        ])
    )]
    fn deserializes_key_bindings(#[case] yaml: &str, #[case] expected: KeyBindings) {
        let system_under_test = bindings(yaml);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::unknown_action("teleport: t", "expected one of help, quit, focus-next")]
    #[case::invalid_key("quit: ctrl-foo", "unknown key `foo`")]
    #[case::invalid_key_in_list("quit: [q, ctrl-foo]", "unknown key `foo`")]
    #[case::null_keys("quit: ~", "a key sequence or a list of key sequences")]
    #[case::nested_map("quit: {key: q}", "a key sequence or a list of key sequences")]
    fn rejects_invalid_key_bindings(#[case] yaml: &str, #[case] expected_message: &str) {
        let system_under_test = serde_saphyr::from_str::<KeyBindings>(yaml)
            .unwrap_err()
            .to_string();

        assert!(
            system_under_test.contains(expected_message),
            "{system_under_test}"
        );
    }
}
