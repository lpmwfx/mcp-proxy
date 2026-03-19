use clap::Parser;
use std::path::PathBuf;

mod adapter;
mod core;
mod gateway;
mod pal;
mod shared;

#[derive(Parser)]
#[command(name = "mcp-proxy", version, about = "MCP stdio multiplexer with config and watcher")]
struct Cli {
    /// Path to config file (mcp-servers.json)
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    eprintln!("mcp-proxy: starting MCP multiplexer");

    let adapter = match cli.config {
        Some(path) => adapter::ProxyAdapter_adp::new().with_config(path),
        None => {
            // Try default config
            let default_path = PathBuf::from("mcp-servers.json");
            if default_path.exists() {
                adapter::ProxyAdapter_adp::new().with_config(default_path)
            } else {
                adapter::ProxyAdapter_adp::new()
            }
        }
    };

    match adapter.run().await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("mcp-proxy: fatal: {e}");
            std::process::exit(1);
        }
    }
}
