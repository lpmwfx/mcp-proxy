//! Shared layer (_x) — errors, results, shared traits.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use std::path::PathBuf;
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

// ============================================================================
// Timeout Constants
// ============================================================================

/// Default timeout for server initialization (seconds).
pub const INIT_TIMEOUT_DEFAULT_SECS: u64 = 5;

/// Default timeout for tool calls (seconds).
pub const TOOL_CALL_TIMEOUT_DEFAULT_SECS: u64 = 30;

#[derive(Debug)]
/// enum `ProxyEvent_x`.
pub enum ProxyEvent_x {
    BinaryChanged(String), // server id
    ProcessDied(String),   // server id
    RespawnDone(String, Box<DownstreamServer_x>), // server id, new server
}

#[derive(Debug)]
/// enum `ProxyError_x`.
pub enum ProxyError_x {
    SpawnFailed(io::Error),
    KillFailed(io::Error),
    RelayBroken(io::Error),
    WatchFailed(String),
    JsonParse(serde_json::Error),
    JsonSerialize(serde_json::Error),
    UpstreamEof,
    DownstreamEof(String),
    InitializeFailed(String),
    ServerNotFound(String),
    ServerAlreadyLoaded(String),
    InvalidRequest(String),
}

impl std::fmt::Display for ProxyError_x {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(e) => write!(f, "spawn failed: {e}"),
            Self::KillFailed(e) => write!(f, "kill failed: {e}"),
            Self::RelayBroken(e) => write!(f, "relay broken: {e}"),
            Self::WatchFailed(s) => write!(f, "watch failed: {s}"),
            Self::JsonParse(e) => write!(f, "json parse: {e}"),
            Self::JsonSerialize(e) => write!(f, "json serialize: {e}"),
            Self::UpstreamEof => write!(f, "upstream EOF"),
            Self::DownstreamEof(id) => write!(f, "downstream EOF: {id}"),
            Self::InitializeFailed(s) => write!(f, "initialize failed: {s}"),
            Self::ServerNotFound(s) => write!(f, "server not found: {s}"),
            Self::ServerAlreadyLoaded(s) => write!(f, "server already loaded: {s}"),
            Self::InvalidRequest(s) => write!(f, "invalid request: {s}"),
        }
    }
}

impl std::error::Error for ProxyError_x {}

/// type `ProxyResult_x`.
pub type ProxyResult_x<T> = Result<T, ProxyError_x>;

// ============================================================================
// JSON-RPC Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
/// enum `JsonRpcId_x` — JSON-RPC ID can be string, number, or null.
pub enum JsonRpcId_x {
    Num(i64),
    Str(String),
    Null,
}

#[derive(Debug, Serialize, Deserialize)]
/// struct `JsonRpcRequest_x` — inbound request or notification from upstream.
pub struct JsonRpcRequest_x {
    pub jsonrpc: String,
    pub id: Option<JsonRpcId_x>,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
/// struct `JsonRpcResponse_x` — response back to upstream.
pub struct JsonRpcResponse_x {
    pub jsonrpc: String,
    pub id: JsonRpcId_x,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError_x>,
}

#[derive(Debug, Serialize, Deserialize)]
/// struct `JsonRpcError_x` — JSON-RPC error object.
pub struct JsonRpcError_x {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Serialize)]
/// struct `JsonRpcNotification_x` — notification to upstream (no id, no response expected).
pub struct JsonRpcNotification_x {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

// ============================================================================
// MCP Tool Type
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
/// struct `McpTool_x` — represents a tool exported by a server.
pub struct McpTool_x {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

// ============================================================================
// Downstream Server State
// ============================================================================

/// type `BufLines`.
pub type BufLines = tokio::io::Lines<BufReader<ChildStdout>>;

#[derive(Debug)]
/// struct `DownstreamServer_x` — represents an active downstream MCP server.
pub struct DownstreamServer_x {
    pub id: String,
    pub binary: PathBuf,
    pub args: Vec<String>,
    pub stdin: ChildStdin,
    pub stdout: BufLines,
    pub tools: Vec<McpTool_x>,
    pub next_id: i64,
    pub crash_count: u32,
    pub kill_tx: Option<oneshot::Sender<()>>,
    pub monitor_handle: Option<JoinHandle<()>>,
    pub watcher_handle: Option<JoinHandle<()>>,
}

// ============================================================================
// Configuration Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
/// struct `ServerConfig_x` — configuration for a single downstream server.
pub struct ServerConfig_x {
    pub id: String,
    pub binary: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
/// struct `ProxyConfig_x` — top-level configuration (list of servers to auto-load).
pub struct ProxyConfig_x {
    pub servers: Vec<ServerConfig_x>,
    /// Timeout for server initialization in seconds (default: 5).
    #[serde(default)]
    pub init_timeout_secs: Option<u64>,
    /// Timeout for tool calls in seconds (default: 30).
    #[serde(default)]
    pub tool_call_timeout_secs: Option<u64>,
}
