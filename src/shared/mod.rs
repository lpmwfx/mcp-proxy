//! Shared layer (_x) — errors, results, shared traits.

use std::io;

#[derive(Debug)]
/// enum `ProxyEvent_x`.
pub enum ProxyEvent_x {
    BinaryChanged,
    ProcessDied,
    RespawnDone,
}

#[derive(Debug)]
/// enum `ProxyError_x`.
pub enum ProxyError_x {
    SpawnFailed(io::Error),
    KillFailed(io::Error),
    RelayBroken(io::Error),
    WatchFailed(String),
}

impl std::fmt::Display for ProxyError_x {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(e) => write!(f, "spawn failed: {e}"),
            Self::KillFailed(e) => write!(f, "kill failed: {e}"),
            Self::RelayBroken(e) => write!(f, "relay broken: {e}"),
            Self::WatchFailed(s) => write!(f, "watch failed: {s}"),
        }
    }
}

impl std::error::Error for ProxyError_x {}

/// type `ProxyResult_x`.
pub type ProxyResult_x<T> = Result<T, ProxyError_x>;
