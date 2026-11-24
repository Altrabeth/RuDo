//! The imperative shell around the [`App`](crate::app::App): it owns the terminal, collects
//! events, lets the application decide, executes the effects and draws.
//!
//! The runtime holds no logic that could be decided without a terminal; that lives in the
//! application core, where it is unit tested.

pub mod event;
pub mod run;
pub mod terminal;

pub use self::run::{RuntimeError, run};
