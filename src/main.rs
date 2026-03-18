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
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
}

fn main() {
    let _cli = Cli::parse();
    println!("mcp-proxy ready");
}
