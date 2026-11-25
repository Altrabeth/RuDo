//! Checks every relative link and heading anchor in the Markdown files of the repository, so
//! that moving a document or renaming a heading cannot break a link unnoticed.
//!
//! Links are found with simple line matching: an inline link `[text](target)` outside fenced
//! code blocks and code spans. Anchors are the heading ids GitHub generates.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use rstest::rstest;

/// Starts and ends a fenced code block.
const FENCE: &str = "```";

/// Folders that hold no documentation: build output, and hidden folders such as `.git`.
fn is_skipped_folder(name: &str) -> bool {
    name == "target" || name.starts_with('.')
}

/// Returns every Markdown file below `folder`, sorted, leaving out skipped folders.
fn markdown_files(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if path.is_dir() {
                if !is_skipped_folder(&name) {
                    pending.push(path);
                }
            } else if path.extension().is_some_and(|extension| extension == "md") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Returns the lines of `text` outside fenced code blocks, with their line numbers from 1.
fn prose_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut in_code_block = false;
    text.lines().enumerate().filter_map(move |(index, line)| {
        if line.trim_start().starts_with(FENCE) {
            in_code_block = !in_code_block;
            return None;
        }
        (!in_code_block).then_some((index + 1, line))
    })
}

/// Returns the anchor GitHub generates for a heading text: lower case, without punctuation
/// other than `-` and `_`, spaces replaced by `-`.
fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric() || matches!(character, ' ' | '-' | '_'))
        .map(|character| if character == ' ' { '-' } else { character })
        .collect()
}

/// Returns the anchors of every heading in `text`; a repeated heading gets `-1`, `-2` and so
/// on appended, as on GitHub.
fn anchors(text: &str) -> HashSet<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    prose_lines(text)
        .filter_map(|(_, line)| {
            let level = line.bytes().take_while(|&byte| byte == b'#').count();
            ((1..=6).contains(&level) && line[level..].starts_with(' '))
                .then(|| slug(&line[level..]))
        })
        .map(|anchor| {
            let count = counts.entry(anchor.clone()).or_default();
            let unique = if *count == 0 {
                anchor
            } else {
                format!("{anchor}-{count}")
            };
            *count += 1;
            unique
        })
        .collect()
}

/// Returns the targets of the inline links in `line`, leaving out code spans.
fn link_targets(line: &str) -> Vec<String> {
    let prose: String = line.split('`').step_by(2).collect::<Vec<_>>().join(" ");
    prose
        .split("](")
        .skip(1)
        .filter_map(|rest| rest.split_once(')').map(|(target, _)| target))
        .map(|target| {
            let target = target.trim();
            let target = match target.strip_prefix('<') {
                Some(enclosed) => enclosed
                    .split_once('>')
                    .map_or(enclosed, |(inner, _)| inner),
                None => target.split_whitespace().next().unwrap_or_default(),
            };
            target.to_owned()
        })
        .collect()
}

/// Tells whether `target` leaves the repository, such as a web address.
fn is_external(target: &str) -> bool {
    target.contains("://") || target.starts_with("mailto:")
}

/// Returns every broken relative link in the Markdown files below `root` as
/// `file:line: target`.
fn broken_links(root: &Path) -> Vec<String> {
    let mut anchors_by_file: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut broken = Vec::new();
    for file in markdown_files(root) {
        let text = fs::read_to_string(&file).unwrap();
        let folder = file.parent().unwrap();
        for (line_number, line) in prose_lines(&text) {
            for target in link_targets(line) {
                if is_external(&target) {
                    continue;
                }
                let (path, anchor) = target.split_once('#').unwrap_or((&target, ""));
                let linked = match path {
                    "" => file.clone(),
                    absolute if absolute.starts_with('/') => root.join(&absolute[1..]),
                    relative => folder.join(relative),
                };
                let resolves = linked.exists()
                    && (anchor.is_empty()
                        || linked.extension().is_none_or(|extension| extension != "md")
                        || anchors_by_file
                            .entry(linked.canonicalize().unwrap())
                            .or_insert_with(|| anchors(&fs::read_to_string(&linked).unwrap()))
                            .contains(anchor));
                if !resolves {
                    let shown = file.strip_prefix(root).unwrap().display();
                    broken.push(format!("{shown}:{line_number}: {target}"));
                }
            }
        }
    }
    broken
}

#[test]
fn every_relative_link_and_anchor_in_markdown_resolves() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let expected: Vec<String> = Vec::new();

    let system_under_test = broken_links(root);

    assert_eq!(system_under_test, expected);
}

#[rstest]
#[case::words(" Using RuDo with Obsidian", "using-rudo-with-obsidian")]
#[case::parentheses(" Bin (lists pane)", "bin-lists-pane")]
#[case::comma(
    " Visual mode (lists pane, tasks pane and bin)",
    "visual-mode-lists-pane-tasks-pane-and-bin"
)]
#[case::dots(" .deb, .rpm and Arch packages", "deb-rpm-and-arch-packages")]
#[case::code_span(" `rudo config`", "rudo-config")]
#[case::underscore(" snake_case", "snake_case")]
fn slug_follows_github(#[case] heading: &str, #[case] expected: &str) {
    let system_under_test = slug(heading);

    assert_eq!(system_under_test, expected);
}

#[test]
fn anchors_number_repeated_headings_and_skip_code_blocks() {
    let text = "# Notes\n\n## Notes\n\n```sh\n# comment\n```\n#tag\n";
    let expected = HashSet::from(["notes".to_owned(), "notes-1".to_owned()]);

    let system_under_test = anchors(text);

    assert_eq!(system_under_test, expected);
}

#[rstest]
#[case::two_links("see [a](a.md) and [b](b.md#part)", &["a.md", "b.md#part"])]
#[case::title("[a](a.md \"Title\")", &["a.md"])]
#[case::angle_brackets("[a](<a b.md>)", &["a b.md"])]
#[case::code_span("`[a](a.md)` and [b](b.md)", &["b.md"])]
#[case::image("![screen](shot.png)", &["shot.png"])]
#[case::none("plain [text] (not a link)", &[])]
fn link_targets_finds_inline_links(#[case] line: &str, #[case] expected: &[&str]) {
    let system_under_test = link_targets(line);

    assert_eq!(system_under_test, expected);
}

#[rstest]
#[case::web("https://github.com/Altrabeth/RuDo", true)]
#[case::mail("mailto:someone@example.com", true)]
#[case::file("docs/README.md", false)]
#[case::anchor("#installation", false)]
fn is_external_recognises_web_addresses(#[case] target: &str, #[case] expected: bool) {
    let system_under_test = is_external(target);

    assert_eq!(system_under_test, expected);
}
