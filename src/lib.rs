//! RuDo: a keyboard-driven terminal user interface for todo lists stored as Markdown files.
//!
//! The library holds all program logic so that it can be unit tested and documented; the
//! `rudo` binary only wires the pieces together. See `docs/ARCHITECTURE.md` for the module
//! layout and the data flow.

pub mod cli;
pub mod domain;
pub mod logging;
pub mod paths;
pub mod ui;
