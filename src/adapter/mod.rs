//! Adapter layer (_adp) — hub, coordinates all layers.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::gateway::{ProcessGateway_gtw, RelayGateway_gtw};
use crate::shared::ProxyResult_x;

/// struct `ProxyAdapter_adp`.
pub struct ProxyAdapter_adp {
    binary: PathBuf,
    args: Vec<String>,
}

impl ProxyAdapter_adp {
    /// fn `new`.
    pub fn new(binary: PathBuf, args: Vec<String>) -> Self {
        Self { binary, args }
    }

    /// fn `run`.
    pub async fn run(self) -> ProxyResult_x<ExitCode> {
        let mut child = ProcessGateway_gtw::spawn_downstream(&self.binary, &self.args)?;

        let child_stdin = child.stdin.take().expect("child stdin piped");
        let child_stdout = child.stdout.take().expect("child stdout piped");

        let upstream_stdin = tokio::io::stdin();
        let upstream_stdout = tokio::io::stdout();

        // upstream→downstream: read our stdin, write to child stdin
        let u2d = tokio::spawn(async move {
            let result = RelayGateway_gtw::relay(upstream_stdin, child_stdin).await;
            // child_stdin is dropped here → child gets EOF
            result
        });

        // downstream→upstream: read child stdout, write to our stdout
        let d2u = tokio::spawn(async move {
            RelayGateway_gtw::relay(child_stdout, upstream_stdout).await
        });

        // Wait for both relays to complete.
        // When upstream EOF → u2d finishes → child_stdin dropped → child gets EOF →
        // child exits → child stdout closes → d2u finishes.
        // When child exits first → child stdout closes → d2u finishes,
        // then we abort u2d (blocked on stdin read).
        tokio::select! {
            // Both done — normal flow
            (u2d_res, d2u_res) = async { tokio::join!(u2d, d2u) } => {
                eprintln!("mcp-proxy: relays ended: u2d={u2d_res:?} d2u={d2u_res:?}");
            }
        }

        // Cleanup: ensure child is dead
        let _ = child.kill().await;
        let _ = child.wait().await;

        Ok(ExitCode::SUCCESS)
    }
}
