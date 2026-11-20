//! Bounded desktop launch seam; independent from Telegram network work.
use crate::error::Error;
#[async_trait::async_trait]
pub(super) trait DesktopLauncher: Send + Sync {
    fn supported(&self) -> bool;
    async fn open(&self, link: &str) -> Result<(), Error>;
}
pub(super) struct SystemLauncher;
#[async_trait::async_trait]
impl DesktopLauncher for SystemLauncher {
    fn supported(&self) -> bool {
        cfg!(target_os = "macos")
    }
    async fn open(&self, link: &str) -> Result<(), Error> {
        #[cfg(not(target_os = "macos"))]
        {
            let _ = link;
            Err(Error::UnsupportedPlatform)
        }
        #[cfg(target_os = "macos")]
        {
            let output = tokio::process::Command::new("open")
                .arg(link)
                .kill_on_drop(true)
                .output()
                .await;
            match output {
                Ok(output) if output.status.success() => Ok(()),
                Ok(output) => Err(Error::DesktopOpenFailed {
                    message: format!("Failed to open: {:?}", output.status),
                    link: link.to_string(),
                }),
                Err(_) => Err(Error::DesktopOpenFailed {
                    message: "Failed to execute open command".into(),
                    link: link.to_string(),
                }),
            }
        }
    }
}
