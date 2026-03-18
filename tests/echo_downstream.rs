/// Simple stdin→stdout echo for testing mcp-proxy relay.
/// Build: rustc tests/echo_downstream.rs -o tests/echo_downstream.exe
use std::io::{self, Read, Write};

fn main() {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf).unwrap();
    io::stdout().write_all(&buf).unwrap();
    io::stdout().flush().unwrap();
}
