Præcis — det er den fulde konsekvens af mønsteret:

**Før:**
```
Claude --stdio--> [rust-app]   (app = MCP + forretningslogik sammenblandet)
```

**Efter:**
```
Claude --stdio--> [tokio-proxy] --IPC--> [rust-app]
                                         (kun forretningslogik)
```

Og så snart du har IPC, sker der noget interessant:

```
                         ┌─── [app-a]  (unix socket)
Claude --stdio--> [tokio]┤─── [app-b]  (unix socket)
                         └─── [app-c]  (unix socket)
```

Den *rigtige* gevinst er at **rust-appene ikke længere behøver at kende til MCP overhovedet**. De eksponerer bare en simpel IPC-kontrakt:

```
← list_tools()     → [{ name, description, schema }]
← call_tool(name)  → result
```

Tokio-laget ejer al MCP-protokol-kompleksitet. Appene er rene.

**Det åbner også for:**
- Appen kan køre *persistent* og ikke spawnes af MCP
- Appen kan registrere sig selv hos proxyen ved opstart
- Hot-reload = app genstarter, re-connecter, sender ny tool-liste

Det er faktisk et **service mesh i miniformat** — bare med unix sockets og `list_changed` i stedet for HTTP og service discovery.

Skal `project.md` opdateres til at afspejle denne arkitektur som det *primære* mønster?


---

<!-- LARS:START -->
<a href="https://lpmathiasen.com">
  <img src="https://carousel.lpmathiasen.com/carousel.svg?slot=2" alt="Lars P. Mathiasen"/>
</a>
<!-- LARS:END -->
