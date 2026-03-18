# MCP Rust Proxy Pattern

## Problem

Hver MCP-server der er registreret i Claude Desktop/Code spawnes som en child-process.
Når den underliggende Rust-applikation ændrer sig (ny binary, ny tool-liste), skal
Claude-instansen genstarte for at opdage ændringerne — selv hvis MCP-wrapperen
stadig kører.

Dette gælder **per server** — det er ikke et meta-management problem, men et
**per-server livscyklus problem**.

---

## Løsning: Tokio-baseret stdio-proxy

Hver MCP-wrapper implementeres som en tynd Tokio-proxy der:

1. Holder den permanente stdio-forbindelse mod Claude (upstream)
2. Spawner og watch'er den interne Rust-binary (downstream)
3. Genstarter downstream ved binary-ændring eller crash
4. Sender `notifications/tools/list_changed` upstream efter genstart

```
Claude Code
    │  stdio (persistent)
    ▼
[mcp-proxy]          ← Tokio async runtime
    │  stdio (respawnable)
    ▼
[din-rust-app]       ← den faktiske MCP-implementering
```

Claude ser aldrig en disconnect. `list_changed` trigger en stille tool-refresh.

---

## Arkitektur

### Komponenter

| Komponent | Ansvar |
|---|---|
| `proxy` task | Relay stdin → downstream, downstream stdout → upstream |
| `watcher` task | `notify` crate — watch binary path for `Modify` events |
| `supervisor` task | Spawn/respawn downstream process via `tokio::process::Command` |
| `notifier` task | Send `notifications/tools/list_changed` til upstream efter respawn |

### Tokio task-graf

```
main
├── proxy_upstream_to_downstream   (tokio::spawn)
├── proxy_downstream_to_upstream   (tokio::spawn)
├── watcher                        (tokio::spawn)
└── supervisor                     ← koordinator
        ↑ channel fra watcher (binary changed / process died)
        ↓ channel til notifier (respawn done)
```

Alle tasks kommunikerer via `tokio::sync::mpsc` channels.
Supervisor holder `tokio::process::Child` handle og er eneste owner.

---

## Dependencies (Cargo.toml)

```toml
[dependencies]
tokio        = { version = "1", features = ["full"] }
notify       = "6"                  # file system watcher
serde        = { version = "1", features = ["derive"] }
serde_json   = "1"
```

Ingen tung MCP-SDK i proxyen — den er JSON-agnostisk og relay'er bytes.
Den eneste JSON proxyen selv konstruerer er `list_changed`-notifikationen.

---

## list_changed payload

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/tools/list_changed"
}
```

Sendes downstream→upstream via stdout-kanalen umiddelbart efter downstream
er oppe og har svaret på `initialize`.

---

## Graceful respawn-sekvens

```
1. watcher detekterer binary Modify event
2. → mpsc signal til supervisor
3. supervisor: SIGTERM til downstream child
4. supervisor: await child exit (timeout → SIGKILL)
5. supervisor: spawn ny child
6. supervisor: await downstream "initialized" svar
7. supervisor: signal notifier
8. notifier: skriv list_changed til upstream stdout
```

Claude Code modtager `list_changed` og re-fetcher tool-listen fra proxyen,
som nu relay'er fra den nye downstream.

---

## Konfiguration i Claude (claude_desktop_config.json)

```json
{
  "mcpServers": {
    "min-server": {
      "command": "/path/to/mcp-proxy",
      "args": ["/path/to/min-rust-app"]
    }
  }
}
```

Proxyen tager binary-stien som første argument. Øvrige args videresendes til downstream.

---

## Scope

- Dette mønster løser **hot-reload af enkelt-servers** ved code/binary ændringer
- Det løser **ikke** add/remove af servere til Claude uden restart af Claude selv
- Det er bevidst minimalt — proxyen er ~150 linjer Rust, ingen forretningslogik

---

## Fremtidigt

- Unix socket transport som alternativ til stdio-relay (bedre for debugging)
- Health-check ping til downstream for crash-detection uden file-watch
- Shared proxy-binary der konfigureres via argument (samme binary, mange servers)
