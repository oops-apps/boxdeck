//! Shared core for the Boxdeck app and `boxdeck-mcp`. Must never depend on Tauri.

pub mod audit;
pub mod config;
pub mod error;
pub mod jobs;
pub mod machines;
pub mod policy;
pub mod process;
pub mod pty;
pub mod vagrant;
pub mod validate;
pub mod watch;

pub use error::{Error, Result};

/// Crate version, shared by the app and the MCP binary.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver() {
        assert_eq!(VERSION.split('.').count(), 3);
    }
}
