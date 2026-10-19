//! MCP server for Boxdeck: the tool router and policy glue behind the stdio binary (`main.rs`).

pub mod ctx;
pub mod server;

pub use ctx::{ApprovalRequest, Approver, Ctx};
pub use server::BoxdeckServer;

/// Name the server reports to MCP clients.
pub const SERVER_NAME: &str = "boxdeck";

/// Version reported to clients; kept in lockstep with the app.
pub fn server_version() -> &'static str {
    boxdeck_core::VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_core_version() {
        assert_eq!(server_version(), boxdeck_core::VERSION);
    }
}
