//! Adapter layer (_adp) — hub, MCP protocol main loop.

use std::process::ExitCode;
use serde_json::{json, Value};

use crate::core::{DownstreamLifecycle_core, McpServer_core, ServerRegistry_core};
use crate::gateway::{DownstreamGateway_gtw, UpstreamGateway_gtw};
use crate::shared::{JsonRpcId_x, ProxyError_x, ProxyResult_x};

/// struct `ProxyAdapter_adp` — MCP protocol hub.
pub struct ProxyAdapter_adp {
    upstream: UpstreamGateway_gtw,
    registry: ServerRegistry_core,
}

impl ProxyAdapter_adp {
    /// fn `new` — creates adapter with empty registry.
    pub fn new() -> Self {
        Self {
            upstream: UpstreamGateway_gtw::new(),
            registry: ServerRegistry_core::new(),
        }
    }

    /// fn `run` — main event loop: read requests, dispatch, respond, cleanup.
    pub async fn run(mut self) -> ProxyResult_x<ExitCode> {
        loop {
            // Read next JSON-RPC request from upstream
            let req = match self.upstream.read_request().await {
                Ok(None) => break, // EOF
                Ok(Some(r)) => r,
                Err(e) => {
                    eprintln!("mcp-proxy: error reading request: {e}");
                    break;
                }
            };

            // Skip notifications from upstream (no id)
            let req_id = match req.id {
                Some(id) => id,
                None => continue,
            };

            // Dispatch request
            let response = self.dispatch(req_id.clone(), &req.method, req.params).await;

            // Send response to upstream
            if let Err(e) = self.upstream.send_response(response).await {
                eprintln!("mcp-proxy: error sending response: {e}");
                break;
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
            "proxy/load" => {
                return self.handle_proxy_load(id, arguments).await;
            }
            "proxy/unload" => {
                return self.handle_proxy_unload(id, arguments).await;
            }
            "proxy/list" => {
                return self.handle_proxy_list(id);
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
