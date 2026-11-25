//! Highlighting the matches of a search filter in the text of a row.
//!
//! The panes split their text into styled parts, such as tags in a task description;
//! [`Highlight::push_spans`] splits each part further where a match begins or ends, so a match
//! keeps the style of the part it is in, patched with the `search_match` style.

use std::ops::Range;

use ratatui::style::Style;
use ratatui::text::Span;

use crate::app::filter::SmartCaseQuery;
use crate::config::theme::{Slot, Theme};

/// The matches of a query in one text and the style that highlights them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Highlight {
    /// Byte ranges of the matches in the text, in order and not overlapping.
    matches: Vec<Range<usize>>,
    style: Style,
}

impl Highlight {
    /// Finds the matches of `query` in `text`; nothing is highlighted without a query.
    pub(crate) fn new(query: Option<&SmartCaseQuery>, text: &str, theme: &Theme) -> Self {
        Self {
            matches: query.map_or_default(|query| query.match_ranges(text).collect()),
            style: theme.style(Slot::SearchMatch),
        }
    }

    /// Appends `part`, the part of the text that starts at byte `offset`, to `spans` in
    /// `style`, with every match inside it patched with the highlight style.
    pub(crate) fn push_spans<'a>(
        &self,
        spans: &mut Vec<Span<'a>>,
        part: &'a str,
        offset: usize,
        style: Style,
    ) {
        let end = offset + part.len();
        let mut position = offset;
        for found in self
            .matches
            .iter()
            .filter(|found| found.start < end && found.end > offset)
        {
            let start = found.start.max(position);
            let stop = found.end.min(end);
            if start > position {
                spans.push(Span::styled(
                    &part[position - offset..start - offset],
                    style,
                ));
            }
            spans.push(Span::styled(
                &part[start - offset..stop - offset],
                style.patch(self.style),
            ));
            position = stop;
        }
        spans.push(Span::styled(&part[position - offset..], style));
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};
    use rstest::rstest;

    use super::*;

    const BASE: Style = Style::new().fg(Color::Magenta);

    /// Renders `spans` as their texts, highlighted ones in brackets.
    fn marked(spans: &[Span<'_>], highlighted: Style) -> String {
        spans
            .iter()
            .map(|span| {
                if span.style == highlighted {
                    format!("[{}]", span.content)
                } else {
                    span.content.to_string()
                }
            })
            .collect()
    }

    #[rstest]
    #[case::no_query(None, "buy milk", 0..8, "buy milk")]
    #[case::one_match(Some("milk"), "buy milk", 0..8, "buy [milk]")]
    #[case::several_matches(Some("b"), "bob", 0..3, "[b]o[b]")]
    #[case::part_after_the_start(Some("milk"), "buy milk now", 4..8, "[milk]")]
    #[case::match_cut_at_the_start_of_the_part(Some("milk"), "buy milk now", 6..12, "[lk] now")]
    #[case::match_cut_at_the_end_of_the_part(Some("milk"), "buy milk now", 0..6, "buy [mi]")]
    fn matches_inside_the_part_are_highlighted(
        #[case] query: Option<&str>,
        #[case] text: &str,
        #[case] part: Range<usize>,
        #[case] expected: &str,
    ) {
        let theme = Theme::default();
        let query = query.map(SmartCaseQuery::new);
        let highlight = Highlight::new(query.as_ref(), text, &theme);
        let highlighted = BASE.patch(theme.style(Slot::SearchMatch));
        let mut spans = Vec::new();

        let system_under_test = {
            highlight.push_spans(&mut spans, &text[part.clone()], part.start, BASE);
            marked(&spans, highlighted)
        };

        assert_eq!(system_under_test, expected);
        assert!(highlighted.add_modifier.contains(Modifier::REVERSED));
    }
}
