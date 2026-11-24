//! Fingerprints of file contents, used to notice changes made by other programs.

use std::hash::{DefaultHasher, Hasher};

/// A cheap summary of a file's bytes: contents with different fingerprints differ.
///
/// A fingerprint combines the length and a hash of the content. Fingerprints never leave the
/// process, so the hash does not need to be stable across Rust versions.
///
/// # Examples
///
/// ```
/// use rudo::storage::fingerprint::Fingerprint;
///
/// assert_eq!(Fingerprint::of(b"- [ ] a\n"), Fingerprint::of(b"- [ ] a\n"));
/// assert_ne!(Fingerprint::of(b"- [ ] a\n"), Fingerprint::of(b"- [x] a\n"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint {
    length: usize,
    hash: u64,
}

impl Fingerprint {
    /// Computes the fingerprint of `content`.
    #[must_use]
    pub fn of(content: &[u8]) -> Self {
        let mut hasher = DefaultHasher::new();
        hasher.write(content);
        Self {
            length: content.len(),
            hash: hasher.finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn equal_content_has_equal_fingerprints() {
        let content = "- [ ] Buy milk\n".as_bytes();
        let expected = Fingerprint::of(content);

        let system_under_test = Fingerprint::of(content);

        assert_eq!(system_under_test, expected);
    }

    #[rstest]
    #[case::same_length("- [ ] Buy milk\n", "- [x] Buy milk\n")]
    #[case::appended_line("- [ ] a\n", "- [ ] a\n- [ ] b\n")]
    #[case::empty_and_newline("", "\n")]
    #[case::line_ending("- [ ] a\n", "- [ ] a\r\n")]
    fn different_content_has_different_fingerprints(#[case] first: &str, #[case] second: &str) {
        let other = Fingerprint::of(second.as_bytes());

        let system_under_test = Fingerprint::of(first.as_bytes());

        assert_ne!(system_under_test, other);
    }
}
