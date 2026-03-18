//! Gateway layer (_gtw) — IO: process spawn, file watch, stdio relay.

use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::Child;
use tokio::sync::mpsc;

use crate::shared::{ProxyError_x, ProxyEvent_x, ProxyResult_x};

/// struct `ProcessGateway_gtw`.
pub struct ProcessGateway_gtw;

impl ProcessGateway_gtw {
    /// fn `spawn_downstream`.
    pub fn spawn_downstream(binary: &Path, args: &[String]) -> ProxyResult_x<Child> {
        tokio::process::Command::new(binary)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(ProxyError_x::SpawnFailed)
    }
}

/// struct `RelayGateway_gtw`.
pub struct RelayGateway_gtw;

impl RelayGateway_gtw {
    /// fn `relay`.
    pub async fn relay<R, W>(mut reader: R, mut writer: W) -> ProxyResult_x<u64>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        tokio::io::copy(&mut reader, &mut writer)
            .await
            .map_err(ProxyError_x::RelayBroken)
    }
}

/// struct `WatcherGateway_gtw`.
pub struct WatcherGateway_gtw;

impl WatcherGateway_gtw {
    /// Stub — blocks forever. Activated in phase 3.
    pub async fn watch_binary(_path: &Path, _tx: mpsc::Sender<ProxyEvent_x>) {
        std::future::pending::<()>().await;
    }
}
