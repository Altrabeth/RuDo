//! Tab completion of the word before the cursor from a vocabulary, such as the keywords of
//! the due date grammar.
//!
//! [`Completion`] works on a text and a cursor, not on a [`TextInput`], so that any input
//! can complete from any vocabulary; the caller applies each step to its input with
//! [`TextInput::replace_range`].

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

#[cfg(doc)]
use crate::ui::text_input::TextInput;
use crate::ui::text_input::is_whitespace;

/// Completing the word before the cursor: the words of a vocabulary that start with it and
/// which of them replaces it.
///
/// Words are runs of non-whitespace grapheme clusters, as for the editing keys of
/// [`TextInput`]. Only the part of the word before the cursor is completed; the text after
/// the cursor stays. Each [`Completion::cycle`] shows the next candidate, and after the last
/// one the typed word again, like `tab` in a shell.
///
/// # Examples
///
/// ```
/// use rudo::app::completion::Completion;
///
/// let vocabulary = ["today", "tomorrow", "friday"];
/// let mut completion = Completion::new("due To", 6, |_| vocabulary).expect("two candidates");
/// assert_eq!(completion.candidates(), ["today", "tomorrow"]);
/// assert_eq!((completion.cycle(), completion.word()), (4..6, "today"));
/// assert_eq!((completion.cycle(), completion.word()), (4..9, "tomorrow"));
/// assert_eq!((completion.cycle(), completion.word()), (4..12, "To"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Completion {
    /// Byte offset of the word in the text.
    start: usize,
    /// The word as it was typed, up to the cursor.
    typed: String,
    /// The words of the vocabulary that start with the typed word, in vocabulary order.
    candidates: Vec<&'static str>,
    /// Index into the candidates of the word shown, or `None` for the typed word.
    current: Option<usize>,
}

impl Completion {
    /// Starts completing the word that ends at `cursor`, a byte offset into `text`.
    ///
    /// `vocabulary` receives the text before the word and returns the words that can follow
    /// it, so that a grammar can offer only the words valid at that position. The candidates
    /// are those that start with the typed word, ignoring ASCII case; an empty word matches
    /// every word. The typed word is shown until the first [`Completion::cycle`].
    ///
    /// Returns `None` if no word matches or `cursor` is not a character boundary of `text`.
    #[must_use]
    pub fn new<Words>(
        text: &str,
        cursor: usize,
        vocabulary: impl FnOnce(&str) -> Words,
    ) -> Option<Self>
    where
        Words: IntoIterator<Item = &'static str>,
    {
        let before_cursor = text.get(..cursor)?;
        let start = word_start(before_cursor);
        let typed = &before_cursor[start..];
        let candidates: Vec<&'static str> = vocabulary(&before_cursor[..start])
            .into_iter()
            .filter(|word| starts_with_ignoring_case(word, typed))
            .collect();
        (!candidates.is_empty()).then(|| Self {
            start,
            typed: typed.to_owned(),
            candidates,
            current: None,
        })
    }

    /// Returns the words that complete the typed word, in vocabulary order.
    #[must_use]
    pub fn candidates(&self) -> &[&'static str] {
        &self.candidates
    }

    /// Returns the word shown in the text: the current candidate or the typed word.
    #[must_use]
    pub fn word(&self) -> &str {
        self.current
            .map_or(self.typed.as_str(), |index| self.candidates[index])
    }

    /// Shows the next candidate, or the typed word after the last candidate, and returns the
    /// byte range in the text of the word shown so far, which [`Completion::word`] replaces.
    #[must_use = "the returned range must be replaced with the new word"]
    pub fn cycle(&mut self) -> Range<usize> {
        let replaced = self.start..self.start + self.word().len();
        self.current = match self.current {
            None => Some(0),
            Some(index) => Some(index + 1).filter(|&next| next < self.candidates.len()),
        };
        replaced
    }
}

/// Returns the byte offset where the word ending at the end of `text` starts; the end of
/// `text` if it ends with whitespace or is empty.
fn word_start(text: &str) -> usize {
    text.grapheme_indices(true)
        .rev()
        .take_while(|(_, grapheme)| !is_whitespace(grapheme))
        .last()
        .map_or(text.len(), |(start, _)| start)
}

/// Returns whether `word` starts with `prefix`, ignoring ASCII case.
fn starts_with_ignoring_case(word: &str, prefix: &str) -> bool {
    word.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const WEEKDAYS: [&str; 7] = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];

    const DAYS: [&str; 7] = [
        "today", "tomorrow", "tuesday", "thursday", "friday", "monday", "sunday",
    ];

    /// Completes the word before `cursor` in `text` from [`DAYS`], or the end of `text`
    /// without a cursor, and returns the text after each of `steps` cycles.
    fn texts_after_cycles(text: &str, cursor: Option<usize>, steps: usize) -> Vec<String> {
        let mut text = text.to_owned();
        let cursor = cursor.unwrap_or(text.len());
        let mut completion = Completion::new(&text, cursor, |_| DAYS).unwrap();
        (0..steps)
            .map(|_| {
                let replaced = completion.cycle();
                text.replace_range(replaced, completion.word());
                text.clone()
            })
            .collect()
    }

    #[rstest]
    #[case::unique_prefix("fr", &["friday"])]
    #[case::ignores_case("FR", &["friday"])]
    #[case::mixed_case("Tu", &["tuesday"])]
    #[case::ambiguous_in_vocabulary_order("t", &["today", "tomorrow", "tuesday", "thursday"])]
    #[case::whole_word("today", &["today"])]
    #[case::empty_word_matches_every_word("", &DAYS)]
    #[case::after_whitespace("on ", &DAYS)]
    #[case::last_word_only("next mo", &["monday"])]
    fn candidates_start_with_the_word_before_the_cursor(
        #[case] text: &str,
        #[case] expected: &[&str],
    ) {
        let system_under_test = Completion::new(text, text.len(), |_| DAYS);

        assert_eq!(system_under_test.unwrap().candidates(), expected);
    }

    #[rstest]
    #[case::no_match("xyz")]
    #[case::longer_than_every_word("fridays")]
    #[case::multi_byte("fr\u{e9}")]
    #[case::emoji("\u{1f600}")]
    fn no_matching_word_is_no_completion(#[case] text: &str) {
        let system_under_test = Completion::new(text, text.len(), |_| DAYS);

        assert_eq!(system_under_test, None);
    }

    #[test]
    fn cursor_inside_a_character_is_no_completion() {
        let text = "\u{e9}";

        let system_under_test = Completion::new(text, 1, |_| DAYS);

        assert_eq!(system_under_test, None);
    }

    #[test]
    fn vocabulary_receives_the_text_before_the_word() {
        let text = "every 2 we";
        let mut received = String::new();

        let system_under_test = Completion::new(text, text.len(), |preceding| {
            received.push_str(preceding);
            ["weeks"]
        });

        assert_eq!(system_under_test.unwrap().candidates(), ["weeks"]);
        assert_eq!(received, "every 2 ");
    }

    #[test]
    fn typed_word_is_shown_before_the_first_cycle() {
        let text = "Fr";

        let system_under_test = Completion::new(text, text.len(), |_| WEEKDAYS).unwrap();

        assert_eq!(system_under_test.word(), "Fr");
    }

    #[test]
    fn cycling_goes_through_the_candidates_and_back_to_the_typed_word() {
        let expected = ["today", "tomorrow", "tuesday", "thursday", "T", "today"];

        let system_under_test = texts_after_cycles("T", None, 6);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::keeps_the_words_before("due fr", None, "due friday")]
    #[case::keeps_the_text_after_the_cursor("fr next", Some(2), "friday next")]
    #[case::completes_only_up_to_the_cursor("fr|day", Some(2), "friday|day")]
    #[case::multi_byte_before("\u{e9}t\u{e9} fr", None, "\u{e9}t\u{e9} friday")]
    #[case::combining_mark_before("e\u{301} su", None, "e\u{301} sunday")]
    #[case::wide_characters_before("\u{65e5}\u{672c} mo", None, "\u{65e5}\u{672c} monday")]
    fn completion_replaces_only_the_word_before_the_cursor(
        #[case] text: &str,
        #[case] cursor: Option<usize>,
        #[case] expected: &str,
    ) {
        let system_under_test = texts_after_cycles(text, cursor, 1);

        assert_eq!(system_under_test, [expected]);
    }

    #[rstest]
    #[case::empty("", 0)]
    #[case::word("friday", 0)]
    #[case::second_word("next fr", 5)]
    #[case::trailing_space("next ", 5)]
    #[case::tab("a\tb", 2)]
    #[case::space_with_a_combining_mark_joins_words("a \u{301}b", 0)]
    fn word_starts_after_the_last_whitespace(#[case] text: &str, #[case] expected: usize) {
        let system_under_test = word_start(text);

        assert_eq!(system_under_test, expected);
    }
}
