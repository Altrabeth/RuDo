//! Key notation: typed key chords and key sequences.
//!
//! A [`KeyChord`] is one key press with its modifiers (`ctrl-d`, `G`, `space`), a
//! [`KeySequence`] is one or more chords typed one after the other (`g g`). Both parse from
//! and print to the notation described in "Key notation" in `docs/keybindings.md`. Chords are
//! normalised on construction, so a chord parsed from the configuration and the same chord
//! reported by the terminal compare equal.

use std::borrow::Borrow;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};
use strum::IntoEnumIterator;

/// Notation of the space bar; a literal space separates the chords of a sequence.
const SPACE_NAME: &str = "space";

/// Separator between the modifiers and the key of a chord.
const MODIFIER_SEPARATOR: char = '-';

/// Errors that occur while parsing key notation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyNotationError {
    /// The notation, or the key after the modifiers, is empty.
    #[error("key notation is empty")]
    Empty,
    /// The key name is neither a single character nor a known key name.
    #[error("unknown key `{0}`")]
    UnknownKey(String),
    /// A prefix before `-` is not one of `ctrl`, `alt` or `shift`.
    #[error("unknown modifier `{0}`")]
    UnknownModifier(String),
    /// The same modifier is given more than once.
    #[error("modifier `{0}` is given twice")]
    DuplicateModifier(Modifier),
    /// A function key outside of `f1` to `f12`.
    #[error("function key `{0}` is not between f1 and f12")]
    InvalidFunctionKey(String),
}

/// A function key from `F1` to `F12`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionKey(u8);

impl FunctionKey {
    /// Lowest supported function key number.
    pub const MIN: u8 = 1;
    /// Highest supported function key number.
    pub const MAX: u8 = 12;

    /// Returns the function key with the given number, or `None` if `number` is not between
    /// [`Self::MIN`] and [`Self::MAX`].
    #[must_use]
    pub const fn new(number: u8) -> Option<Self> {
        if number >= Self::MIN && number <= Self::MAX {
            Some(Self(number))
        } else {
            None
        }
    }

    /// Returns the number of the function key, `1` for `F1`.
    #[must_use]
    pub const fn number(self) -> u8 {
        self.0
    }
}

/// Defaults to `F1`; required to iterate over the variants of [`Key`].
impl Default for FunctionKey {
    fn default() -> Self {
        Self(Self::MIN)
    }
}

/// A key without modifiers.
///
/// Named keys print as their lowercase variant name (`pageup`, `backtab`); the space bar is
/// `Char(' ')` and prints as `space`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, strum::EnumIter, strum::IntoStaticStr,
)]
#[strum(serialize_all = "lowercase")]
#[non_exhaustive]
pub enum Key {
    /// A character key, including the space bar.
    Char(char),
    /// The enter key.
    Enter,
    /// The escape key.
    Esc,
    /// The tab key.
    Tab,
    /// Shift and tab.
    BackTab,
    /// The backspace key.
    Backspace,
    /// The delete key.
    Delete,
    /// The insert key.
    Insert,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// The home key.
    Home,
    /// The end key.
    End,
    /// The page up key.
    PageUp,
    /// The page down key.
    PageDown,
    /// A function key.
    F(FunctionKey),
}

impl Key {
    /// Returns the key pressed together with shift, if shift changes the key itself.
    ///
    /// Terminals disagree on whether shift is reported for characters (`G`, `?`) and caps
    /// lock produces upper-case characters without shift, so the shifted character stands
    /// for itself.
    fn shifted(self) -> Option<Self> {
        match self {
            Self::Char(character) => Some(Self::Char(to_single_uppercase(character))),
            Self::Tab | Self::BackTab => Some(Self::BackTab),
            Self::Enter
            | Self::Esc
            | Self::Backspace
            | Self::Delete
            | Self::Insert
            | Self::Up
            | Self::Down
            | Self::Left
            | Self::Right
            | Self::Home
            | Self::End
            | Self::PageUp
            | Self::PageDown
            | Self::F(_) => None,
        }
    }

    fn from_key_code(code: KeyCode) -> Option<Self> {
        let key = match code {
            KeyCode::Char(character) => Self::Char(character),
            KeyCode::Enter => Self::Enter,
            KeyCode::Esc => Self::Esc,
            KeyCode::Tab => Self::Tab,
            KeyCode::BackTab => Self::BackTab,
            KeyCode::Backspace => Self::Backspace,
            KeyCode::Delete => Self::Delete,
            KeyCode::Insert => Self::Insert,
            KeyCode::Up => Self::Up,
            KeyCode::Down => Self::Down,
            KeyCode::Left => Self::Left,
            KeyCode::Right => Self::Right,
            KeyCode::Home => Self::Home,
            KeyCode::End => Self::End,
            KeyCode::PageUp => Self::PageUp,
            KeyCode::PageDown => Self::PageDown,
            KeyCode::F(number) => Self::F(FunctionKey::new(number)?),
            _ => return None,
        };
        Some(key)
    }

    /// Returns the keys that are written as their lowercase variant name (`enter`, `pageup`).
    fn named() -> impl Iterator<Item = Self> {
        Self::iter().filter(|key| !matches!(key, Self::Char(_) | Self::F(_)))
    }

    /// Returns the named key whose name is `name`.
    fn from_name(name: &str) -> Option<Self> {
        Self::named().find(|&key| <&'static str>::from(key) == name)
    }
}

/// Returns the upper-case form of `character` if it is a single character (`ß` stays `ß`).
fn to_single_uppercase(character: char) -> char {
    let mut uppercase = character.to_uppercase();
    match (uppercase.next(), uppercase.next()) {
        (Some(upper), None) => upper,
        _ => character,
    }
}

impl FromStr for Key {
    type Err = KeyNotationError;

    fn from_str(notation: &str) -> Result<Self, Self::Err> {
        if notation.is_empty() {
            return Err(KeyNotationError::Empty);
        }
        if notation == SPACE_NAME {
            return Ok(Self::Char(' '));
        }
        let mut characters = notation.chars();
        if let (Some(character), None) = (characters.next(), characters.next()) {
            return Ok(Self::Char(character));
        }
        if let Some(number) = notation
            .strip_prefix('f')
            .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return number
                .parse()
                .ok()
                .and_then(FunctionKey::new)
                .map(Self::F)
                .ok_or_else(|| KeyNotationError::InvalidFunctionKey(notation.to_owned()));
        }
        Self::from_name(notation).ok_or_else(|| KeyNotationError::UnknownKey(notation.to_owned()))
    }
}

impl Display for Key {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Char(' ') => formatter.write_str(SPACE_NAME),
            Self::Char(character) => write!(formatter, "{character}"),
            Self::F(function_key) => write!(formatter, "f{}", function_key.number()),
            named @ (Self::Enter
            | Self::Esc
            | Self::Tab
            | Self::BackTab
            | Self::Backspace
            | Self::Delete
            | Self::Insert
            | Self::Up
            | Self::Down
            | Self::Left
            | Self::Right
            | Self::Home
            | Self::End
            | Self::PageUp
            | Self::PageDown) => formatter.write_str(named.into()),
        }
    }
}

/// A modifier key held while pressing a key.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumIter, strum::EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum Modifier {
    /// The control key, written `ctrl`.
    Ctrl,
    /// The alt key, written `alt`.
    Alt,
    /// The shift key, written `shift`.
    Shift,
}

impl Modifier {
    const fn crossterm_flag(self) -> KeyModifiers {
        match self {
            Self::Ctrl => KeyModifiers::CONTROL,
            Self::Alt => KeyModifiers::ALT,
            Self::Shift => KeyModifiers::SHIFT,
        }
    }

    const fn bit(self) -> u8 {
        match self {
            Self::Ctrl => 0b001,
            Self::Alt => 0b010,
            Self::Shift => 0b100,
        }
    }
}

/// A set of [`Modifier`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    /// The empty set.
    pub const NONE: Self = Self(0);

    /// Returns this set with `modifier` added.
    #[must_use]
    pub const fn with(self, modifier: Modifier) -> Self {
        Self(self.0 | modifier.bit())
    }

    /// Returns this set with `modifier` removed.
    #[must_use]
    pub const fn without(self, modifier: Modifier) -> Self {
        Self(self.0 & !modifier.bit())
    }

    /// Returns whether `modifier` is in this set.
    #[must_use]
    pub const fn contains(self, modifier: Modifier) -> bool {
        self.0 & modifier.bit() != 0
    }

    /// Returns the modifiers in this set in their canonical order: ctrl, alt, shift.
    pub fn iter(self) -> impl Iterator<Item = Modifier> {
        Modifier::iter().filter(move |&modifier| self.contains(modifier))
    }

    /// Converts crossterm modifiers, or returns `None` if they contain a modifier RuDo does
    /// not support (super, hyper or meta), so that such a chord never triggers an action.
    fn from_crossterm(modifiers: KeyModifiers) -> Option<Self> {
        let supported: KeyModifiers = Modifier::iter().map(Modifier::crossterm_flag).collect();
        if !supported.contains(modifiers) {
            return None;
        }
        let converted = Modifier::iter()
            .filter(|modifier| modifiers.contains(modifier.crossterm_flag()))
            .collect();
        Some(converted)
    }
}

impl FromIterator<Modifier> for Modifiers {
    fn from_iter<I: IntoIterator<Item = Modifier>>(modifiers: I) -> Self {
        modifiers.into_iter().fold(Self::NONE, Self::with)
    }
}

/// One key press: a [`Key`] and the [`Modifiers`] held with it.
///
/// Chords are normalised on construction: shift is folded into characters (`shift-a` is
/// `A`, `shift-?` is `?`) and into tab (`shift-tab` is `backtab`), so that equal key presses
/// compare equal however the terminal reports them.
///
/// # Examples
///
/// ```
/// use rudo::config::keymap::key::{KeyChord, KeyNotationError};
///
/// let chord: KeyChord = "alt-ctrl-x".parse()?;
/// assert_eq!(chord.to_string(), "ctrl-alt-x");
/// assert_eq!("shift-a".parse::<KeyChord>()?, "A".parse()?);
/// # Ok::<(), KeyNotationError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyChord {
    key: Key,
    modifiers: Modifiers,
}

impl KeyChord {
    /// Creates the normalised chord for pressing `key` while holding `modifiers`.
    #[must_use]
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        let Some(shifted_key) = key
            .shifted()
            .filter(|_| modifiers.contains(Modifier::Shift))
        else {
            return Self { key, modifiers };
        };
        Self {
            key: shifted_key,
            modifiers: modifiers.without(Modifier::Shift),
        }
    }

    /// Converts a key event reported by the terminal.
    ///
    /// Returns `None` for keys that cannot be bound: lone modifier keys, media keys, function
    /// keys above `F12` and chords with the super, hyper or meta modifier. The kind of the
    /// event (press, repeat, release) is ignored.
    #[must_use]
    pub fn from_key_event(event: KeyEvent) -> Option<Self> {
        let key = Key::from_key_code(event.code)?;
        let modifiers = Modifiers::from_crossterm(event.modifiers)?;
        Some(Self::new(key, modifiers))
    }

    /// Returns the key of the chord.
    #[must_use]
    pub const fn key(self) -> Key {
        self.key
    }

    /// Returns the modifiers of the chord.
    #[must_use]
    pub const fn modifiers(self) -> Modifiers {
        self.modifiers
    }
}

/// Splits a leading `modifier-` prefix off `notation`, if there is one.
///
/// A notation without a non-empty text before the first `-` has no prefix, which keeps the
/// key `-` itself (`-`, `ctrl--`) intact.
fn split_modifier_prefix(notation: &str) -> Option<(&str, &str)> {
    notation
        .split_once(MODIFIER_SEPARATOR)
        .filter(|(prefix, _)| !prefix.is_empty())
}

impl FromStr for KeyChord {
    type Err = KeyNotationError;

    fn from_str(notation: &str) -> Result<Self, Self::Err> {
        if notation.is_empty() {
            return Err(KeyNotationError::Empty);
        }
        let mut modifiers = Modifiers::NONE;
        let mut rest = notation;
        while let Some((prefix, remainder)) = split_modifier_prefix(rest) {
            let modifier: Modifier = prefix
                .parse()
                .map_err(|_| KeyNotationError::UnknownModifier(prefix.to_owned()))?;
            if modifiers.contains(modifier) {
                return Err(KeyNotationError::DuplicateModifier(modifier));
            }
            modifiers = modifiers.with(modifier);
            rest = remainder;
        }
        let key: Key = rest.parse()?;
        Ok(Self::new(key, modifiers))
    }
}

impl Display for KeyChord {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        for modifier in self.modifiers.iter() {
            write!(formatter, "{modifier}{MODIFIER_SEPARATOR}")?;
        }
        write!(formatter, "{}", self.key)
    }
}

/// A non-empty sequence of [`KeyChord`]s typed one after the other, written with spaces
/// between the chords (`g g`, `z a`).
///
/// Sequences compare chord by chord like the slices they borrow as, so a sequence sorts
/// directly before the sequences that extend it. In a configuration file a sequence is a
/// string in key notation; a plain number such as `1` is read as the keys of its digits.
///
/// # Examples
///
/// ```
/// use rudo::config::keymap::key::{KeyNotationError, KeySequence};
///
/// let sequence: KeySequence = "g  g".parse()?;
/// assert_eq!(sequence.chords().len(), 2);
/// assert_eq!(sequence.to_string(), "g g");
/// # Ok::<(), KeyNotationError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeySequence(Vec<KeyChord>);

impl KeySequence {
    /// Creates a sequence of the given chords.
    ///
    /// # Errors
    ///
    /// Returns [`KeyNotationError::Empty`] if `chords` is empty.
    pub fn new(chords: Vec<KeyChord>) -> Result<Self, KeyNotationError> {
        if chords.is_empty() {
            return Err(KeyNotationError::Empty);
        }
        Ok(Self(chords))
    }

    /// Returns the chords of the sequence in typing order; never empty.
    #[must_use]
    pub fn chords(&self) -> &[KeyChord] {
        &self.0
    }

    /// Returns the short form of the sequence shown on the help screen and in the bottom bar.
    ///
    /// A sequence of plain characters is written without spaces, as vim users know it (`dd`,
    /// `zM`); any other sequence keeps its notation so that `ctrl-d` or `g home` stay readable.
    ///
    /// # Examples
    ///
    /// ```
    /// use rudo::config::keymap::key::{KeyNotationError, KeySequence};
    ///
    /// let fold_all: KeySequence = "z M".parse()?;
    /// let half_page_down: KeySequence = "ctrl-d".parse()?;
    /// assert_eq!(fold_all.compact().to_string(), "zM");
    /// assert_eq!(half_page_down.compact().to_string(), "ctrl-d");
    /// # Ok::<(), KeyNotationError>(())
    /// ```
    #[must_use]
    pub fn compact(&self) -> CompactKeySequence<'_> {
        CompactKeySequence(self)
    }
}

/// Displays a [`KeySequence`] in its short form; created by [`KeySequence::compact`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CompactKeySequence<'a>(&'a KeySequence);

/// Tells whether a chord is typed as one visible character without modifiers.
///
/// The space bar is excluded because a blank would be invisible in the short form.
fn is_plain_character(chord: &KeyChord) -> bool {
    chord.modifiers == Modifiers::NONE
        && matches!(chord.key, Key::Char(character) if character != ' ')
}

impl Display for CompactKeySequence<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let chords = self.0.chords();
        if chords.iter().all(is_plain_character) {
            chords
                .iter()
                .try_for_each(|chord| write!(formatter, "{}", chord.key))
        } else {
            Display::fmt(self.0, formatter)
        }
    }
}

impl FromStr for KeySequence {
    type Err = KeyNotationError;

    fn from_str(notation: &str) -> Result<Self, Self::Err> {
        let chords: Vec<KeyChord> = notation
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        Self::new(chords)
    }
}

impl Display for KeySequence {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let mut chords = self.0.iter();
        if let Some(first) = chords.next() {
            write!(formatter, "{first}")?;
        }
        chords.try_for_each(|chord| write!(formatter, " {chord}"))
    }
}

/// Lets maps keyed by sequences be queried with the chords typed so far.
impl Borrow<[KeyChord]> for KeySequence {
    fn borrow(&self) -> &[KeyChord] {
        &self.0
    }
}

impl<'de> Deserialize<'de> for KeySequence {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(KeySequenceVisitor)
    }
}

struct KeySequenceVisitor;

impl Visitor<'_> for KeySequenceVisitor {
    type Value = KeySequence;

    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter.write_str("a key sequence such as `x`, `ctrl-d` or `g g`")
    }

    fn visit_str<E: de::Error>(self, notation: &str) -> Result<Self::Value, E> {
        notation.parse().map_err(E::custom)
    }

    /// YAML reads a lone digit key such as `1` as a number.
    fn visit_u64<E: de::Error>(self, digits: u64) -> Result<Self::Value, E> {
        self.visit_str(&digits.to_string())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use ratatui::crossterm::event::KeyEventKind;
    use rstest::rstest;

    use super::*;

    fn chord(key: Key, modifiers: &[Modifier]) -> KeyChord {
        let modifiers = modifiers.iter().copied().collect();
        KeyChord { key, modifiers }
    }

    fn function_key(number: u8) -> Key {
        Key::F(FunctionKey::new(number).unwrap())
    }

    #[rstest]
    #[case::lowercase("a", chord(Key::Char('a'), &[]))]
    #[case::uppercase("A", chord(Key::Char('A'), &[]))]
    #[case::question_mark("?", chord(Key::Char('?'), &[]))]
    #[case::greater_than(">", chord(Key::Char('>'), &[]))]
    #[case::digit("1", chord(Key::Char('1'), &[]))]
    #[case::dash("-", chord(Key::Char('-'), &[]))]
    #[case::multi_byte("ä", chord(Key::Char('ä'), &[]))]
    #[case::ctrl("ctrl-d", chord(Key::Char('d'), &[Modifier::Ctrl]))]
    #[case::alt("alt-x", chord(Key::Char('x'), &[Modifier::Alt]))]
    #[case::ctrl_alt("ctrl-alt-x", chord(Key::Char('x'), &[Modifier::Ctrl, Modifier::Alt]))]
    #[case::alt_ctrl("alt-ctrl-x", chord(Key::Char('x'), &[Modifier::Ctrl, Modifier::Alt]))]
    #[case::ctrl_dash("ctrl--", chord(Key::Char('-'), &[Modifier::Ctrl]))]
    #[case::space("space", chord(Key::Char(' '), &[]))]
    #[case::ctrl_space("ctrl-space", chord(Key::Char(' '), &[Modifier::Ctrl]))]
    #[case::enter("enter", chord(Key::Enter, &[]))]
    #[case::esc("esc", chord(Key::Esc, &[]))]
    #[case::tab("tab", chord(Key::Tab, &[]))]
    #[case::backtab("backtab", chord(Key::BackTab, &[]))]
    #[case::pagedown("pagedown", chord(Key::PageDown, &[]))]
    #[case::f1("f1", chord(function_key(1), &[]))]
    #[case::f12("f12", chord(function_key(12), &[]))]
    #[case::letter_f("f", chord(Key::Char('f'), &[]))]
    #[case::shift_lowercase("shift-a", chord(Key::Char('A'), &[]))]
    #[case::shift_uppercase("shift-A", chord(Key::Char('A'), &[]))]
    #[case::shift_symbol("shift-?", chord(Key::Char('?'), &[]))]
    #[case::shift_without_uppercase("shift-ß", chord(Key::Char('ß'), &[]))]
    #[case::ctrl_shift_letter("ctrl-shift-a", chord(Key::Char('A'), &[Modifier::Ctrl]))]
    #[case::shift_tab("shift-tab", chord(Key::BackTab, &[]))]
    #[case::shift_backtab("shift-backtab", chord(Key::BackTab, &[]))]
    #[case::shift_enter("shift-enter", chord(Key::Enter, &[Modifier::Shift]))]
    #[case::shift_f5("shift-f5", chord(function_key(5), &[Modifier::Shift]))]
    fn parses_valid_chord(#[case] notation: &str, #[case] expected: KeyChord) {
        let system_under_test = notation.parse::<KeyChord>();

        assert_eq!(system_under_test, Ok(expected));
    }

    #[rstest]
    #[case::empty("", KeyNotationError::Empty)]
    #[case::modifier_without_key("ctrl-", KeyNotationError::Empty)]
    #[case::unknown_name("foo", KeyNotationError::UnknownKey("foo".to_owned()))]
    #[case::double_dash("--", KeyNotationError::UnknownKey("--".to_owned()))]
    #[case::capitalised_name("Enter", KeyNotationError::UnknownKey("Enter".to_owned()))]
    #[case::multi_character_after_modifier(
        "ctrl-ab",
        KeyNotationError::UnknownKey("ab".to_owned())
    )]
    #[case::unknown_modifier("meta-x", KeyNotationError::UnknownModifier("meta".to_owned()))]
    #[case::capitalised_modifier(
        "Ctrl-x",
        KeyNotationError::UnknownModifier("Ctrl".to_owned())
    )]
    #[case::duplicate_modifier(
        "ctrl-alt-ctrl-x",
        KeyNotationError::DuplicateModifier(Modifier::Ctrl)
    )]
    #[case::f0("f0", KeyNotationError::InvalidFunctionKey("f0".to_owned()))]
    #[case::f13("f13", KeyNotationError::InvalidFunctionKey("f13".to_owned()))]
    #[case::f_overflow("f999", KeyNotationError::InvalidFunctionKey("f999".to_owned()))]
    fn rejects_invalid_chord(#[case] notation: &str, #[case] expected: KeyNotationError) {
        let system_under_test = notation.parse::<KeyChord>();

        assert_eq!(system_under_test, Err(expected));
    }

    #[rstest]
    #[case::character(chord(Key::Char('G'), &[]), "G")]
    #[case::space(chord(Key::Char(' '), &[]), "space")]
    #[case::dash_with_ctrl(chord(Key::Char('-'), &[Modifier::Ctrl]), "ctrl--")]
    #[case::canonical_modifier_order(
        chord(Key::Up, &[Modifier::Shift, Modifier::Alt, Modifier::Ctrl]),
        "ctrl-alt-shift-up"
    )]
    #[case::backtab(chord(Key::BackTab, &[]), "backtab")]
    #[case::function_key(chord(function_key(10), &[]), "f10")]
    fn displays_chord(#[case] chord: KeyChord, #[case] expected: &str) {
        let system_under_test = chord.to_string();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn named_keys_round_trip_through_their_names() {
        let expected: Vec<Result<Key, KeyNotationError>> = Key::named().map(Ok).collect();

        let system_under_test: Vec<Result<Key, KeyNotationError>> =
            Key::named().map(|key| key.to_string().parse()).collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::single("q", vec![chord(Key::Char('q'), &[])])]
    #[case::two_chords("g g", vec![chord(Key::Char('g'), &[]); 2])]
    #[case::extra_whitespace(" z  a ", vec![chord(Key::Char('z'), &[]), chord(Key::Char('a'), &[])])]
    #[case::with_space_key(
        "ctrl-w space",
        vec![chord(Key::Char('w'), &[Modifier::Ctrl]), chord(Key::Char(' '), &[])]
    )]
    fn parses_valid_sequence(#[case] notation: &str, #[case] expected_chords: Vec<KeyChord>) {
        let expected = KeySequence::new(expected_chords).unwrap();

        let system_under_test = notation.parse::<KeySequence>();

        assert_eq!(system_under_test, Ok(expected));
    }

    #[rstest]
    #[case::empty("", KeyNotationError::Empty)]
    #[case::only_spaces("   ", KeyNotationError::Empty)]
    #[case::invalid_chord("g foo", KeyNotationError::UnknownKey("foo".to_owned()))]
    fn rejects_invalid_sequence(#[case] notation: &str, #[case] expected: KeyNotationError) {
        let system_under_test = notation.parse::<KeySequence>();

        assert_eq!(system_under_test, Err(expected));
    }

    #[test]
    fn sequence_without_chords_is_rejected() {
        let chords = Vec::new();

        let system_under_test = KeySequence::new(chords);

        assert_eq!(system_under_test, Err(KeyNotationError::Empty));
    }

    #[test]
    fn sequence_displays_chords_separated_by_spaces() {
        let sequence = KeySequence::new(vec![
            chord(Key::Char('z'), &[]),
            chord(Key::Char(' '), &[Modifier::Alt]),
        ])
        .unwrap();
        let expected = "z alt-space";

        let system_under_test = sequence.to_string();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::single_character("j", "j")]
    #[case::repeated_character("d d", "dd")]
    #[case::mixed_case("z M", "zM")]
    #[case::multi_byte("ä ö", "äö")]
    #[case::named_key("down", "down")]
    #[case::modifier("ctrl-d", "ctrl-d")]
    #[case::space("space", "space")]
    #[case::character_then_space("z space", "z space")]
    #[case::character_then_modifier("g ctrl-d", "g ctrl-d")]
    #[case::character_then_named_key("g home", "g home")]
    fn compact_form_joins_only_plain_characters(#[case] notation: &str, #[case] expected: &str) {
        let sequence: KeySequence = notation.parse().unwrap();

        let system_under_test = sequence.compact().to_string();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::character("x", "x")]
    #[case::sequence("g g", "g g")]
    #[case::quoted_modifier("\"ctrl-d\"", "ctrl-d")]
    #[case::digit("1", "1")]
    fn deserializes_sequence(#[case] yaml: &str, #[case] expected: &str) {
        let expected: KeySequence = expected.parse().unwrap();

        let system_under_test: KeySequence = serde_saphyr::from_str(yaml).unwrap();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::unknown_key("ctrl-foo", "unknown key `foo`")]
    #[case::empty("\"\"", "key notation is empty")]
    #[case::list("[x]", "a key sequence such as")]
    fn rejects_invalid_sequence_in_yaml(#[case] yaml: &str, #[case] expected_message: &str) {
        let system_under_test = serde_saphyr::from_str::<KeySequence>(yaml)
            .unwrap_err()
            .to_string();

        assert!(
            system_under_test.contains(expected_message),
            "{system_under_test}"
        );
    }

    #[rstest]
    #[case::prefix_before_extension("g", "g g")]
    #[case::extension_before_next_key("g g", "h")]
    #[case::shorter_extension_first("z a", "z a a")]
    fn sequences_sort_chord_by_chord(#[case] smaller: &str, #[case] larger: &str) {
        let smaller: KeySequence = smaller.parse().unwrap();
        let larger: KeySequence = larger.parse().unwrap();

        let system_under_test = smaller.cmp(&larger);

        assert_eq!(system_under_test, std::cmp::Ordering::Less);
    }

    #[rstest]
    #[case::zero(0, None)]
    #[case::one(1, Some(1))]
    #[case::twelve(12, Some(12))]
    #[case::thirteen(13, None)]
    fn function_key_accepts_one_to_twelve(#[case] number: u8, #[case] expected: Option<u8>) {
        let system_under_test = FunctionKey::new(number);

        assert_eq!(system_under_test.map(FunctionKey::number), expected);
    }

    #[test]
    fn modifiers_add_and_remove_every_modifier() {
        let expected: Vec<(bool, bool)> = Modifier::iter().map(|_| (true, false)).collect();

        let system_under_test: Vec<(bool, bool)> = Modifier::iter()
            .map(|modifier| {
                let added = Modifiers::NONE.with(modifier);
                (
                    added.contains(modifier),
                    added.without(modifier).contains(modifier),
                )
            })
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn modifier_names_parse_back() {
        let expected: Vec<Result<Modifier, KeyNotationError>> = Modifier::iter().map(Ok).collect();

        let system_under_test: Vec<Result<Modifier, KeyNotationError>> = Modifier::iter()
            .map(|modifier| {
                format!("{modifier}-up")
                    .parse::<KeyChord>()
                    .map(|chord| chord.modifiers().iter().next().unwrap())
            })
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::plain_character(
        KeyCode::Char('g'),
        KeyModifiers::NONE,
        Some(chord(Key::Char('g'), &[]))
    )]
    #[case::shifted_character(
        KeyCode::Char('G'),
        KeyModifiers::SHIFT,
        Some(chord(Key::Char('G'), &[]))
    )]
    #[case::caps_lock_character(
        KeyCode::Char('G'),
        KeyModifiers::NONE,
        Some(chord(Key::Char('G'), &[]))
    )]
    #[case::shifted_lowercase_character(
        KeyCode::Char('g'),
        KeyModifiers::SHIFT,
        Some(chord(Key::Char('G'), &[]))
    )]
    #[case::shifted_symbol(
        KeyCode::Char('?'),
        KeyModifiers::SHIFT,
        Some(chord(Key::Char('?'), &[]))
    )]
    #[case::ctrl_character(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL,
        Some(chord(Key::Char('d'), &[Modifier::Ctrl]))
    )]
    #[case::alt_character(
        KeyCode::Char('x'),
        KeyModifiers::ALT,
        Some(chord(Key::Char('x'), &[Modifier::Alt]))
    )]
    #[case::backtab_with_shift(
        KeyCode::BackTab,
        KeyModifiers::SHIFT,
        Some(chord(Key::BackTab, &[]))
    )]
    #[case::tab_with_shift(KeyCode::Tab, KeyModifiers::SHIFT, Some(chord(Key::BackTab, &[])))]
    #[case::shifted_arrow(KeyCode::Up, KeyModifiers::SHIFT, Some(chord(Key::Up, &[Modifier::Shift])))]
    #[case::enter(KeyCode::Enter, KeyModifiers::NONE, Some(chord(Key::Enter, &[])))]
    #[case::esc(KeyCode::Esc, KeyModifiers::NONE, Some(chord(Key::Esc, &[])))]
    #[case::backspace(KeyCode::Backspace, KeyModifiers::NONE, Some(chord(Key::Backspace, &[])))]
    #[case::delete(KeyCode::Delete, KeyModifiers::NONE, Some(chord(Key::Delete, &[])))]
    #[case::insert(KeyCode::Insert, KeyModifiers::NONE, Some(chord(Key::Insert, &[])))]
    #[case::down(KeyCode::Down, KeyModifiers::NONE, Some(chord(Key::Down, &[])))]
    #[case::left(KeyCode::Left, KeyModifiers::NONE, Some(chord(Key::Left, &[])))]
    #[case::right(KeyCode::Right, KeyModifiers::NONE, Some(chord(Key::Right, &[])))]
    #[case::home(KeyCode::Home, KeyModifiers::NONE, Some(chord(Key::Home, &[])))]
    #[case::end(KeyCode::End, KeyModifiers::NONE, Some(chord(Key::End, &[])))]
    #[case::page_up(KeyCode::PageUp, KeyModifiers::NONE, Some(chord(Key::PageUp, &[])))]
    #[case::page_down(KeyCode::PageDown, KeyModifiers::NONE, Some(chord(Key::PageDown, &[])))]
    #[case::f12(KeyCode::F(12), KeyModifiers::NONE, Some(chord(function_key(12), &[])))]
    #[case::f13(KeyCode::F(13), KeyModifiers::NONE, None)]
    #[case::null(KeyCode::Null, KeyModifiers::NONE, None)]
    #[case::caps_lock(KeyCode::CapsLock, KeyModifiers::NONE, None)]
    #[case::super_modifier(KeyCode::Char('x'), KeyModifiers::SUPER, None)]
    fn converts_key_event(
        #[case] code: KeyCode,
        #[case] modifiers: KeyModifiers,
        #[case] expected: Option<KeyChord>,
    ) {
        let event = KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Press);

        let system_under_test = KeyChord::from_key_event(event);

        assert_eq!(system_under_test, expected);
    }

    /// Terminals send `ctrl-space` as a NUL byte, which crossterm reports as a space with the
    /// control modifier, as it does for the key event of the kitty keyboard protocol.
    #[test]
    fn key_event_and_notation_agree_for_ctrl_space() {
        let event = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL);
        let expected: KeyChord = "ctrl-space".parse().unwrap();

        let system_under_test = KeyChord::from_key_event(event);

        assert_eq!(system_under_test, Some(expected));
    }

    #[test]
    fn key_event_and_notation_agree_for_shifted_tab() {
        let event = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        let expected: KeyChord = "shift-tab".parse().unwrap();

        let system_under_test = KeyChord::from_key_event(event);

        assert_eq!(system_under_test, Some(expected));
    }

    #[test]
    fn errors_are_lowercase_without_trailing_punctuation() {
        let error = KeyNotationError::DuplicateModifier(Modifier::Ctrl);
        let expected = "modifier `ctrl` is given twice";

        let system_under_test = error.to_string();

        assert_eq!(system_under_test, expected);
    }

    fn any_key() -> impl Strategy<Value = Key> {
        let printable = any::<char>()
            .prop_filter("not whitespace or control", |character| {
                !character.is_whitespace() && !character.is_control()
            })
            .prop_map(Key::Char);
        let function = (FunctionKey::MIN..=FunctionKey::MAX).prop_map(function_key);
        let named = proptest::sample::select(Key::named().collect::<Vec<Key>>());
        prop_oneof![printable, Just(Key::Char(' ')), function, named]
    }

    fn any_chord() -> impl Strategy<Value = KeyChord> {
        (
            any_key(),
            proptest::sample::subsequence(Modifier::iter().collect::<Vec<_>>(), 0..=3),
        )
            .prop_map(|(key, modifiers)| KeyChord::new(key, modifiers.into_iter().collect()))
    }

    proptest! {
        #[test]
        fn chord_display_parses_back(chord in any_chord()) {
            let system_under_test = chord.to_string().parse::<KeyChord>();

            prop_assert_eq!(system_under_test, Ok(chord));
        }

        #[test]
        fn sequence_display_parses_back(
            chords in proptest::collection::vec(any_chord(), 1..5)
        ) {
            let sequence = KeySequence::new(chords).unwrap();

            let system_under_test = sequence.to_string().parse::<KeySequence>();

            prop_assert_eq!(system_under_test, Ok(sequence));
        }
    }
}
