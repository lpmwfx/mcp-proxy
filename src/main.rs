use clap::Parser;

mod adapter;
mod core;
mod gateway;
mod pal;
mod shared;

#[derive(Parser)]
#[command(name = "mcp-proxy", version, about = "MCP stdio proxy with hot-reload")]
struct Cli {
    /// Path to the downstream binary
    binary: std::path::PathBuf,

    /// Arguments to pass to the downstream binary
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    eprintln!("mcp-proxy: spawning {:?}", cli.binary);

    match adapter::ProxyAdapter_adp::new(cli.binary, cli.args).run().await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("mcp-proxy: fatal: {e}");
            std::process::exit(1);
        }
    }
}
