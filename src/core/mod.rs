//! Core layer (_core) — supervisor, server registry, MCP protocol handling.

/// mod `registry`.
pub mod registry;
/// mod `lifecycle`.
pub mod lifecycle;

use serde_json::json;
use tokio::process::Child;

pub use registry::ServerRegistry_core;
pub use lifecycle::DownstreamLifecycle_core;

use crate::shared::{JsonRpcError_x, JsonRpcId_x, JsonRpcNotification_x, JsonRpcResponse_x, McpTool_x, ProxyError_x, ProxyResult_x};

/// struct `SupervisorCore_core`.
pub struct SupervisorCore_core;

impl SupervisorCore_core {
    /// fn `await_child_exit`.
    pub async fn await_child_exit(child: &mut Child) -> ProxyResult_x<std::process::ExitStatus> {
        child.wait().await.map_err(ProxyError_x::KillFailed)
    }
}

/// struct `McpServer_core` — handles MCP protocol requests.
pub struct McpServer_core;

impl McpServer_core {
    /// fn `proxy_tools` — returns the proxy's own management tools.
    pub fn proxy_tools() -> Vec<McpTool_x> {
        vec![
            McpTool_x {
                name: "mcp/load".to_string(),
                description: Some("Load a new MCP server".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Unique server ID" },
                        "binary": { "type": "string", "description": "Path to server binary" },
                        "args": { "type": "array", "items": { "type": "string" }, "description": "Arguments" }
                    },
                    "required": ["id", "binary"]
                }),
            },
            McpTool_x {
                name: "mcp/unload".to_string(),
                description: Some("Unload an MCP server".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Server ID to unload" }
                    },
                    "required": ["id"]
                }),
            },
            McpTool_x {
                name: "mcp/list".to_string(),
                description: Some("List all loaded servers".to_string()),
                input_schema: json!({"type": "object", "properties": {}}),
            },
            McpTool_x {
                name: "mcp/restart".to_string(),
                description: Some("Restart an MCP server".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "Server ID to restart" }
                    },
                    "required": ["id"]
                }),
            },
        ]
    }

    /// fn `handle_initialize` — returns initialization response with given server name.
    pub fn handle_initialize(id: JsonRpcId_x, server_name: &str) -> JsonRpcResponse_x {
        JsonRpcResponse_x {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": server_name,
                    "version": "0.1.0"
                },
                "capabilities": {
                    "tools": {}
                }
            })),
            error: None,
        }
    }

    /// fn `handle_tools_list` — returns all proxy + downstream tools.
    pub fn handle_tools_list(id: JsonRpcId_x, registry: &ServerRegistry_core) -> JsonRpcResponse_x {
        let mut tools = Self::proxy_tools();
        tools.extend(registry.all_tools_namespaced());

        JsonRpcResponse_x {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(json!({
                "tools": tools
            })),
            error: None,
        }
    }

    /// fn `error_response` — returns a JSON-RPC error response.
    pub fn error_response(id: JsonRpcId_x, code: i32, msg: &str) -> JsonRpcResponse_x {
        JsonRpcResponse_x {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError_x {
                code,
                message: msg.to_string(),
                data: None,
            }),
        }
    }

    /// fn `tools_list_changed_notification` — returns a list_changed notification.
    pub fn tools_list_changed_notification() -> JsonRpcNotification_x {
        JsonRpcNotification_x {
            jsonrpc: "2.0".to_string(),
            method: "notifications/tools/list_changed".to_string(),
            params: Some(json!({})),
        }
    }
}
