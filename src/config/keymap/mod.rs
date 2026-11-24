//! Keymap: key notation, actions and the bindings between them.

pub mod action;
pub mod key;
#[expect(
    clippy::module_inception,
    reason = "the effective keymap lives in its own file next to keys and actions"
)]
pub mod keymap;
pub mod resolver;
