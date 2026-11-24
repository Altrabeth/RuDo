//! Pure task metadata logic: due dates, recurrence, priorities and tags.
//!
//! Nothing in this module reads the clock or the file system; functions that depend on the
//! current date take it as a parameter.

pub mod date;
pub mod recurrence;
