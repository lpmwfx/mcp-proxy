//! Handler functions for proxy management tools and tool dispatch.

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::core::{DownstreamLifecycle_core, McpServer_core, ServerRegistry_core};
use crate::gateway::{DownstreamGateway_gtw, UpstreamGateway_gtw};
use crate::shared::{JsonRpcId_x, ProxyEvent_x};

/// Handle `tools/call` — routes to proxy handlers or downstream.
pub async fn handle_tools_call(
    id: JsonRpcId_x,
    params: Option<Value>,
    registry: &mut ServerRegistry_core,
    upstream: &mut UpstreamGateway_gtw,
    event_tx: &mpsc::Sender<ProxyEvent_x>,
    init_timeout_secs: u64,
    tool_call_timeout_secs: u64,
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
            return handle_proxy_load(id, arguments, registry, upstream, event_tx, init_timeout_secs).await;
        }
        "mcp/unload" => {
            return handle_proxy_unload(id, arguments, registry, upstream).await;
        }
        "mcp/list" => {
            return handle_proxy_list(id, registry);
        }
        "mcp/restart" => {
            return handle_proxy_restart(id, arguments, registry, upstream, event_tx, init_timeout_secs).await;
        }
        _ => {}
    }

    // Route to downstream server
    match registry.resolve_tool(tool_name) {
        Some((server_id, orig_name)) => {
            let server = match registry.get_mut(&server_id) {
                Some(s) => s,
                None => return McpServer_core::error_response(id, -32603, "Server not found"),
            };

            // Build tools/call request for downstream
            let downstream_params = json!({
                "name": orig_name,
                "arguments": arguments
            });

            match DownstreamGateway_gtw::send_request(server, "tools/call", Some(downstream_params), tool_call_timeout_secs).await {
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

/// Handle `mcp/load` — spawns and initializes a downstream server.
pub async fn handle_proxy_load(
    id: JsonRpcId_x,
    args: Option<Value>,
    registry: &mut ServerRegistry_core,
    upstream: &mut UpstreamGateway_gtw,
    event_tx: &mpsc::Sender<ProxyEvent_x>,
    init_timeout_secs: u64,
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
    if registry.contains(server_id) {
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
    match DownstreamLifecycle_core::spawn_and_initialize(server_id, std::path::Path::new(binary), &server_args, init_timeout_secs, event_tx.clone()).await {
        Ok(mut server) => {
            // Spawn watcher and store handle
            let binary_path = server.binary.clone();
            let sid = server.id.clone();
            let tx = event_tx.clone();
            let watcher_handle = tokio::spawn(async move {
                use crate::gateway::WatcherGateway_gtw;
                WatcherGateway_gtw::watch_binary(sid, &binary_path, tx).await;
            });
            server.watcher_handle = Some(watcher_handle);

            registry.insert(server);

            // Send list_changed notification
            let _ = upstream
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

/// Handle `mcp/unload` — shuts down a downstream server.
pub async fn handle_proxy_unload(
    id: JsonRpcId_x,
    args: Option<Value>,
    registry: &mut ServerRegistry_core,
    upstream: &mut UpstreamGateway_gtw,
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
    match registry.remove(server_id) {
        Some(server) => {
            DownstreamLifecycle_core::shutdown(server).await;

            // Send list_changed notification
            let _ = upstream
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

/// Handle `mcp/list` — returns list of running servers with their tools.
pub fn handle_proxy_list(
    id: JsonRpcId_x,
    registry: &ServerRegistry_core,
) -> crate::shared::JsonRpcResponse_x {
    let servers = registry
        .server_list()
        .iter()
        .map(|(server_id, tool_count)| {
            // Get tool names for this server with namespace prefix
            let tool_names: Vec<String> = registry
                .get_server_tools(server_id)
                .iter()
                .map(|tool| format!("{}__{}",server_id, tool.name))
                .collect();

            json!({
                "id": server_id,
                "tool_count": tool_count,
                "tools": tool_names
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

/// Handle `mcp/restart` — restarts a downstream server.
pub async fn handle_proxy_restart(
    id: JsonRpcId_x,
    args: Option<Value>,
    registry: &mut ServerRegistry_core,
    upstream: &mut UpstreamGateway_gtw,
    event_tx: &mpsc::Sender<ProxyEvent_x>,
    init_timeout_secs: u64,
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
    match registry.remove(server_id) {
        Some(old_server) => {
            // Restart it
            match DownstreamLifecycle_core::restart(old_server, init_timeout_secs, event_tx.clone()).await {
                Ok(mut new_server) => {
                    // Respawn watcher
                    let binary_path = new_server.binary.clone();
                    let sid = new_server.id.clone();
                    let tx = event_tx.clone();
                    let handle = tokio::spawn(async move {
                        use crate::gateway::WatcherGateway_gtw;
                        WatcherGateway_gtw::watch_binary(sid, &binary_path, tx).await;
                    });
                    new_server.watcher_handle = Some(handle);

                    registry.insert(new_server);

                    // Send list_changed notification
                    let _ = upstream
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
