//! The application core in the Elm architecture: the state in [`App`], the decisions in
//! [`App::update`] and the side effects in [`Effect`], performed by [`execute`], plus the rules
//! that decide what the panes show, such as filtering.
//!
//! Nothing in this module reads the terminal, the clock or the environment; only [`execute`]
//! touches the file system.

pub mod effect;
pub mod filter;
pub mod help;
pub mod lists;
pub mod prompt;
pub mod state;
pub mod status;
mod task_toggle;
pub mod tasks;
mod update;

pub use self::effect::{Effect, execute};
pub use self::state::{App, Focus, Mode};
pub use self::status::{Severity, StatusMessage};
