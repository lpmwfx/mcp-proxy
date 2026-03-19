//! Adapter layer (_adp) — hub, MCP protocol main loop.

/// mod `handlers`.
pub mod handlers;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::core::{DownstreamLifecycle_core, McpServer_core, ServerRegistry_core};
use crate::gateway::{ConfigGateway_gtw, UpstreamGateway_gtw, WatcherGateway_gtw};
use crate::shared::{JsonRpcId_x, ProxyEvent_x, ProxyResult_x, INIT_TIMEOUT_DEFAULT_SECS, TOOL_CALL_TIMEOUT_DEFAULT_SECS};
use self::handlers::handle_tools_call;

/// struct `ProxyAdapter_adp` — MCP protocol hub.
pub struct ProxyAdapter_adp {
    upstream: UpstreamGateway_gtw,
    registry: ServerRegistry_core,
    event_rx: mpsc::Receiver<ProxyEvent_x>,
    event_tx: mpsc::Sender<ProxyEvent_x>,
    config_path: Option<PathBuf>,
    init_timeout_secs: u64,
    tool_call_timeout_secs: u64,
}

impl ProxyAdapter_adp {
    /// fn `new` — creates adapter with empty registry.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            upstream: UpstreamGateway_gtw::new(),
            registry: ServerRegistry_core::new(),
            event_rx: rx,
            event_tx: tx,
            config_path: None,
            init_timeout_secs: INIT_TIMEOUT_DEFAULT_SECS,
            tool_call_timeout_secs: TOOL_CALL_TIMEOUT_DEFAULT_SECS,
        }
    }

    /// fn `with_config` — sets the config file path.
    pub fn with_config(mut self, path: PathBuf) -> Self {
        self.config_path = Some(path);
        self
    }

    /// fn `run` — main event loop: load config, read requests, dispatch, respond, cleanup.
    pub async fn run(mut self) -> ProxyResult_x<ExitCode> {
        // Load config if provided
        if let Some(ref config_path) = self.config_path {
            if ConfigGateway_gtw::config_exists(config_path) {
                match ConfigGateway_gtw::load_config(config_path) {
                    Ok(config) => {
                        // Update timeout values from config
                        if let Some(init_timeout) = config.init_timeout_secs {
                            self.init_timeout_secs = init_timeout;
                        }
                        if let Some(tool_timeout) = config.tool_call_timeout_secs {
                            self.tool_call_timeout_secs = tool_timeout;
                        }

                        let num_servers = config.servers.len();
                        tracing::info!(count = num_servers, "loading servers from config");
                        for server_cfg in config.servers {
                            let binary = Path::new(&server_cfg.binary);
                            match DownstreamLifecycle_core::spawn_and_initialize(&server_cfg.id, binary, &server_cfg.args, self.init_timeout_secs, self.event_tx.clone()).await {
                                Ok(mut server) => {
                                    let id = server.id.clone();
                                    // Spawn watcher and store handle
                                    let binary_path = server.binary.clone();
                                    let sid = server.id.clone();
                                    let tx = self.event_tx.clone();
                                    let handle = tokio::spawn(async move {
                                        WatcherGateway_gtw::watch_binary(sid, &binary_path, tx).await;
                                    });
                                    server.watcher_handle = Some(handle);
                                    self.registry.insert(server);
                                    tracing::info!(server = %id, "server loaded and initialized");
                                }
                                Err(e) => {
                                    tracing::error!(server = %server_cfg.id, error = %e, "failed to load server");
                                }
                            }
                        }
                        // Send list_changed after auto-load
                        if !self.registry.server_list().is_empty() {
                            let _ = self.upstream.send_notification(McpServer_core::tools_list_changed_notification()).await;
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "failed to load config");
                    }
                }
            }
        }

        // Main event loop
        loop {
            tokio::select! {
                // Handle upstream JSON-RPC requests
                req_result = self.upstream.read_request() => {
                    match req_result {
                        Ok(None) => break, // EOF
                        Ok(Some(r)) => {
                            let req_id = match r.id {
                                Some(id) => id,
                                None => continue, // Skip notifications
                            };
                            let response = self.dispatch(req_id.clone(), &r.method, r.params).await;
                            if let Err(e) = self.upstream.send_response(response).await {
                                tracing::error!(error = %e, "error sending response");
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "error reading request");
                            break;
                        }
                    }
                }
                // Handle internal events (binary changed, process died, etc)
                event = self.event_rx.recv() => {
                    match event {
                        Some(ProxyEvent_x::BinaryChanged(server_id)) => {
                            tracing::info!(server = %server_id, "binary modified, restarting server");
                            // Auto-restart the server
                            if let Some(old_server) = self.registry.remove(&server_id) {
                                match DownstreamLifecycle_core::restart(old_server, self.init_timeout_secs, self.event_tx.clone()).await {
                                    Ok(mut new_server) => {
                                        // Respawn watcher
                                        let binary_path = new_server.binary.clone();
                                        let sid = new_server.id.clone();
                                        let tx = self.event_tx.clone();
                                        let handle = tokio::spawn(async move {
                                            WatcherGateway_gtw::watch_binary(sid, &binary_path, tx).await;
                                        });
                                        new_server.watcher_handle = Some(handle);
                                        self.registry.insert(new_server);
                                        // Send list_changed notification
                                        let _ = self.upstream
                                            .send_notification(McpServer_core::tools_list_changed_notification())
                                            .await;
                                        tracing::info!(server = %server_id, "server restarted successfully");
                                    }
                                    Err(e) => {
                                        tracing::error!(server = %server_id, error = %e, "failed to restart server");
                                    }
                                }
                            }
                        }

                        Some(ProxyEvent_x::ProcessDied(server_id)) => {
                            tracing::warn!(server = %server_id, "downstream process died unexpectedly");
                            if let Some(server) = self.registry.remove(&server_id) {
                                let _ = self.upstream.send_notification(McpServer_core::tools_list_changed_notification()).await;

                                let crash_count = server.crash_count + 1;
                                let backoff = Duration::from_secs(2u64.pow(crash_count.min(4)));
                                let tx = self.event_tx.clone();
                                let binary = server.binary.clone();
                                let args = server.args.clone();
                                let id = server_id.clone();
                                let init_timeout = self.init_timeout_secs;

                                tokio::spawn(async move {
                                    tokio::time::sleep(backoff).await;
                                    match DownstreamLifecycle_core::spawn_and_initialize(
                                        &id, &binary, &args, init_timeout, tx.clone()
                                    ).await {
                                        Ok(mut new_server) => {
                                            new_server.crash_count = crash_count;
                                            // Spawn watcher
                                            let binary_path = new_server.binary.clone();
                                            let sid = new_server.id.clone();
                                            let wtx = tx.clone();
                                            let whandle = tokio::spawn(async move {
                                                WatcherGateway_gtw::watch_binary(sid, &binary_path, wtx).await;
                                            });
                                            new_server.watcher_handle = Some(whandle);
                                            let _ = tx.send(ProxyEvent_x::RespawnDone(id, Box::new(new_server))).await;
                                        }
                                        Err(e) => {
                                            tracing::error!(server = %id, error = %e, "crash recovery respawn failed");
                                        }
                                    }
                                });
                            }
                        }

                        Some(ProxyEvent_x::RespawnDone(server_id, new_server)) => {
                            self.registry.insert(*new_server);
                            let _ = self.upstream.send_notification(McpServer_core::tools_list_changed_notification()).await;
                            tracing::info!(server = %server_id, "server respawned successfully");
                        }

                        None => break, // Channel closed
                    }
                }

                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("shutdown signal received, stopping");
                    break;
                }
            }
        }

        // Cleanup: shutdown all servers
        for server in self.registry.drain_all() {
            DownstreamLifecycle_core::shutdown(server).await;
        }

        Ok(ExitCode::SUCCESS)
    }

    /// fn `dispatch` — routes requests to appropriate handler.
    async fn dispatch(
        &mut self,
        id: JsonRpcId_x,
        method: &str,
        params: Option<Value>,
    ) -> crate::shared::JsonRpcResponse_x {
        match method {
            "initialize" => McpServer_core::handle_initialize(id),
            "tools/list" => McpServer_core::handle_tools_list(id, &self.registry),
            "tools/call" => handle_tools_call(id, params, &mut self.registry, &mut self.upstream, &self.event_tx, self.init_timeout_secs, self.tool_call_timeout_secs).await,
            "ping" => {
                // Respond to ping
                crate::shared::JsonRpcResponse_x {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(json!({})),
                    error: None,
                }
            }
            _ => McpServer_core::error_response(id, -32601, "Method not found"),
        }
    }

}
