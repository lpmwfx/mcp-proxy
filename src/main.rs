use clap::Parser;
use std::path::PathBuf;
use tracing_appender::non_blocking;
use tracing_subscriber::fmt::time::SystemTime;

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
    // Initialize logging to file
    let file_appender = tracing_appender::rolling::never(".", "mcp-proxy.log");
    let (non_blocking, _guard) = non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_timer(SystemTime::default())
        .with_level(true)
        .init();

    let cli = Cli::parse();

    tracing::info!("starting mcp-proxy");

    let adapter = match cli.config {
        Some(path) => {
            tracing::info!(config = ?path, "using config file");
            adapter::ProxyAdapter_adp::new().with_config(path)
        }
        None => {
            let default_path = PathBuf::from("mcp-servers.json");
            if default_path.exists() {
                tracing::info!(config = ?default_path, "using default config file");
                adapter::ProxyAdapter_adp::new().with_config(default_path)
            } else {
                tracing::info!("no config file found, starting without servers");
                adapter::ProxyAdapter_adp::new()
            }
        }
    };

    match adapter.run().await {
        Ok(_) => tracing::info!("proxy shutdown"),
        Err(e) => {
            tracing::error!(error = %e, "fatal error");
            std::process::exit(1);
        }
    }
}
