# mcp-proxy Installation & Setup

## Quick Start

mcp-proxy is an MCP-aware multiplexer that aggregates multiple downstream MCP servers into a single upstream connection.

### 1. Build

```bash
cargo build --release
```

Binary: `target/release/mcp-proxy.exe`

### 2. Configure MCP Servers

Create `mcp-servers.json` in the proxy directory with your servers:

```json
{
  "servers": [
    {
      "id": "gui-mcp",
      "binary": "C:/Users/mathi/bin/gui-mcp.exe",
      "args": []
    },
    {
      "id": "rules",
      "binary": "C:/Users/mathi/AppData/Local/Packages/PythonSoftwareFoundation.Python.3.13_qbz5n2kfra8p0/LocalCache/local-packages/Python313/Scripts/rules-mcp.exe",
      "args": []
    },
    {
      "id": "rulestools",
      "binary": "C:/Users/mathi/AppData/Local/Packages/PythonSoftwareFoundation.Python.3.13_qbz5n2kfra8p0/LocalCache/local-packages/Python313/Scripts/rulestools-mcp.exe",
      "args": []
    }
  ]
}
```

### 3. Register with Claude Code

Add to `~/.claude.json` (NOT settings.json):

```json
"mcpServers": {
  "mcp-proxy": {
    "type": "stdio",
    "command": "D:/REPO/RUSTPROJECTS/MCP/mcp-proxy/target/release/mcp-proxy.exe",
    "args": ["--config", "D:/REPO/RUSTPROJECTS/MCP/mcp-proxy/mcp-servers.json"],
    "env": {}
  }
}
```

**IMPORTANT:** Use `~/.claude.json` (global MCP config), not `~/.claude/settings.json`

### 4. Restart Claude Code

Fully close and reopen Claude Code. Run `/mcp` command to verify:

```
mcp-proxy · ✔ connected
gui-mcp · ✔ connected
rules · ✔ connected
rulestools · ✔ connected
```

## Features

### Management Tools

Proxy exposes MCP-standard management tools:
- **mcp/load** — Load a new server dynamically
- **mcp/unload** — Unload a running server
- **mcp/list** — List all loaded servers
- **mcp/restart** — Restart a server (auto-restart on binary change)

### Tool Aggregation

Tools from downstream servers are namespaced with `{server-id}__` prefix:
- `gui-mcp` → `gui-mcp__screenshot_window`, `gui-mcp__send_keys`, etc.
- `rules` → `rules__get_rule`, `rules__search_rules`, etc.
- `rulestools` → `rulestools__scan_file`, `rulestools__scan_tree`, etc.

### Auto-Restart on Binary Change

Proxy watches server binaries using `notify` crate. On modify event, server auto-restarts and tools are re-aggregated with `notifications/tools/list_changed`.

### Structured Logging

All proxy operations logged to `mcp-proxy.log` with timestamps and levels:
```
[2026-03-19T11:28:49.003712Z] INFO server loaded and initialized (server=gui-mcp)
[2026-03-19T11:28:51.882896Z] INFO server loaded and initialized (server=rules)
```

## Configuration File Format

### Server Entry

```json
{
  "id": "unique-server-id",
  "binary": "path/to/binary.exe",
  "args": ["--flag", "value"]
}
```

- **id**: Used for tool namespacing (`id__toolname`)
- **binary**: Full path or command name
- **args**: Command-line arguments passed to binary

## Troubleshooting

### Proxy not showing in `/mcp` list

- Ensure you edited `~/.claude.json` (not settings.json)
- Verify binary path and config path are correct
- Check `mcp-proxy.log` for startup errors
- Fully restart Claude Code

### Servers fail to initialize

- Check `mcp-proxy.log` for error details
- Verify binary paths exist and are executable
- Initialization timeout is 5 seconds for slow servers (e.g., Python FastMCP)

### Log file location

- `mcp-proxy.log` in the working directory when proxy starts
- Contains all server lifecycle events and errors

## Development

### Test Server

Minimal standalone MCP server for testing without external dependencies:

```bash
rustc test-server.rs -o test-server.exe
```

Add to mcp-servers.json:
```json
{"id": "test-server", "binary": "./test-server.exe", "args": []}
```

### Running Locally

```bash
./target/release/mcp-proxy.exe --config mcp-servers.json
```

Then send JSON-RPC 2.0 requests via stdin, responses on stdout.


---

<!-- LARS:START -->
<a href="https://lpmathiasen.com">
  <img src="https://carousel.lpmathiasen.com/carousel.svg?slot=2" alt="Lars P. Mathiasen"/>
</a>
<!-- LARS:END -->
