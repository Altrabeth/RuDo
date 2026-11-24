//! Obsidian tags (`#tag`, `#area/subtag`) in task descriptions.
//!
//! A tag starts with `#` at the start of the text or after whitespace and continues with
//! letters, digits, `_`, `-` and `/`, Unicode letters included. It needs at least one
//! character that is not a digit, so `#123` is not a tag, and `C#` is none either because the
//! `#` follows a letter. As in Obsidian, inline code spans and link targets (`[x](#anchor)`)
//! never contain tags.

use std::iter;
use std::ops::Range;

/// Starts a tag.
const HASH: char = '#';
/// Opens and closes an inline code span.
const BACKTICK: char = '`';
/// Ends the text of a Markdown link; a link target in parentheses may follow.
const LINK_TEXT_END: char = ']';

/// Returns the byte ranges of the tags in `description`, including their `#`, in order.
///
/// # Examples
///
/// ```
/// use rudo::domain::tags;
///
/// let description = "Buy milk #shopping, see `#not-a-tag`";
/// let ranges: Vec<_> = tags::spans(description).collect();
/// assert_eq!(ranges, [9..18]);
/// assert_eq!(&description[9..18], "#shopping");
/// ```
pub fn spans(description: &str) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut position = 0;
    iter::from_fn(move || {
        while let Some(offset) = description[position..].find([HASH, BACKTICK, LINK_TEXT_END]) {
            let start = position + offset;
            let rest = &description[start..];
            if rest.starts_with(BACKTICK) {
                position = code_span_end(description, start);
            } else if rest.starts_with(LINK_TEXT_END) {
                position = link_target_end(description, start);
            } else {
                let end = start + HASH.len_utf8() + tag_name_len(&rest[HASH.len_utf8()..]);
                position = end;
                if is_tag(description, start..end) {
                    return Some(start..end);
                }
            }
        }
        None
    })
}

/// Returns the byte length of the run of tag characters at the start of `text`.
fn tag_name_len(text: &str) -> usize {
    text.find(|character: char| !is_tag_character(character))
        .unwrap_or(text.len())
}

fn is_tag_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-' | '/')
}

/// Tells whether the `#` and name at `range` of `text` form a tag: the `#` starts the text or
/// follows whitespace, and the name is not made of digits only.
fn is_tag(text: &str, range: Range<usize>) -> bool {
    let follows_whitespace = text[..range.start]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace);
    let name = &text[range.start + HASH.len_utf8()..range.end];
    follows_whitespace && name.chars().any(|character| !character.is_numeric())
}

/// Returns the end of the code span opened by the backticks at `start` of `text`.
///
/// A code span is closed by the next run of exactly as many backticks; without one, the
/// backticks are literal text and only they are skipped.
fn code_span_end(text: &str, start: usize) -> usize {
    let opening_len = backtick_run_len(&text[start..]);
    let mut position = start + opening_len;
    while let Some(offset) = text[position..].find(BACKTICK) {
        let run_start = position + offset;
        let run_len = backtick_run_len(&text[run_start..]);
        position = run_start + run_len;
        if run_len == opening_len {
            return position;
        }
    }
    start + opening_len
}

/// Returns the number of backticks at the start of `text`; a backtick is one byte long.
fn backtick_run_len(text: &str) -> usize {
    text.bytes().take_while(|&byte| byte == b'`').count()
}

/// Returns the end of the link target in parentheses that follows the `]` at `start` of
/// `text`, or the position after the `]` if no link target follows.
fn link_target_end(text: &str, start: usize) -> usize {
    let after_bracket = start + LINK_TEXT_END.len_utf8();
    text[after_bracket..]
        .strip_prefix('(')
        .and_then(|target| target.find(')'))
        .map_or(after_bracket, |close| {
            after_bracket + "(".len() + close + ")".len()
        })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn tag_texts(description: &str) -> Vec<&str> {
        spans(description)
            .map(|range| &description[range])
            .collect()
    }

    #[rstest]
    #[case::alone("#tag", &["#tag"])]
    #[case::after_text("Buy milk #shopping", &["#shopping"])]
    #[case::several("#a #b\t#c", &["#a", "#b", "#c"])]
    #[case::comma_ends_the_tag("#tag, more", &["#tag"])]
    #[case::period_ends_the_tag("see #tag.", &["#tag"])]
    #[case::nested("#area/subtag", &["#area/subtag"])]
    #[case::short_nested("#a/b", &["#a/b"])]
    #[case::underscore_and_dash("#to_do-later", &["#to_do-later"])]
    #[case::digits_with_letter("#y1984", &["#y1984"])]
    #[case::hash_inside_a_tag_ends_it("#a#b", &["#a"])]
    #[case::umlauts("#Küche #straße", &["#Küche", "#straße"])]
    #[case::non_latin_letters("#日本語", &["#日本語"])]
    fn tags_are_recognised(#[case] description: &str, #[case] expected: &[&str]) {
        let system_under_test = tag_texts(description);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::plain_text("Buy milk")]
    #[case::digits_only("#123")]
    #[case::after_a_letter("C#")]
    #[case::inside_a_word("C#sharp")]
    #[case::hash_alone("# ")]
    #[case::heading_hashes("## heading")]
    #[case::after_punctuation("(#tag)")]
    #[case::code_span("`#code`")]
    #[case::code_span_after_whitespace("see ` #code`")]
    #[case::double_backtick_code_span("``a ` #code``")]
    #[case::link_target("[x](#anchor)")]
    #[case::link_target_with_space("[x](<a #anchor>)")]
    fn non_tags_are_ignored(#[case] description: &str) {
        let system_under_test = tag_texts(description);

        assert_eq!(system_under_test, Vec::<&str>::new());
    }

    #[rstest]
    #[case::after_a_code_span("`code` #tag", &["#tag"])]
    #[case::unclosed_backtick("` #tag", &["#tag"])]
    #[case::unmatched_backtick_run("`` #tag `", &["#tag"])]
    #[case::after_a_link("[x](#anchor) #tag", &["#tag"])]
    #[case::bracket_without_target("[x] #tag", &["#tag"])]
    #[case::unclosed_target("[x]( #tag", &["#tag"])]
    fn tags_around_code_and_links_are_recognised(
        #[case] description: &str,
        #[case] expected: &[&str],
    ) {
        let system_under_test = tag_texts(description);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn spans_are_byte_ranges() {
        let description = "Küche #ä #b";
        let expected = [7..10, 11..13];

        let system_under_test: Vec<Range<usize>> = spans(description).collect();

        assert_eq!(system_under_test, expected);
    }
}
