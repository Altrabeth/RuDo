//! The state of a fuzzy picker: a text input with a live, ranked list of the candidates that
//! match the typed text, like fzf.
//!
//! [`Picker`] knows nothing about what its candidates are; anything that implements
//! [`PickerCandidate`] can be picked, such as the folders of the folder picker. The results
//! are ranked once per change of the text, so moving the selection and drawing never match
//! again, and the widget [`PickerPopup`](crate::ui::picker::PickerPopup) draws only the rows
//! that are visible.

use std::ops::Range;

use crate::app::fuzzy::{FuzzyMatcher, FuzzyQuery};
use crate::config::keymap::key::KeyChord;
use crate::ui::text_input::{TextChange, TextInput};

/// Something a [`Picker`] offers: a text that is shown, matched and completed to.
pub trait PickerCandidate {
    /// Returns the text shown in the result list, matched against the query and put into the
    /// input by completion.
    fn text(&self) -> &str;

    /// Returns the byte offset in [`PickerCandidate::text`] where the name starts, such as
    /// the last component of a path; matches inside the name rank first. `0` makes the whole
    /// text the name.
    fn name_start(&self) -> usize {
        0
    }
}

/// A candidate that matches the typed text: which one, how good and where.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Ranked {
    /// Index into the candidates.
    candidate: usize,
    score: u32,
    /// Byte ranges of the matched characters in the text of the candidate.
    ranges: Vec<Range<usize>>,
}

/// A row of the result list: a matching candidate with its matched characters.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct PickerResult<'a, C> {
    candidate: &'a C,
    ranges: &'a [Range<usize>],
}

// Copied without requiring `C: Copy`, since only references are copied.
impl<C> Clone for PickerResult<'_, C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C> Copy for PickerResult<'_, C> {}

impl<'a, C> PickerResult<'a, C> {
    /// Returns the candidate.
    #[must_use]
    pub const fn candidate(&self) -> &'a C {
        self.candidate
    }

    /// Returns the byte ranges of the matched characters in the text of the candidate, for
    /// highlighting them.
    #[must_use]
    pub const fn ranges(&self) -> &'a [Range<usize>] {
        self.ranges
    }
}

/// The input, the candidates, the ranked results and the selected result of a picker.
#[derive(Debug, Clone)]
pub struct Picker<C> {
    input: TextInput,
    candidates: Vec<C>,
    results: Vec<Ranked>,
    /// Index into the results; `0` while there are none.
    selected: usize,
    matcher: FuzzyMatcher,
}

impl<C: PickerCandidate> Picker<C> {
    /// Opens a picker with an empty input over `candidates`, which are all listed in the
    /// given order with the first one selected.
    #[must_use]
    pub fn new(candidates: Vec<C>) -> Self {
        let mut picker = Self {
            input: TextInput::new(),
            candidates,
            results: Vec::new(),
            selected: 0,
            matcher: FuzzyMatcher::default(),
        };
        picker.rank();
        picker
    }

    /// Returns the typed text and the cursor.
    #[must_use]
    pub const fn input(&self) -> &TextInput {
        &self.input
    }

    /// Returns the number of candidates, matching or not.
    #[must_use]
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// Returns the number of candidates that match the typed text.
    #[must_use]
    pub fn result_count(&self) -> usize {
        self.results.len()
    }

    /// Returns the matching candidates, the best match first, starting at the result
    /// `offset`.
    pub fn results(&self, offset: usize) -> impl Iterator<Item = PickerResult<'_, C>> {
        self.results.iter().skip(offset).map(|ranked| PickerResult {
            candidate: &self.candidates[ranked.candidate],
            ranges: &ranked.ranges,
        })
    }

    /// Returns the position of the selected result, or `None` without results.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        (self.selected < self.results.len()).then_some(self.selected)
    }

    /// Returns the selected candidate, or `None` without results.
    #[must_use]
    pub fn selected(&self) -> Option<&C> {
        let ranked = self.results.get(self.selected)?;
        Some(&self.candidates[ranked.candidate])
    }

    /// Edits the text with a key press that no action is bound to and ranks the candidates
    /// again if the text changed; returns whether it did.
    pub fn press(&mut self, chord: KeyChord) -> TextChange {
        let change = self.input.press(chord);
        match change {
            TextChange::Changed => self.rank(),
            TextChange::Unchanged => {}
        }
        change
    }

    /// Inserts pasted text and ranks the candidates again.
    pub fn paste(&mut self, text: &str) {
        self.input.insert_str(text);
        self.rank();
    }

    /// Selects the next result; the last one stays selected.
    pub fn select_next(&mut self) {
        if self.selected + 1 < self.results.len() {
            self.selected += 1;
        }
    }

    /// Selects the previous result; the first one stays selected.
    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Replaces the text with the text of the selected candidate, like `Tab` in a shell; if
    /// the text already is that, selects the next result first, wrapping around, so that
    /// completing again cycles through the results.
    ///
    /// The results stay as they were ranked for the typed text, so that cycling goes through
    /// them; the next edit ranks again.
    pub fn complete(&mut self) {
        let Some(selected) = self.selected() else {
            return;
        };
        if selected.text() == self.input.value() {
            self.selected = (self.selected + 1) % self.results.len();
        }
        let text = self.candidates[self.results[self.selected].candidate].text();
        self.input = TextInput::with_value(text);
    }

    /// Ranks the candidates against the typed text, best first and otherwise in their given
    /// order, and selects the best match.
    fn rank(&mut self) {
        let query = FuzzyQuery::new(self.input.value());
        let matcher = &mut self.matcher;
        self.results.clear();
        self.results.extend(
            self.candidates
                .iter()
                .enumerate()
                .filter_map(|(index, candidate)| {
                    let found = matcher.score(&query, candidate.text(), candidate.name_start())?;
                    Some(Ranked {
                        candidate: index,
                        score: found.score(),
                        ranges: found.into_ranges(),
                    })
                }),
        );
        // A stable sort keeps the given order among equal scores.
        self.results
            .sort_by_key(|ranked| std::cmp::Reverse(ranked.score));
        self.selected = 0;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::config::keymap::key::{Key, Modifiers};

    impl PickerCandidate for &str {
        fn text(&self) -> &str {
            self
        }

        fn name_start(&self) -> usize {
            self.rfind('/').map_or(0, |slash| slash + 1)
        }
    }

    const FOLDERS: [&str; 5] = ["/", "Home", "Home/Garden", "Work", "Work/Projects/Release"];

    fn picker() -> Picker<&'static str> {
        Picker::new(FOLDERS.to_vec())
    }

    fn typed(text: &str) -> Picker<&'static str> {
        let mut picker = picker();
        picker.paste(text);
        picker
    }

    fn result_texts(picker: &Picker<&'static str>) -> Vec<&'static str> {
        picker
            .results(0)
            .map(|result| *result.candidate())
            .collect()
    }

    #[test]
    fn empty_input_lists_every_candidate_in_order_with_the_first_selected() {
        let expected = FOLDERS.to_vec();

        let system_under_test = picker();

        assert_eq!(result_texts(&system_under_test), expected);
        assert_eq!(system_under_test.selected(), Some(&"/"));
        assert_eq!(system_under_test.candidate_count(), 5);
    }

    #[rstest]
    #[case::best_match_first("gar", &["Home/Garden"])]
    #[case::path_match("projects", &["Work/Projects/Release"])]
    #[case::no_match("xyz", &[])]
    fn typing_ranks_the_matching_candidates(#[case] text: &str, #[case] expected: &[&str]) {
        let system_under_test = typed(text);

        assert_eq!(result_texts(&system_under_test), expected);
        assert_eq!(system_under_test.result_count(), expected.len());
    }

    #[test]
    fn typing_a_key_ranks_again_and_selects_the_best_match() {
        let mut picker = picker();
        picker.select_next();
        let expected = Some(&"Work/Projects/Release");

        let system_under_test = {
            for character in "rel".chars() {
                let chord = KeyChord::new(Key::Char(character), Modifiers::NONE);
                let _: TextChange = picker.press(chord);
            }
            picker
        };

        assert_eq!(system_under_test.selected(), expected);
        assert_eq!(system_under_test.input().value(), "rel");
    }

    #[test]
    fn unchanged_text_keeps_the_selection() {
        let mut picker = picker();
        picker.select_next();

        let system_under_test = picker.press("left".parse().unwrap());

        assert_eq!(system_under_test, TextChange::Unchanged);
        assert_eq!(picker.selected(), Some(&"Home"));
    }

    #[rstest]
    #[case::next(1, 0, Some(1))]
    #[case::stops_at_the_last(9, 0, Some(4))]
    #[case::previous(3, 1, Some(2))]
    #[case::stops_at_the_first(1, 5, Some(0))]
    fn selection_stays_within_the_results(
        #[case] next: usize,
        #[case] previous: usize,
        #[case] expected: Option<usize>,
    ) {
        let mut picker = picker();

        let system_under_test = {
            (0..next).for_each(|_| picker.select_next());
            (0..previous).for_each(|_| picker.select_previous());
            picker.selected_index()
        };

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn without_results_nothing_is_selected() {
        let mut picker = typed("xyz");

        let system_under_test = {
            picker.select_next();
            picker.select_previous();
            picker.complete();
            &picker
        };

        assert_eq!(system_under_test.selected_index(), None);
        assert_eq!(system_under_test.selected(), None);
        assert_eq!(system_under_test.input().value(), "xyz");
    }

    #[test]
    fn complete_replaces_the_text_with_the_selected_candidate() {
        let mut picker = typed("o");
        picker.select_next();
        let expected = picker.selected().copied();

        let system_under_test = {
            picker.complete();
            picker.input().value()
        };

        assert_eq!(Some(system_under_test), expected);
    }

    #[test]
    fn complete_again_cycles_through_the_results_and_wraps_around() {
        let mut picker = typed("ho");
        let results = result_texts(&picker);
        let expected: Vec<&str> = results.iter().chain(&results).copied().collect();

        let system_under_test: Vec<String> = (0..expected.len())
            .map(|_| {
                picker.complete();
                picker.input().value().to_owned()
            })
            .collect();

        assert_eq!(system_under_test, expected);
        assert_eq!(picker.result_count(), results.len());
    }

    #[test]
    fn results_start_at_the_offset_with_their_ranges() {
        let picker = typed("w");
        let expected = vec![("Work/Projects/Release", vec![(0, 1)])];

        let system_under_test: Vec<(&str, Vec<(usize, usize)>)> = picker
            .results(1)
            .map(|result| {
                let ranges = result.ranges().iter();
                let ranges = ranges.map(|range| (range.start, range.end)).collect();
                (*result.candidate(), ranges)
            })
            .collect();

        assert_eq!(system_under_test, expected);
    }
}
