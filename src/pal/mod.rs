//! PAL layer (_pal) — platform abstraction.

use tokio::process::Child;
use crate::shared::ProxyResult_x;
use crate::shared::ProxyError_x;

/// trait `ProcessPal_pal`.
pub trait ProcessPal_pal {
    async fn terminate(child: &mut Child) -> ProxyResult_x<()>;
    async fn force_kill(child: &mut Child) -> ProxyResult_x<()>;
}

/// struct `WindowsProcessPal_pal`.
pub struct WindowsProcessPal_pal;

impl ProcessPal_pal for WindowsProcessPal_pal {
    async fn terminate(child: &mut Child) -> ProxyResult_x<()> {
        child.kill().await.map_err(ProxyError_x::KillFailed)
    }

    async fn force_kill(child: &mut Child) -> ProxyResult_x<()> {
        child.kill().await.map_err(ProxyError_x::KillFailed)
    }
}
