# mcp-proxy — MCP Multiplexer for Claude Code

**MCP proxy that manages multiple downstream MCP servers as a single unified tool surface.**

Claude Code sees one MCP server (`mcp-tools` in `/mcp`). Behind it, mcp-proxy spawns and manages multiple downstream servers, aggregates their tools, routes calls, and handles crashes — all without Claude ever seeing a disconnect.

## What it does

- **Spawns all configured servers** on startup from `~/.mcp/mcp-servers.json`
- **Aggregates tools** with namespace prefixes (`server-id__tool-name`)
- **Routes tool calls** transparently to the correct downstream server
- **Auto-restarts** servers when their binary changes (file watcher)
- **Crash recovery** with exponential backoff (2s → 4s → 8s → 16s)
- **Graceful shutdown** on Ctrl+C with proper cleanup
- **Structured logging** to `mcp-proxy.log`

## Installation

```bash
cargo build --release
cp target/release/mcp-proxy.exe ~/bin/
```

### Claude Code config (`~/.claude.json`)

```json
{
  "mcpServers": {
    "mcp-tools": {
      "command": "C:/Users/you/bin/mcp-proxy.exe",
      "args": ["--config", "C:/Users/you/.mcp/mcp-servers.json"],
      "type": "stdio"
    }
  }
}
```

### Server config (`~/.mcp/mcp-servers.json`)

```json
{
  "servers": [
    {
      "id": "gui-mcp",
      "binary": "C:/Users/you/bin/gui-mcp.exe",
      "args": [],
      "description": "Desktop GUI automation via Windows API"
    },
    {
      "id": "rules",
      "binary": "C:/Users/you/.cargo/bin/rules-mcp.exe",
      "args": [],
      "description": "Project rules and conventions engine"
    }
  ],
  "init_timeout_secs": 5,
  "tool_call_timeout_secs": 30
}
```

To add a new MCP server: add an entry to `~/.mcp/mcp-servers.json` and restart your session.

## Architecture

```
Claude Code (upstream)
    │ stdio (persistent JSON-RPC)
    ▼
[mcp-proxy]                          ← appears as "mcp-tools" in /mcp
    ├── tool routing                 ← server-id__tool-name → server-id → tool-name
    ├── crash recovery               ← exponential backoff respawn
    ├── binary watcher               ← notify crate, auto-restart on change
    ├── [gui-mcp]        15 tools    ← downstream MCP server
    ├── [rules]           5 tools    ← downstream MCP server
    ├── [rulestools]     24 tools    ← downstream MCP server
    └── [issuesmcp]       7 tools    ← downstream MCP server
```

### Tool namespacing

```
gui-mcp exports:    send_keys, screenshot_window, ...
Claude sees:        gui-mcp__send_keys, gui-mcp__screenshot_window, ...
Proxy routes:       gui-mcp → send_keys (original name restored)
```

### Claude Code Tool Search integration

When total tools exceed the context threshold (~128), Claude Code automatically activates **deferred tools** / **Tool Search**. This is transparent — the proxy doesn't need to do anything special. Tools are discovered via `ToolSearch` on demand.

## Config reference

### `~/.mcp/mcp-servers.json`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `servers[].id` | string | yes | Unique server identifier (used as namespace prefix) |
| `servers[].binary` | string | yes | Path to MCP server binary |
| `servers[].args` | string[] | no | Command-line arguments (default: `[]`) |
| `servers[].description` | string | no | Human-readable description |
| `init_timeout_secs` | number | no | Server initialization timeout (default: 5) |
| `tool_call_timeout_secs` | number | no | Tool call timeout (default: 30) |

Servers with empty `binary` are skipped.

## Code structure

```
src/
├── main.rs              Entry point, CLI args, Tokio runtime
├── shared/mod.rs        Types: JSON-RPC, errors, config, server state
├── adapter/
│   ├── mod.rs           Main event loop, dispatch, server lifecycle
│   └── handlers.rs      Tool call routing to downstream servers
├── core/
│   ├── mod.rs           MCP protocol (initialize, tools/list, management tools)
│   ├── lifecycle.rs     spawn_and_initialize, shutdown, restart, crash monitor
│   └── registry.rs      ServerRegistry (tool index, server storage)
├── gateway/
│   └── mod.rs           IO: process spawn, file watch, JSON-RPC read/write
└── pal/
    └── mod.rs           Platform abstraction (process termination)
```

## Monitoring

Check `mcp-proxy.log` for server lifecycle events, crash recovery, and errors.

## Stack

- Rust 2024 edition
- Tokio (async runtime)
- notify 6 (file watching)
- serde + serde_json (JSON-RPC)
- clap 4 (CLI)
- tracing (structured logging)


---

<!-- LARS:START -->
<a href="https://lpmathiasen.com">
  <img src="https://carousel.lpmathiasen.com/carousel.svg?slot=2" alt="Lars P. Mathiasen"/>
</a>
<!-- LARS:END -->

<!-- MIB-NOTICE -->

> **Note:** This project is as-is — it is an artefact of a MIB process. See [mib.lpmwfx.com](https://mib.lpmwfx.com/) for details. It is only an MVP, not a full release. Feel free to use it for your own projects as you like.
