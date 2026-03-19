//! Handler functions for tool dispatch to downstream servers.

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::core::{McpServer_core, ServerRegistry_core};
use crate::gateway::{DownstreamGateway_gtw, UpstreamGateway_gtw};
use crate::shared::{JsonRpcId_x, ProxyEvent_x};

/// Handle `tools/call` — routes to downstream server only.
pub async fn handle_tools_call(
    id: JsonRpcId_x,
    params: Option<Value>,
    registry: &mut ServerRegistry_core,
    _upstream: &mut UpstreamGateway_gtw,
    _event_tx: &mpsc::Sender<ProxyEvent_x>,
    _init_timeout_secs: u64,
    tool_call_timeout_secs: u64,
) -> crate::shared::JsonRpcResponse_x {
    let params = match params {
        Some(p) => p,
        None => return McpServer_core::error_response(id, -32602, "Missing params"),
    };

    let tool_name = match params.get("name").and_then(|n| n.as_str()) {
        Some(n) => n,
        None => return McpServer_core::error_response(id, -32602, "Missing tool name"),
    };

    let arguments = params.get("arguments").cloned();

    // Route to downstream server
    match registry.resolve_tool(tool_name) {
        Some((server_id, orig_name)) => {
            let server = match registry.get_mut(&server_id) {
                Some(s) => s,
                None => return McpServer_core::error_response(id, -32603, "Server not found"),
            };

            let downstream_params = json!({
                "name": orig_name,
                "arguments": arguments
            });

            match DownstreamGateway_gtw::send_request(server, "tools/call", Some(downstream_params), tool_call_timeout_secs).await {
                Ok(resp) => crate::shared::JsonRpcResponse_x {
                    jsonrpc: resp.jsonrpc,
                    id,
                    result: resp.result,
                    error: resp.error,
                },
                Err(e) => {
                    tracing::error!(error = %e, "downstream error");
                    McpServer_core::error_response(id, -32603, &format!("Downstream error: {e}"))
                }
            }
        }
        None => McpServer_core::error_response(id, -32601, "Tool not found"),
    }
}
