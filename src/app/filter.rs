//! Smart-case text matching shared by every filter: the help screen and the search in the
//! lists and tasks pane.
//!
//! As in vim with `ignorecase` and `smartcase`, a query matches regardless of case unless it
//! contains an upper-case letter.

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
        match self.case_sensitivity {
            CaseSensitivity::Sensitive => haystack.contains(&self.needle),
            CaseSensitivity::Insensitive => contains_ignoring_case(haystack, &self.needle),
        }
    }
}

/// Tells whether `haystack` contains `lowercase_needle` when compared in lower case, without
/// allocating a lower-case copy of `haystack`.
fn contains_ignoring_case(haystack: &str, lowercase_needle: &str) -> bool {
    lowercase_needle.is_empty()
        || haystack
            .char_indices()
            .any(|(start, _)| starts_with_ignoring_case(&haystack[start..], lowercase_needle))
}

fn starts_with_ignoring_case(text: &str, lowercase_needle: &str) -> bool {
    let mut lowercase_text = text.chars().flat_map(char::to_lowercase);
    lowercase_needle
        .chars()
        .all(|needle_character| lowercase_text.next() == Some(needle_character))
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
    #[case::empty("", true)]
    #[case::text("x", false)]
    #[case::uppercase("X", false)]
    fn is_empty_only_without_text(#[case] query: &str, #[case] expected: bool) {
        let system_under_test = SmartCaseQuery::new(query);

        assert_eq!(system_under_test.is_empty(), expected);
    }
}
