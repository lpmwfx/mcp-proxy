//! Core layer (_core) — supervisor, server registry, MCP protocol handling.

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitStatus;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;

use crate::gateway::{DownstreamGateway_gtw, ProcessGateway_gtw};
use crate::shared::{
    DownstreamServer_x, JsonRpcError_x, JsonRpcId_x, JsonRpcNotification_x, JsonRpcResponse_x,
    McpTool_x, ProxyError_x, ProxyResult_x,
};

/// struct `SupervisorCore_core`.
pub struct SupervisorCore_core;

impl SupervisorCore_core {
    /// fn `await_child_exit`.
    pub async fn await_child_exit(child: &mut Child) -> ProxyResult_x<ExitStatus> {
        child.wait().await.map_err(ProxyError_x::KillFailed)
    }
}

// ============================================================================
// ServerRegistry_core — manages active downstream servers
// ============================================================================

/// struct `ServerRegistry_core` — owns and manages all active downstream servers.
pub struct ServerRegistry_core {
    servers: HashMap<String, DownstreamServer_x>,
    tool_index: HashMap<String, String>, // "id__name" → "id"
}

impl ServerRegistry_core {
    /// fn `new` — creates an empty registry.
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
            tool_index: HashMap::new(),
        }
    }

    /// fn `insert` — adds a server and builds tool_index entries.
    pub fn insert(&mut self, server: DownstreamServer_x) {
        let server_id = server.id.clone();
        for tool in &server.tools {
            let namespaced_key = format!("{}__{}",server_id, tool.name);
            self.tool_index.insert(namespaced_key, server_id.clone());
        }
        self.servers.insert(server_id, server);
    }

    /// fn `remove` — removes a server by id.
    pub fn remove(&mut self, id: &str) -> Option<DownstreamServer_x> {
        if let Some(server) = self.servers.remove(id) {
            // Clean up tool_index entries for this server
            self.tool_index.retain(|_, v| v != id);
            Some(server)
        } else {
            None
        }
    }

    /// fn `get_mut` — borrows a server mutably.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut DownstreamServer_x> {
        self.servers.get_mut(id)
    }

    /// fn `contains` — checks if a server exists.
    pub fn contains(&self, id: &str) -> bool {
        self.servers.contains_key(id)
    }

    /// fn `all_tools_namespaced` — returns all tools with server id prefix.
    pub fn all_tools_namespaced(&self) -> Vec<McpTool_x> {
        let mut tools = Vec::new();
        for (_, server) in &self.servers {
            for tool in &server.tools {
                let mut tool = tool.clone();
                tool.name = format!("{}__{}",server.id, tool.name);
                tools.push(tool);
            }
        }
        tools
    }

    /// fn `resolve_tool` — finds server_id and original tool name from namespaced name.
    pub fn resolve_tool(&self, namespaced: &str) -> Option<(String, String)> {
        if let Some(server_id) = self.tool_index.get(namespaced) {
            let original_name = namespaced.split_once("__")?.1.to_string();
            Some((server_id.clone(), original_name))
        } else {
            None
        }
    }

    /// fn `server_list` — returns info on all running servers.
    pub fn server_list(&self) -> Vec<(String, usize)> {
        self.servers
            .iter()
            .map(|(id, server)| (id.clone(), server.tools.len()))
            .collect()
    }

    /// fn `servers_mut` — returns mutable iterator over servers.
    pub fn servers_mut(&mut self) -> impl Iterator<Item = &mut DownstreamServer_x> {
        self.servers.values_mut()
    }
}

// ============================================================================
// DownstreamLifecycle_core — spawn, initialize, shutdown downstream servers
// ============================================================================

/// struct `DownstreamLifecycle_core` — lifecycle management for downstream servers.
pub struct DownstreamLifecycle_core;

impl DownstreamLifecycle_core {
    /// fn `spawn_and_initialize` — spawns a server, initializes it, and collects tools.
    pub async fn spawn_and_initialize(
        id: &str,
        binary: &Path,
        args: &[String],
    ) -> ProxyResult_x<DownstreamServer_x> {
        // 1. Spawn the downstream process
        let mut child = ProcessGateway_gtw::spawn_downstream(binary, args)?;
        let mut stdin = child.stdin.take().expect("child stdin piped");
        let stdout = child.stdout.take().expect("child stdout piped");
        let mut stdout = BufReader::new(stdout).lines();

        // 2. Send initialize request
        let init_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1i64,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "clientInfo": {
                    "name": "mcp-proxy",
                    "version": "0.1.0"
                },
                "capabilities": {}
            }
        });

        stdin.write_all(serde_json::to_string(&init_req).unwrap().as_bytes())
            .await
            .map_err(|e| ProxyError_x::RelayBroken(e))?;
        stdin.write_all(b"\n").await.map_err(|e| ProxyError_x::RelayBroken(e))?;
        stdin.flush().await.map_err(|e| ProxyError_x::RelayBroken(e))?;

        // Read initialize response (with 5s timeout for slow servers)
        let line = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stdout.next_line(),
        )
            .await
            .map_err(|_| ProxyError_x::InitializeFailed("initialize timeout (5s)".to_string()))?
            .map_err(|_| ProxyError_x::InitializeFailed("failed to read initialize response".to_string()))?;

        let _resp = match line {
            None => return Err(ProxyError_x::InitializeFailed("EOF on initialize".to_string())),
            Some(l) => serde_json::from_str::<Value>(&l)
                .map_err(|_| ProxyError_x::InitializeFailed("invalid initialize response".to_string()))?,
        };

        // Check for error in response
        if _resp.get("error").is_some() {
            return Err(ProxyError_x::InitializeFailed(
                format!("initialize error: {}", _resp.get("error").unwrap()),
            ));
        }

        // 3. Send initialized notification (no id, no response expected)
        let initialized_notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });

        stdin.write_all(serde_json::to_string(&initialized_notif).unwrap().as_bytes())
            .await
            .map_err(|e| ProxyError_x::RelayBroken(e))?;
        stdin.write_all(b"\n").await.map_err(|e| ProxyError_x::RelayBroken(e))?;
        stdin.flush().await.map_err(|e| ProxyError_x::RelayBroken(e))?;

        // 4. Request tools/list
        let tools_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2i64,
            "method": "tools/list",
            "params": null
        });

        stdin.write_all(serde_json::to_string(&tools_req).unwrap().as_bytes())
            .await
            .map_err(|e| ProxyError_x::RelayBroken(e))?;
        stdin.write_all(b"\n").await.map_err(|e| ProxyError_x::RelayBroken(e))?;
        stdin.flush().await.map_err(|e| ProxyError_x::RelayBroken(e))?;

        // Read tools/list response (with 5s timeout for slow servers)
        let line = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stdout.next_line(),
        )
            .await
            .map_err(|_| ProxyError_x::InitializeFailed("tools/list timeout (5s)".to_string()))?
            .map_err(|_| ProxyError_x::InitializeFailed("failed to read tools/list response".to_string()))?;

        let tools_resp = match line {
            None => return Err(ProxyError_x::InitializeFailed("EOF on tools/list".to_string())),
            Some(l) => serde_json::from_str::<Value>(&l)
                .map_err(|_| ProxyError_x::InitializeFailed("invalid tools/list response".to_string()))?,
        };

        // Extract tools from result
        let mut tools = Vec::new();
        if let Some(tools_arr) = tools_resp.get("result").and_then(|r| r.get("tools")).and_then(|t| t.as_array()) {
            for tool_val in tools_arr {
                if let Ok(tool) = serde_json::from_value::<McpTool_x>(tool_val.clone()) {
                    tools.push(tool);
                }
            }
        }

        let server = DownstreamServer_x {
            id: id.to_string(),
            binary: binary.to_path_buf(),
            args: args.to_vec(),
            child,
            stdin,
            stdout,
            tools,
            next_id: 3,
        };

        Ok(server)
    }

    /// fn `shutdown` — gracefully shuts down a server.
    pub async fn shutdown(mut server: DownstreamServer_x) {
        let _ = server.child.kill().await;
        let _ = server.child.wait().await;
    }

    /// fn `restart` — kills and respawns a server, collects tools.
    pub async fn restart(mut old_server: DownstreamServer_x) -> ProxyResult_x<DownstreamServer_x> {
        // Kill old server
        let _ = old_server.child.kill().await;
        let _ = old_server.child.wait().await;

        // Respawn with same config
        Self::spawn_and_initialize(&old_server.id, &old_server.binary, &old_server.args).await
    }
}

// ============================================================================
// McpServer_core — MCP protocol handlers
// ============================================================================

/// struct `McpServer_core` — handles MCP protocol requests.
pub struct McpServer_core;

impl McpServer_core {
    /// fn `proxy_tools` — returns the proxy's own management tools.
    pub fn proxy_tools() -> Vec<McpTool_x> {
        vec![
            McpTool_x {
                name: "proxy/load".to_string(),
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
                name: "proxy/unload".to_string(),
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
                name: "proxy/list".to_string(),
                description: Some("List all loaded servers".to_string()),
                input_schema: json!({"type": "object", "properties": {}}),
            },
            McpTool_x {
                name: "proxy/restart".to_string(),
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

    /// fn `handle_initialize` — returns proxy's initialization response.
    pub fn handle_initialize(id: JsonRpcId_x) -> JsonRpcResponse_x {
        JsonRpcResponse_x {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "mcp-proxy",
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
