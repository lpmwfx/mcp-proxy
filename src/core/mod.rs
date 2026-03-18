//! Core layer (_core) — supervisor, watcher, respawn logic.

use std::process::ExitStatus;
use tokio::process::Child;

use crate::shared::{ProxyError_x, ProxyResult_x};

/// struct `SupervisorCore_core`.
pub struct SupervisorCore_core;

impl SupervisorCore_core {
    /// fn `await_child_exit`.
    pub async fn await_child_exit(child: &mut Child) -> ProxyResult_x<ExitStatus> {
        child.wait().await.map_err(ProxyError_x::KillFailed)
    }
}
