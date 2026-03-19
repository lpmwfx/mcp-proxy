//! Server lifecycle — spawn, initialize, shutdown downstream servers.

use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::gateway::ProcessGateway_gtw;
use crate::shared::{DownstreamServer_x, ProxyError_x, ProxyResult_x, INIT_TIMEOUT_DEFAULT_SECS};

/// struct `DownstreamLifecycle_core` — lifecycle management for downstream servers.
pub struct DownstreamLifecycle_core;

impl DownstreamLifecycle_core {
    /// fn `spawn_and_initialize` — spawns a server, initializes it, and collects tools.
    pub async fn spawn_and_initialize(
        id: &str,
        binary: &Path,
        args: &[String],
        init_timeout_secs: u64,
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

        // Read initialize response (with timeout for slow servers)
        let line = tokio::time::timeout(
            std::time::Duration::from_secs(init_timeout_secs),
            stdout.next_line(),
        )
            .await
            .map_err(|_| ProxyError_x::InitializeFailed(format!("initialize timeout ({}s)", init_timeout_secs)))?
            .map_err(|_| ProxyError_x::InitializeFailed("failed to read initialize response".to_string()))?;

        let _resp = match line {
            None => return Err(ProxyError_x::InitializeFailed("EOF on initialize".to_string())),
            Some(l) => serde_json::from_str::<serde_json::Value>(&l)
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

        // Read tools/list response (with timeout for slow servers)
        let line = tokio::time::timeout(
            std::time::Duration::from_secs(init_timeout_secs),
            stdout.next_line(),
        )
            .await
            .map_err(|_| ProxyError_x::InitializeFailed(format!("tools/list timeout ({}s)", init_timeout_secs)))?
            .map_err(|_| ProxyError_x::InitializeFailed("failed to read tools/list response".to_string()))?;

        let tools_resp = match line {
            None => return Err(ProxyError_x::InitializeFailed("EOF on tools/list".to_string())),
            Some(l) => serde_json::from_str::<serde_json::Value>(&l)
                .map_err(|_| ProxyError_x::InitializeFailed("invalid tools/list response".to_string()))?,
        };

        // Extract tools from result
        let mut tools = Vec::new();
        if let Some(tools_arr) = tools_resp.get("result").and_then(|r| r.get("tools")).and_then(|t| t.as_array()) {
            for tool_val in tools_arr {
                if let Ok(tool) = serde_json::from_value::<crate::shared::McpTool_x>(tool_val.clone()) {
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
    pub async fn restart(mut old_server: DownstreamServer_x, init_timeout_secs: u64) -> ProxyResult_x<DownstreamServer_x> {
        // Kill old server
        let _ = old_server.child.kill().await;
        let _ = old_server.child.wait().await;

        // Respawn with same config
        Self::spawn_and_initialize(&old_server.id, &old_server.binary, &old_server.args, init_timeout_secs).await
    }
}
