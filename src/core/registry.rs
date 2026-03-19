//! Server registry — manages active downstream servers and tool index.

use std::collections::HashMap;

use crate::shared::{DownstreamServer_x, McpTool_x};

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

    /// fn `get_server_tools` — returns tools for a specific server (unnamespaced).
    pub fn get_server_tools(&self, server_id: &str) -> Vec<&McpTool_x> {
        self.servers
            .get(server_id)
            .map(|server| server.tools.iter().collect())
            .unwrap_or_default()
    }

    /// fn `servers_mut` — returns mutable iterator over servers.
    pub fn servers_mut(&mut self) -> impl Iterator<Item = &mut DownstreamServer_x> {
        self.servers.values_mut()
    }
}
