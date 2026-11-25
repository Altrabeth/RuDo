//! Smart-case text matching shared by every filter: the help screen and the search in the
//! lists and tasks pane.
//!
//! As in vim with `ignorecase` and `smartcase`, a query matches regardless of case unless it
//! contains an upper-case letter. [`SmartCaseWords`] applies this to every word of a query
//! on its own, all of which must match.

use std::iter;
use std::ops::Range;

/// Whether letters must match in case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
enum CaseSensitivity {
    /// Letters match regardless of case; the needle is stored in lower case.
    #[default]
    Insensitive,
    /// Letters match only in the same case.
    Sensitive,
}

/// A search query that matches text containing it, with vim's smart case.
///
/// # Examples
///
/// ```
/// use rudo::app::filter::SmartCaseQuery;
///
/// let lowercase = SmartCaseQuery::new("due");
/// assert!(lowercase.matches("Set Due Date"));
///
/// let with_capital = SmartCaseQuery::new("Due");
/// assert!(with_capital.matches("Set Due Date"));
/// assert!(!with_capital.matches("set due date"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SmartCaseQuery {
    needle: String,
    case_sensitivity: CaseSensitivity,
}

impl SmartCaseQuery {
    /// Creates the query for `query`, which is case-sensitive only if it contains an
    /// upper-case letter.
    #[must_use]
    pub fn new(query: &str) -> Self {
        if query.chars().any(char::is_uppercase) {
            Self {
                needle: query.to_owned(),
                case_sensitivity: CaseSensitivity::Sensitive,
            }
        } else {
            Self {
                needle: query.chars().flat_map(char::to_lowercase).collect(),
                case_sensitivity: CaseSensitivity::Insensitive,
            }
        }
    }

    /// Tells whether the query is empty and therefore matches everything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.needle.is_empty()
    }

    /// Tells whether `haystack` contains the query.
    #[must_use]
    pub fn matches(&self, haystack: &str) -> bool {
        self.is_empty() || self.find(haystack).is_some()
    }

    /// Returns the byte ranges of the non-overlapping occurrences of the query in `haystack`,
    /// from left to right, for highlighting them; none for an empty query.
    ///
    /// A range always covers whole characters, also where only part of the lower-case form of
    /// a character matches.
    ///
    /// # Examples
    ///
    /// ```
    /// use rudo::app::filter::SmartCaseQuery;
    ///
    /// let query = SmartCaseQuery::new("ab");
    /// let ranges: Vec<_> = query.match_ranges("AB cab").collect();
    /// assert_eq!(ranges, [0..2, 4..6]);
    /// ```
    pub fn match_ranges<'a>(&'a self, haystack: &'a str) -> impl Iterator<Item = Range<usize>> {
        let mut position = 0;
        iter::from_fn(move || {
            if self.is_empty() {
                return None;
            }
            let found = self.find(&haystack[position..])?;
            let range = position + found.start..position + found.end;
            position = range.end;
            Some(range)
        })
    }

    /// Returns the byte range of the first occurrence of the non-empty query in `haystack`.
    fn find(&self, haystack: &str) -> Option<Range<usize>> {
        match self.case_sensitivity {
            CaseSensitivity::Sensitive => haystack
                .find(&self.needle)
                .map(|start| start..start + self.needle.len()),
            CaseSensitivity::Insensitive => haystack.char_indices().find_map(|(start, _)| {
                matched_length(&haystack[start..], &self.needle).map(|length| start..start + length)
            }),
        }
    }
}

/// The words of a search query, each a [`SmartCaseQuery`] of its own, which a text matches if
/// it contains every word, in any order.
///
/// # Examples
///
/// ```
/// use rudo::app::filter::SmartCaseWords;
///
/// let words = SmartCaseWords::new("milk Buy");
/// assert!(words.matches("Buy oat milk"));
/// assert!(!words.matches("buy oat milk"));
/// assert_eq!(words.match_ranges("Buy oat milk"), [0..3, 8..12]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SmartCaseWords {
    words: Vec<SmartCaseQuery>,
}

impl SmartCaseWords {
    /// Splits `text` at whitespace into words, each case-sensitive only if it contains an
    /// upper-case letter.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            words: text.split_whitespace().map(SmartCaseQuery::new).collect(),
        }
    }

    /// Tells whether there are no words, which match everything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Tells whether `haystack` contains every word.
    #[must_use]
    pub fn matches(&self, haystack: &str) -> bool {
        self.words.iter().all(|word| word.matches(haystack))
    }

    /// Returns the byte ranges of the occurrences of every word in `haystack`, for
    /// highlighting them: in order, with overlapping and adjacent ranges merged.
    #[must_use]
    pub fn match_ranges(&self, haystack: &str) -> Vec<Range<usize>> {
        let mut ranges: Vec<Range<usize>> = self
            .words
            .iter()
            .flat_map(|word| word.match_ranges(haystack))
            .collect();
        ranges.sort_unstable_by_key(|range| range.start);
        ranges.dedup_by(|next, merged| {
            let overlaps = next.start <= merged.end;
            if overlaps {
                merged.end = merged.end.max(next.end);
            }
            overlaps
        });
        ranges
    }
}

/// Returns how many bytes at the start of `text` match `lowercase_needle` when compared in
/// lower case, or `None` if `text` does not start with it; compares without allocating a
/// lower-case copy of `text`.
pub(crate) fn matched_length(text: &str, lowercase_needle: &str) -> Option<usize> {
    let mut needle = lowercase_needle.chars().peekable();
    for (offset, character) in text.char_indices() {
        if needle.peek().is_none() {
            return Some(offset);
        }
        for lowercase in character.to_lowercase() {
            match needle.next() {
                Some(expected) if expected == lowercase => {}
                Some(_) => return None,
                None => break,
            }
        }
    }
    needle.peek().is_none().then_some(text.len())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::empty_query_matches_everything("", "move down", true)]
    #[case::empty_query_matches_empty_text("", "", true)]
    #[case::exact("down", "down", true)]
    #[case::substring("own", "move down", true)]
    #[case::lowercase_query_ignores_case("down", "Move DOWN", true)]
    #[case::uppercase_query_respects_case("Down", "move down", false)]
    #[case::uppercase_query_matches_same_case("Down", "Move Down", true)]
    #[case::multi_byte_ignores_case("ärger", "kein ÄRGER", true)]
    #[case::multi_byte_respects_case("Ä", "ärger", false)]
    #[case::digits_and_symbols_keep_case_insensitive("ctrl-d", "CTRL-D", true)]
    #[case::missing("up", "move down", false)]
    #[case::longer_than_text("move downward", "move down", false)]
    #[case::match_at_the_end("wn", "down", true)]
    fn matches_with_smart_case(#[case] query: &str, #[case] text: &str, #[case] expected: bool) {
        let query = SmartCaseQuery::new(query);

        let system_under_test = query.matches(text);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::empty_query_highlights_nothing("", "move down", &[])]
    #[case::missing("up", "move down", &[])]
    #[case::every_occurrence("o", "move down", &[(1, 2), (6, 7)])]
    #[case::ignoring_case("down", "Down DOWN", &[(0, 4), (5, 9)])]
    #[case::respecting_case("Do", "do Do", &[(3, 5)])]
    #[case::not_overlapping("aa", "aaaa a", &[(0, 2), (2, 4)])]
    #[case::multi_byte("ärger", "kein ÄRGER", &[(5, 11)])]
    #[case::partial_lowercase_form_covers_the_character("i", "\u{130}x", &[(0, 2)])]
    fn match_ranges_cover_every_occurrence(
        #[case] query: &str,
        #[case] text: &str,
        #[case] expected: &[(usize, usize)],
    ) {
        let query = SmartCaseQuery::new(query);

        let system_under_test: Vec<(usize, usize)> = query
            .match_ranges(text)
            .map(|range| (range.start, range.end))
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::no_words_match_everything("", "move down", true)]
    #[case::whitespace_only("  \t ", "move down", true)]
    #[case::every_word_in_any_order("down move", "move down", true)]
    #[case::one_word_missing("move up", "move down", false)]
    #[case::smart_case_per_word("Move down", "Move DOWN", true)]
    #[case::uppercase_word_respects_case("move Down", "move down", false)]
    fn words_match_if_the_text_contains_every_word(
        #[case] words: &str,
        #[case] text: &str,
        #[case] expected: bool,
    ) {
        let words = SmartCaseWords::new(words);

        let system_under_test = words.matches(text);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::none("", "move down", &[])]
    #[case::in_text_order("down move", "move down", &[(0, 4), (5, 9)])]
    #[case::overlapping_merged("mov ove", "move", &[(0, 4)])]
    #[case::adjacent_merged("mo ve", "move", &[(0, 4)])]
    #[case::contained_merged("move ov", "move", &[(0, 4)])]
    #[case::missing_word_highlights_the_others("move up", "move down", &[(0, 4)])]
    fn word_ranges_are_ordered_and_merged(
        #[case] words: &str,
        #[case] text: &str,
        #[case] expected: &[(usize, usize)],
    ) {
        let words = SmartCaseWords::new(words);

        let system_under_test: Vec<(usize, usize)> = words
            .match_ranges(text)
            .into_iter()
            .map(|range| (range.start, range.end))
            .collect();

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::empty("", true)]
    #[case::blank(" ", true)]
    #[case::word("x", false)]
    fn words_are_empty_only_without_a_word(#[case] words: &str, #[case] expected: bool) {
        let system_under_test = SmartCaseWords::new(words);

        assert_eq!(system_under_test.is_empty(), expected);
    }

    #[rstest]
    #[case::empty("", true)]
    #[case::text("x", false)]
    #[case::uppercase("X", false)]
    fn is_empty_only_without_text(#[case] query: &str, #[case] expected: bool) {
        let system_under_test = SmartCaseQuery::new(query);

        assert_eq!(system_under_test.is_empty(), expected);
    }
}
