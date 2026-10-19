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
        let core = VERSION.split_once('-').map_or(VERSION, |(core, _)| core);
        let parts: Vec<_> = core.split('.').collect();
        assert_eq!(parts.len(), 3, "{VERSION}");
        assert!(parts.iter().all(|p| p.parse::<u64>().is_ok()), "{VERSION}");
    }
}
