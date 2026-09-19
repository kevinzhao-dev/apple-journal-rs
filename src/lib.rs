//! Apple Journal operations, independent of terminal output.
pub mod cli;
mod codec;
mod export;
mod input;
mod markdown;
pub mod mcp;
pub mod model;
mod mutation;
pub mod presentation;
mod query;
mod read;
pub mod response;
mod store;
mod write;
use anyhow::Result;
use cli::Command;
use response::Response;
use std::path::Path;

pub fn execute(path: &Path, command: &Command) -> Result<Response> {
    // Normalize once for both direct library callers and CLI callers.
    let path = std::path::absolute(path)?;
    match command {
        Command::Mcp => anyhow::bail!("use mcp::serve to start the MCP transport"),
        Command::Sandbox(a) => Ok(Response::Sandbox(store::sandbox(a)?)),
        Command::Export(a) => export::run(&path, a),
        Command::Render(_)
        | Command::Write(_)
        | Command::Edit(_)
        | Command::Delete(_)
        | Command::Restore(_)
        | Command::Empty(_)
        | Command::RepairLocations(_) => write::run(&path, command),
        _ => Ok(Response::Read(read::run(&path, command)?)),
    }
}
