//! The user documentation in `docs/` as test data for the guard tests that keep it in sync
//! with the code.
//!
//! A document is read with simple line matching: a section is the text below a heading up to
//! the next heading of the same or a higher level, outside fenced code blocks, and a table is
//! a header row, a separator row and the rows below it.

/// A Markdown document of `docs/`, compiled into the tests so that editing it reruns them.
pub(crate) struct Document {
    /// The path of the document in the repository, for failure messages.
    path: &'static str,
    /// The content of the document.
    text: &'static str,
}

/// `docs/keybindings.md`, with the keybinding tables and the editing keys.
pub(crate) const KEYBINDINGS: Document = Document {
    path: "docs/keybindings.md",
    text: include_str!("../docs/keybindings.md"),
};

/// `docs/configuration.md`, with the configuration example and the theme tables.
pub(crate) const CONFIGURATION: Document = Document {
    path: "docs/configuration.md",
    text: include_str!("../docs/configuration.md"),
};

/// Starts and ends a fenced code block.
const FENCE: &str = "```";

impl Document {
    /// Returns the whole content of the document.
    pub(crate) fn text(&self) -> &'static str {
        self.text
    }

    /// Returns the lines below the heading line `heading`, such as `"## Theme"`, up to the
    /// next heading of the same or a higher level.
    ///
    /// # Panics
    ///
    /// Panics if the document has no such heading.
    pub(crate) fn section(&self, heading: &str) -> Vec<&'static str> {
        let level = heading_level(heading).expect("the heading starts with `#` and a space");
        let mut lines = self.text.lines();
        assert!(
            lines.any(|line| line == heading),
            "{} has no heading {heading:?}",
            self.path
        );
        let mut in_code_block = false;
        lines
            .take_while(|line| {
                if line.starts_with(FENCE) {
                    in_code_block = !in_code_block;
                }
                in_code_block || heading_level(line).is_none_or(|other| other > level)
            })
            .collect()
    }
}

/// Returns the level of a Markdown heading line, the number of its leading `#`.
fn heading_level(line: &str) -> Option<usize> {
    let level = line.bytes().take_while(|&byte| byte == b'#').count();
    (level > 0 && line[level..].starts_with(' ')).then_some(level)
}

/// Returns the trimmed cells of the body rows of every table in `lines` whose first header
/// cell is `first_header`.
pub(crate) fn table_rows<'document>(
    lines: &[&'document str],
    first_header: &str,
) -> Vec<Vec<&'document str>> {
    lines
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| is_separator(pair[1]) && cells(pair[0]).first() == Some(&first_header))
        .flat_map(|(header, _)| {
            lines[header + 2..]
                .iter()
                .take_while(|line| line.starts_with('|'))
                .map(|line| cells(line))
        })
        .collect()
}

/// Returns the trimmed cells of a table row.
fn cells(row: &str) -> Vec<&str> {
    let inner = row.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(str::trim).collect()
}

/// Tells whether `line` is the row below a table header, such as `| --- | --- |`.
fn is_separator(line: &str) -> bool {
    line.starts_with('|')
        && cells(line)
            .iter()
            .all(|cell| !cell.is_empty() && cell.chars().all(|character| character == '-'))
}

/// Returns the text of the inline code spans in `text`, such as `g g` in ``"`g g`, `home`"``.
pub(crate) fn code_spans(text: &str) -> impl Iterator<Item = &str> {
    text.split('`').skip(1).step_by(2)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::level_one("# RuDo", Some(1))]
    #[case::level_three("### Theme", Some(3))]
    #[case::without_space("#tag", None)]
    #[case::text("text", None)]
    fn heading_level_counts_the_hashes(#[case] line: &str, #[case] expected: Option<usize>) {
        let system_under_test = heading_level(line);

        assert_eq!(system_under_test, expected);
    }

    #[test]
    fn section_ends_at_the_next_heading_of_the_same_level() {
        let heading = "## Tasks pane";
        let last_action = "`toggle-completed`";
        let next_heading = "## Visual mode (lists pane, tasks pane and bin)";

        let system_under_test = KEYBINDINGS.section(heading);

        assert!(
            system_under_test
                .iter()
                .any(|line| line.contains(last_action))
        );
        assert!(!system_under_test.contains(&next_heading));
    }

    #[test]
    fn section_does_not_end_at_comments_in_code_blocks() {
        let heading = "# Configuration";
        let subsection = "## Theme";

        let system_under_test = CONFIGURATION.section(heading);

        assert!(system_under_test.contains(&subsection));
    }

    #[test]
    fn table_rows_returns_the_body_of_matching_tables() {
        let lines = [
            "| Keys | Action |",
            "| ---- | ------ |",
            "| `a`  | `one`  |",
            "",
            "| Slot | Used for |",
            "| ---- | -------- |",
            "| `b`  | two      |",
            "",
            "| Keys | Action |",
            "| ---- | ------ |",
            "| `c`, `d` | `three` |",
        ];
        let expected = vec![vec!["`a`", "`one`"], vec!["`c`, `d`", "`three`"]];

        let system_under_test = table_rows(&lines, "Keys");

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::two_keys("`g g`, `home`", &["g g", "home"])]
    #[case::text_between("fold, else `h` or `left`", &["h", "left"])]
    #[case::none("plain text", &[])]
    fn code_spans_returns_the_quoted_text(#[case] text: &str, #[case] expected: &[&str]) {
        let system_under_test: Vec<&str> = code_spans(text).collect();

        assert_eq!(system_under_test, expected);
    }
}
