mod adapter;
mod core;
mod gateway;
mod pal;
mod shared;

#[tokio::main]
async fn main() {
    eprintln!("mcp-proxy: starting MCP multiplexer");

    match adapter::ProxyAdapter_adp::new().run().await {
        Ok(_) => {}
        Err(e) => {
            eprintln!("mcp-proxy: fatal: {e}");
            std::process::exit(1);
        }
    }
}
