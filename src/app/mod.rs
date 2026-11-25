//! The application core in the Elm architecture: the state in [`App`], the decisions in
//! [`App::update`] and the side effects in [`Effect`], performed by [`execute`], plus the rules
//! that decide what the panes show, such as filtering.
//!
//! Nothing in this module reads the terminal, the clock or the environment; only [`execute`]
//! and [`App::reload_list`] touch the file system.

pub mod confirm;
pub mod editor;
pub mod effect;
pub mod filter;
pub mod help;
mod history;
pub mod lists;
pub mod prompt;
mod reload;
pub mod state;
pub mod status;
mod task_due;
pub mod task_input;
mod task_priority;
mod task_reorder;
mod task_toggle;
pub mod task_visibility;
pub mod tasks;
mod update;

pub use self::effect::{Effect, execute};
pub use self::state::{App, Focus, Mode};
pub use self::status::{Severity, StatusMessage};
