//! The connection between documents and the disk: scanning the workspace, saving lists,
//! managing list files and folders and watching the workspace for changes by other programs.

pub mod entries;
pub mod fingerprint;
pub mod save;
pub mod watch;
pub mod workspace;
