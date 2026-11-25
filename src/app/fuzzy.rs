//! Fuzzy matching for pickers, like fzf: a [`FuzzyQuery`] matches a text that contains its
//! characters in order, possibly with gaps, and [`FuzzyMatcher::score`] ranks the match and
//! returns the matched characters for highlighting.
//!
//! The scoring is that of `nucleo-matcher` (the matcher of the Helix editor), which prefers
//! consecutive characters and characters at the start of words and path components. Case
//! follows vim's smart case like every filter of RuDo: a query without capital letters
//! ignores case.

use std::ops::Range;

use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Added to the score of a match inside the name of a candidate, so that it ranks before any
/// match that needs the rest of the text; `nucleo-matcher` scores fit into 16 bits.
const NAME_BONUS: u32 = 1 << u16::BITS;

/// A typed query, prepared once per keystroke for matching many candidates.
///
/// # Examples
///
/// ```
/// use rudo::app::fuzzy::{FuzzyMatcher, FuzzyQuery};
///
/// let mut matcher = FuzzyMatcher::default();
/// let query = FuzzyQuery::new("wpr");
/// let found = matcher.score(&query, "Work/Projects/Release", 14).expect("it matches");
/// assert_eq!(found.ranges(), [0..1, 5..7]);
/// assert!(matcher.score(&query, "Home", 0).is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyQuery {
    atom: Atom,
}

impl FuzzyQuery {
    /// Prepares `query`, which is case-sensitive only if it contains an upper-case letter.
    ///
    /// Every character is matched literally, spaces included; there is no special syntax.
    #[must_use]
    pub fn new(query: &str) -> Self {
        Self {
            atom: Atom::new(
                query,
                CaseMatching::Smart,
                Normalization::Never,
                AtomKind::Fuzzy,
                false,
            ),
        }
    }
}

/// A match of a [`FuzzyQuery`] in a text: how good it is and where the matched characters
/// are.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FuzzyMatch {
    score: u32,
    ranges: Vec<Range<usize>>,
}

impl FuzzyMatch {
    /// Returns the score; a higher score is a better match.
    #[must_use]
    pub const fn score(&self) -> u32 {
        self.score
    }

    /// Returns the byte ranges of the matched characters in the text, in order, not
    /// overlapping and with adjacent characters merged; empty for an empty query.
    #[must_use]
    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }

    /// Returns the byte ranges of the matched characters, as [`FuzzyMatch::ranges`] does,
    /// without copying them.
    #[must_use]
    pub fn into_ranges(self) -> Vec<Range<usize>> {
        self.ranges
    }
}

/// Matches queries against texts, reusing its memory between calls; keep one per picker.
#[derive(Debug, Clone)]
pub struct FuzzyMatcher {
    matcher: Matcher,
    /// The characters of a non-ASCII text, as `nucleo-matcher` reads them.
    characters: Vec<char>,
    /// The character indices of the last match.
    indices: Vec<u32>,
}

impl Default for FuzzyMatcher {
    fn default() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
            characters: Vec::new(),
            indices: Vec::new(),
        }
    }
}

impl FuzzyMatcher {
    /// Matches `query` against `text`, whose name starts at byte `name_start`, such as the
    /// last component of a path; `None` if the text does not contain the characters of the
    /// query in order.
    ///
    /// A match inside the name ranks before every match that needs the text before it, so
    /// that `rel` finds `Work/Projects/Release` before `Release Notes/Old`. A `name_start` of
    /// `0` makes the whole text the name. An empty query matches every text with score `0`.
    ///
    /// # Panics
    ///
    /// Panics if `name_start` is not a character boundary of `text`.
    #[must_use]
    pub fn score(
        &mut self,
        query: &FuzzyQuery,
        text: &str,
        name_start: usize,
    ) -> Option<FuzzyMatch> {
        if let Some(found) = self.score_part(query, &text[name_start..], name_start) {
            return Some(FuzzyMatch {
                score: found.score + NAME_BONUS,
                ..found
            });
        }
        if name_start == 0 {
            return None;
        }
        self.score_part(query, text, 0)
    }

    /// Matches `query` against `part`, a part of a text that starts at byte `offset`, and
    /// returns the ranges relative to the whole text.
    fn score_part(&mut self, query: &FuzzyQuery, part: &str, offset: usize) -> Option<FuzzyMatch> {
        self.indices.clear();
        let haystack = Utf32Str::new(part, &mut self.characters);
        let score = query
            .atom
            .indices(haystack, &mut self.matcher, &mut self.indices)?;
        self.indices.sort_unstable();
        self.indices.dedup();
        Some(FuzzyMatch {
            score: u32::from(score),
            ranges: byte_ranges(part, &self.indices, offset),
        })
    }
}

/// Converts the sorted character `indices` of `text` into byte ranges shifted by `offset`,
/// merging adjacent characters into one range.
fn byte_ranges(text: &str, indices: &[u32], offset: usize) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::with_capacity(indices.len());
    let mut wanted = indices.iter().peekable();
    for (index, (start, character)) in (0_u32..).zip(text.char_indices()) {
        if wanted.next_if_eq(&&index).is_none() {
            continue;
        }
        let range = offset + start..offset + start + character.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == range.start => last.end = range.end,
            Some(_) | None => ranges.push(range),
        }
        if wanted.peek().is_none() {
            break;
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    /// Returns the texts that `query` matches, best first, ties in the given order.
    fn ranked<'a>(query: &str, texts: &[&'a str]) -> Vec<&'a str> {
        let mut matcher = FuzzyMatcher::default();
        let query = FuzzyQuery::new(query);
        let mut found: Vec<(u32, &str)> = texts
            .iter()
            .filter_map(|text| {
                let name_start = text.rfind('/').map_or(0, |slash| slash + 1);
                matcher
                    .score(&query, text, name_start)
                    .map(|found| (found.score(), *text))
            })
            .collect();
        found.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        found.into_iter().map(|(_, text)| text).collect()
    }

    fn ranges(query: &str, text: &str, name_start: usize) -> Option<Vec<(usize, usize)>> {
        let mut matcher = FuzzyMatcher::default();
        matcher
            .score(&FuzzyQuery::new(query), text, name_start)
            .map(|found| {
                found
                    .ranges()
                    .iter()
                    .map(|range| (range.start, range.end))
                    .collect()
            })
    }

    #[rstest]
    #[case::name_before_path(
        "rel",
        &["Release Notes/Old", "Work/Projects/Release"],
        &["Work/Projects/Release", "Release Notes/Old"]
    )]
    #[case::consecutive_before_scattered(
        "gar",
        &["Gear/Archive", "Garden"],
        &["Garden", "Gear/Archive"]
    )]
    #[case::word_start_before_inside("ho", &["Photos", "Home"], &["Home", "Photos"])]
    #[case::missing_letters_drop_out("xyz", &["Home", "Work"], &[])]
    #[case::characters_in_order_only("oh", &["Home"], &[])]
    #[case::empty_query_keeps_the_order("", &["Work", "Home"], &["Work", "Home"])]
    fn better_matches_rank_first(
        #[case] query: &str,
        #[case] texts: &[&str],
        #[case] expected: &[&str],
    ) {
        let system_under_test = ranked(query, texts);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::consecutive_characters_merge("wor", "Work", 0, Some(vec![(0, 3)]))]
    #[case::gaps_split("wk", "Work", 0, Some(vec![(0, 1), (3, 4)]))]
    #[case::name_ranges_are_shifted("rel", "Work/Release", 5, Some(vec![(5, 8)]))]
    #[case::path_match_outside_the_name("wr", "Work/Release", 5, Some(vec![(0, 1), (5, 6)]))]
    #[case::empty_query_highlights_nothing("", "Work", 0, Some(vec![]))]
    #[case::multi_byte_characters("äg", "Ärger/Tag", 0, Some(vec![(0, 2), (3, 4)]))]
    #[case::multi_byte_before_the_match("ab", "ö/ab", 3, Some(vec![(3, 5)]))]
    #[case::no_match("q", "Work", 0, None)]
    fn ranges_cover_the_matched_characters(
        #[case] query: &str,
        #[case] text: &str,
        #[case] name_start: usize,
        #[case] expected: Option<Vec<(usize, usize)>>,
    ) {
        let system_under_test = ranges(query, text, name_start);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::lowercase_ignores_case("home", "HOME", true)]
    #[case::uppercase_respects_case("Home", "home", false)]
    #[case::uppercase_matches_same_case("Home", "My Home", true)]
    #[case::multi_byte_ignores_case("ärger", "ÄRGER", true)]
    #[case::multi_byte_respects_case("Ärger", "ärger", false)]
    #[case::no_normalization("a", "ä", false)]
    fn case_is_smart(#[case] query: &str, #[case] text: &str, #[case] expected: bool) {
        let mut matcher = FuzzyMatcher::default();
        let query = FuzzyQuery::new(query);

        let system_under_test = matcher.score(&query, text, 0);

        assert_eq!(system_under_test.is_some(), expected);
    }

    #[test]
    fn name_match_outscores_every_path_match() {
        let mut matcher = FuzzyMatcher::default();
        let query = FuzzyQuery::new("a");

        let system_under_test = (
            matcher.score(&query, "x/a", 2).unwrap().score(),
            matcher.score(&query, "a/x", 2).unwrap().score(),
        );

        assert!(
            system_under_test.0 > system_under_test.1,
            "{system_under_test:?}"
        );
    }
}
