//! Folio filesystem and terminal operations. UI callers execute these on
//! the background executor.
pub mod blame;
pub mod buffer;
pub mod diff;
pub mod editorconfig;
pub mod fs_op;
pub mod git;
pub mod recent;
pub mod search;
pub mod session;
pub mod settings;
#[cfg(feature = "terminal")]
pub mod terminal;
#[cfg(feature = "terminal")]
pub mod terminal_keys;
#[cfg(feature = "terminal")]
pub mod terminal_mouse;
pub mod tree;
pub mod watch;
pub mod workspace;
