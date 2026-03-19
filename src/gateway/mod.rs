//! Gateway layer (_gtw) — IO: process spawn, file watch, stdio relay, JSON-RPC read/write.

use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::mpsc;

use crate::shared::{
    DownstreamServer_x, JsonRpcError_x, JsonRpcId_x, JsonRpcNotification_x, JsonRpcRequest_x,
    JsonRpcResponse_x, ProxyConfig_x, ProxyError_x, ProxyEvent_x, ProxyResult_x,
};

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
    /// fn `watch_binary` — watches a binary file for modifications.
    pub async fn watch_binary(server_id: String, path: &Path, tx: mpsc::Sender<ProxyEvent_x>) {
        use notify::{Watcher, RecursiveMode, Result as NotifyResult};
        use std::sync::mpsc as sync_mpsc;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let (tx_notify, rx_notify) = sync_mpsc::channel();
        let should_stop = Arc::new(AtomicBool::new(false));
        let should_stop_clone = Arc::clone(&should_stop);

        // Spawn blocking watcher in background task
        let path = path.to_path_buf();
        let _watcher_task = tokio::task::spawn_blocking(move || {
            let mut watcher = match notify::recommended_watcher(move |res: NotifyResult<notify::Event>| {
                match res {
                    Ok(event) => {
                        if matches!(event.kind, notify::EventKind::Modify(_)) {
                            let _ = tx_notify.send(());
                        }
                    }
                    Err(_) => {}
                }
            }) {
                Ok(w) => w,
                Err(_) => return,
            };

            if let Err(_) = watcher.watch(&path, RecursiveMode::NonRecursive) {
                return;
            }

            // Keep watcher alive until stop signal
            while !should_stop_clone.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });

        // Forward events to async channel (include server id)
        while let Ok(()) = rx_notify.recv() {
            let _ = tx.send(ProxyEvent_x::BinaryChanged(server_id.clone())).await;
        }

        should_stop.store(true, Ordering::Relaxed);
    }
}

// ============================================================================
// ConfigGateway_gtw — loads config from JSON file
// ============================================================================

/// struct `ConfigGateway_gtw` — loads MCP server configuration from file.
pub struct ConfigGateway_gtw;

impl ConfigGateway_gtw {
    /// fn `load_config` — loads mcp-servers.json (or custom path).
    pub fn load_config(path: &Path) -> ProxyResult_x<ProxyConfig_x> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ProxyError_x::WatchFailed(format!("read config: {e}")))?;
        serde_json::from_str::<ProxyConfig_x>(&content)
            .map_err(|e| ProxyError_x::JsonParse(e))
    }

    /// fn `config_exists` — checks if a config file exists.
    pub fn config_exists(path: &Path) -> bool {
        path.exists() && path.is_file()
    }
}

// ============================================================================
// UpstreamGateway_gtw — reads JSON-RPC from stdin, writes responses to stdout
// ============================================================================

/// struct `UpstreamGateway_gtw` — handles communication with Claude (upstream).
pub struct UpstreamGateway_gtw {
    reader: tokio::io::Lines<BufReader<tokio::io::Stdin>>,
    writer: tokio::io::Stdout,
}

impl UpstreamGateway_gtw {
    /// fn `new` — creates a new upstream gateway wrapping stdin/stdout.
    pub fn new() -> Self {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin).lines();
        let writer = tokio::io::stdout();
        Self { reader, writer }
    }

    /// fn `read_request` — reads next JSON-RPC request, looping past blank lines.
    /// Returns Ok(None) if EOF, Ok(Some(request)) if valid request, Err on invalid JSON.
    pub async fn read_request(&mut self) -> ProxyResult_x<Option<JsonRpcRequest_x>> {
        loop {
            let line = self
                .reader
                .next_line()
                .await
                .map_err(|_| ProxyError_x::UpstreamEof)?;

            match line {
                None => return Ok(None), // EOF
                Some(l) if l.trim().is_empty() => continue, // blank line, skip
                Some(l) => {
                    let req =
                        serde_json::from_str::<JsonRpcRequest_x>(&l).map_err(ProxyError_x::JsonParse)?;
                    return Ok(Some(req));
                }
            }
        }
    }

    /// fn `send_response` — sends a JSON-RPC response to upstream.
    pub async fn send_response(&mut self, resp: JsonRpcResponse_x) -> ProxyResult_x<()> {
        let json = serde_json::to_string(&resp).map_err(ProxyError_x::JsonSerialize)?;
        self.writer.write_all(json.as_bytes()).await.map_err(ProxyError_x::RelayBroken)?;
        self.writer.write_all(b"\n").await.map_err(ProxyError_x::RelayBroken)?;
        self.writer.flush().await.map_err(ProxyError_x::RelayBroken)?;
        Ok(())
    }

    /// fn `send_notification` — sends a JSON-RPC notification to upstream.
    pub async fn send_notification(&mut self, notif: JsonRpcNotification_x) -> ProxyResult_x<()> {
        let json = serde_json::to_string(&notif).map_err(ProxyError_x::JsonSerialize)?;
        self.writer.write_all(json.as_bytes()).await.map_err(ProxyError_x::RelayBroken)?;
        self.writer.write_all(b"\n").await.map_err(ProxyError_x::RelayBroken)?;
        self.writer.flush().await.map_err(ProxyError_x::RelayBroken)?;
        Ok(())
    }
}

// ============================================================================
// DownstreamGateway_gtw — sends requests to downstream servers, reads responses
// ============================================================================

/// struct `DownstreamGateway_gtw` — handles communication with downstream MCP servers.
pub struct DownstreamGateway_gtw;

impl DownstreamGateway_gtw {
    /// fn `send_request` — sends a JSON-RPC request to downstream server and reads response.
    pub async fn send_request(
        server: &mut DownstreamServer_x,
        method: &str,
        params: Option<Value>,
        tool_call_timeout_secs: u64,
    ) -> ProxyResult_x<JsonRpcResponse_x> {
        let id = server.next_id;
        server.next_id += 1;

        let request = JsonRpcRequest_x {
            jsonrpc: "2.0".to_string(),
            id: Some(JsonRpcId_x::Num(id)),
            method: method.to_string(),
            params,
        };

        let json = serde_json::to_string(&request).map_err(ProxyError_x::JsonSerialize)?;
        server.stdin.write_all(json.as_bytes()).await.map_err(ProxyError_x::RelayBroken)?;
        server.stdin.write_all(b"\n").await.map_err(ProxyError_x::RelayBroken)?;
        server.stdin.flush().await.map_err(ProxyError_x::RelayBroken)?;

        // Read response with timeout
        let line = tokio::time::timeout(
            std::time::Duration::from_secs(tool_call_timeout_secs),
            server.stdout.next_line(),
        )
            .await
            .map_err(|_| ProxyError_x::DownstreamEof(server.id.clone()))?
            .map_err(|_| ProxyError_x::DownstreamEof(server.id.clone()))?;

        match line {
            None => Err(ProxyError_x::DownstreamEof(server.id.clone())),
            Some(l) => {
                let resp = serde_json::from_str::<JsonRpcResponse_x>(&l)
                    .map_err(ProxyError_x::JsonParse)?;
                Ok(resp)
            }
        }
    }

    /// fn `send_notification` — sends a JSON-RPC notification to downstream server.
    pub async fn send_notification(
        server: &mut DownstreamServer_x,
        method: &str,
        params: Option<Value>,
    ) -> ProxyResult_x<()> {
        let notif = JsonRpcRequest_x {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.to_string(),
            params,
        };

        let json = serde_json::to_string(&notif).map_err(ProxyError_x::JsonSerialize)?;
        server.stdin.write_all(json.as_bytes()).await.map_err(ProxyError_x::RelayBroken)?;
        server.stdin.write_all(b"\n").await.map_err(ProxyError_x::RelayBroken)?;
        server.stdin.flush().await.map_err(ProxyError_x::RelayBroken)?;
        Ok(())
    }
}

