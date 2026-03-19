//! Adapter layer (_adp) — hub, MCP protocol main loop.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::core::{DownstreamLifecycle_core, McpServer_core, ServerRegistry_core};
use crate::gateway::{ConfigGateway_gtw, DownstreamGateway_gtw, UpstreamGateway_gtw, WatcherGateway_gtw};
use crate::shared::{JsonRpcId_x, ProxyEvent_x, ProxyError_x, ProxyResult_x};

/// struct `ProxyAdapter_adp` — MCP protocol hub.
pub struct ProxyAdapter_adp {
    upstream: UpstreamGateway_gtw,
    registry: ServerRegistry_core,
    event_rx: mpsc::Receiver<ProxyEvent_x>,
    event_tx: mpsc::Sender<ProxyEvent_x>,
    config_path: Option<PathBuf>,
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
                        let num_servers = config.servers.len();
                        tracing::info!(count = num_servers, "loading servers from config");
                        for server_cfg in config.servers {
                            let binary = Path::new(&server_cfg.binary);
                            match DownstreamLifecycle_core::spawn_and_initialize(&server_cfg.id, binary, &server_cfg.args).await {
                                Ok(server) => {
                                    let id = server.id.clone();
                                    // Spawn watcher for this server
                                    let tx = self.event_tx.clone();
                                    let server_id = id.clone();
                                    let binary_path = binary.to_path_buf();
                                    tokio::spawn(async move {
                                        WatcherGateway_gtw::watch_binary(server_id, &binary_path, tx).await;
                                    });
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
                                match DownstreamLifecycle_core::restart(old_server).await {
                                    Ok(new_server) => {
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
                        }
                        Some(ProxyEvent_x::RespawnDone(server_id)) => {
                            tracing::info!(server = %server_id, "server respawn completed");
                        }
                        None => break, // Channel closed
                    }
                }
            }
        }

        // Cleanup: shutdown all servers
        let server_ids: Vec<_> = self.registry.server_list().iter().map(|(id, _)| id.clone()).collect();
        for id in server_ids {
            if let Some(server) = self.registry.remove(&id) {
                DownstreamLifecycle_core::shutdown(server).await;
            }
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
            "tools/call" => self.handle_tools_call(id, params).await,
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

    /// fn `handle_tools_call` — routes tool calls to proxy handlers or downstream.
    async fn handle_tools_call(
        &mut self,
        id: JsonRpcId_x,
        params: Option<Value>,
    ) -> crate::shared::JsonRpcResponse_x {
        let params = match params {
            Some(p) => p,
            None => {
                return McpServer_core::error_response(id, -32602, "Missing params");
            }
        };

        let tool_name = match params.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => {
                return McpServer_core::error_response(id, -32602, "Missing tool name");
            }
        };

        let arguments = params.get("arguments").cloned();

        // Handle proxy management tools
        match tool_name {
            "mcp/load" => {
                return self.handle_proxy_load(id, arguments).await;
            }
            "mcp/unload" => {
                return self.handle_proxy_unload(id, arguments).await;
            }
            "mcp/list" => {
                return self.handle_proxy_list(id);
            }
            "mcp/restart" => {
                return self.handle_proxy_restart(id, arguments).await;
            }
            _ => {}
        }

        // Route to downstream server
        match self.registry.resolve_tool(tool_name) {
            Some((server_id, orig_name)) => {
                let server = match self.registry.get_mut(&server_id) {
                    Some(s) => s,
                    None => return McpServer_core::error_response(id, -32603, "Server not found"),
                };

                // Build tools/call request for downstream
                let downstream_params = json!({
                    "name": orig_name,
                    "arguments": arguments
                });

                match DownstreamGateway_gtw::send_request(server, "tools/call", Some(downstream_params)).await {
                    Ok(resp) => {
                        // Mirror response back to upstream (update id)
                        crate::shared::JsonRpcResponse_x {
                            jsonrpc: resp.jsonrpc,
                            id,
                            result: resp.result,
                            error: resp.error,
                        }
                    }
                    Err(e) => {
                        eprintln!("mcp-proxy: downstream error: {e}");
                        McpServer_core::error_response(id, -32603, &format!("Downstream error: {e}"))
                    }
                }
            }
            None => McpServer_core::error_response(id, -32601, "Tool not found"),
        }
    }

    /// fn `handle_proxy_restart` — restarts a downstream server.
    async fn handle_proxy_restart(
        &mut self,
        id: JsonRpcId_x,
        args: Option<Value>,
    ) -> crate::shared::JsonRpcResponse_x {
        let args = match args {
            Some(a) => a,
            None => return McpServer_core::error_response(id, -32602, "Missing arguments"),
        };

        let server_id = match args.get("id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return McpServer_core::error_response(id, -32602, "Missing server id"),
        };

        // Remove old server
        match self.registry.remove(server_id) {
            Some(old_server) => {
                // Restart it
                match DownstreamLifecycle_core::restart(old_server).await {
                    Ok(new_server) => {
                        self.registry.insert(new_server);

                        // Send list_changed notification
                        let _ = self
                            .upstream
                            .send_notification(McpServer_core::tools_list_changed_notification())
                            .await;

                        // Return success
                        crate::shared::JsonRpcResponse_x {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: Some(json!({"status": "ok"})),
                            error: None,
                        }
                    }
                    Err(e) => {
                        eprintln!("mcp-proxy: failed to restart server: {e}");
                        McpServer_core::error_response(id, -32603, &format!("Restart failed: {e}"))
                    }
                }
            }
            None => McpServer_core::error_response(id, -32603, "Server not found"),
        }
    }

    /// fn `handle_proxy_load` — spawns and initializes a downstream server.
    async fn handle_proxy_load(
        &mut self,
        id: JsonRpcId_x,
        args: Option<Value>,
    ) -> crate::shared::JsonRpcResponse_x {
        let args = match args {
            Some(a) => a,
            None => return McpServer_core::error_response(id, -32602, "Missing arguments"),
        };

        let server_id = match args.get("id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return McpServer_core::error_response(id, -32602, "Missing server id"),
        };

        let binary = match args.get("binary").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return McpServer_core::error_response(id, -32602, "Missing binary path"),
        };

        // Check if server already exists
        if self.registry.contains(server_id) {
            return McpServer_core::error_response(id, -32603, "Server already loaded");
        }

        // Parse args array
        let server_args = match args.get("args").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };

        // Spawn and initialize server
        match DownstreamLifecycle_core::spawn_and_initialize(server_id, std::path::Path::new(binary), &server_args).await {
            Ok(server) => {
                self.registry.insert(server);

                // Send list_changed notification
                let _ = self
                    .upstream
                    .send_notification(McpServer_core::tools_list_changed_notification())
                    .await;

                // Return success
                crate::shared::JsonRpcResponse_x {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(json!({"status": "ok"})),
                    error: None,
                }
            }
            Err(e) => {
                eprintln!("mcp-proxy: failed to load server: {e}");
                McpServer_core::error_response(id, -32603, &format!("Load failed: {e}"))
            }
        }
    }

    /// fn `handle_proxy_unload` — shuts down a downstream server.
    async fn handle_proxy_unload(
        &mut self,
        id: JsonRpcId_x,
        args: Option<Value>,
    ) -> crate::shared::JsonRpcResponse_x {
        let args = match args {
            Some(a) => a,
            None => return McpServer_core::error_response(id, -32602, "Missing arguments"),
        };

        let server_id = match args.get("id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return McpServer_core::error_response(id, -32602, "Missing server id"),
        };

        // Remove and shutdown server
        match self.registry.remove(server_id) {
            Some(server) => {
                DownstreamLifecycle_core::shutdown(server).await;

                // Send list_changed notification
                let _ = self
                    .upstream
                    .send_notification(McpServer_core::tools_list_changed_notification())
                    .await;

                // Return success
                crate::shared::JsonRpcResponse_x {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(json!({"status": "ok"})),
                    error: None,
                }
            }
            None => McpServer_core::error_response(id, -32603, "Server not found"),
        }
    }

    /// fn `handle_proxy_list` — returns list of running servers.
    fn handle_proxy_list(&self, id: JsonRpcId_x) -> crate::shared::JsonRpcResponse_x {
        let servers = self
            .registry
            .server_list()
            .iter()
            .map(|(server_id, tool_count)| {
                json!({
                    "id": server_id,
                    "tools": tool_count
                })
            })
            .collect::<Vec<_>>();

        crate::shared::JsonRpcResponse_x {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(json!({
                "servers": servers
            })),
            error: None,
        }
    }
}
