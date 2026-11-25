//! Messages that fill an empty pane, such as the hint how to create the first list.
//!
//! Both panes word their hints with [`key_sentence`], so a hint always names the key the
//! keymap actually binds, and both render them with [`render_message`].

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::App;
use crate::config::keymap::action::Action;
use crate::config::keymap::key::CompactKeySequence;
use crate::config::keymap::keymap::Keymap;
use crate::config::theme::Slot;

/// Shown instead of the rows of a pane whose search filter matches nothing.
pub(crate) const NO_MATCHES: &str = "No matches";

/// Renders `message` in the style of `slot`, wrapped into `area`.
pub(crate) fn render_message(app: &App, frame: &mut Frame, area: Rect, message: &str, slot: Slot) {
    let paragraph = Paragraph::new(message)
        .style(app.theme().style(slot))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

/// Returns `sentence` with the first key bound to `action`, or asks to bind the action if it
/// is unbound.
pub(crate) fn key_sentence(
    keymap: &Keymap,
    action: Action,
    sentence: impl FnOnce(CompactKeySequence<'_>) -> String,
) -> String {
    match keymap.keys(action).first() {
        Some(keys) => sentence(keys.compact()),
        None => format!("Bind {action} in your configuration"),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::config::keymap::keymap::KeyBindings;

    fn keymap(yaml: &str) -> Keymap {
        let bindings: KeyBindings = serde_saphyr::from_str(yaml).unwrap();
        Keymap::new(&bindings).unwrap()
    }

    #[rstest]
    #[case::default("", "Press dd now")]
    #[case::first_of_several_keys("delete: [ctrl-x, d d]", "Press ctrl-x now")]
    #[case::unbound("delete: []", "Bind delete in your configuration")]
    fn key_sentence_names_the_first_bound_key(#[case] yaml: &str, #[case] expected: &str) {
        let keymap = keymap(yaml);

        let system_under_test =
            key_sentence(&keymap, Action::Delete, |keys| format!("Press {keys} now"));

        assert_eq!(system_under_test, expected);
    }
}
