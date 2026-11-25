//! The application core in the Elm architecture: the state in [`App`], the decisions in
//! [`App::update`] and the side effects in [`Effect`], performed by [`execute`], plus the rules
//! that decide what the panes show, such as filtering.
//!
//! Nothing in this module reads the terminal, the clock or the environment; only [`execute`]
//! and the reloads of [`App::reload_list`] and [`App::rescan`] touch the file system.

pub mod confirm;
pub mod editor;
pub mod effect;
pub mod entry_visibility;
pub mod filter;
pub mod help;
pub mod history;
pub mod lists;
pub mod prompt;
mod register;
mod reload;
pub mod search;
pub mod state;
pub mod status;
mod task_delete;
mod task_due;
mod task_fields;
pub mod task_input;
mod task_priority;
mod task_recurrence;
mod task_reorder;
mod task_sort;
mod task_toggle;
pub mod task_visibility;
mod task_yank;
pub mod tasks;
mod update;
mod visual;

pub use self::effect::{Effect, execute};
pub use self::state::{App, Focus, Mode};
pub use self::status::{Severity, StatusMessage};
