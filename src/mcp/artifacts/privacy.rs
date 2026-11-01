//! Trusted, deadline-bounded OS checks; never expose utility diagnostics.
use super::StoreError;
use std::path::Path;

async fn output(mut command: tokio::process::Command) -> Result<Vec<u8>, StoreError> {
    use std::process::Stdio;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(2), command.output())
        .await
        .map_err(|_| StoreError::Unavailable)?
        .map_err(|_| StoreError::Unavailable)?;
    if !output.status.success() {
        return Err(StoreError::Unavailable);
    }
    Ok(output.stdout)
}
pub(super) async fn uid() -> Result<u32, StoreError> {
    if !cfg!(unix) {
        return Err(StoreError::Unavailable);
    }
    let mut command = tokio::process::Command::new("/usr/bin/id");
    command.arg("-u").env("LC_ALL", "C");
    String::from_utf8_lossy(&output(command).await?)
        .trim()
        .parse()
        .map_err(|_| StoreError::Unavailable)
}
pub(super) async fn directory_acl(path: &Path) -> Result<(), StoreError> {
    #[cfg(target_os = "macos")]
    {
        let mut command = tokio::process::Command::new("/bin/ls");
        command.args(["-lde", "--"]).arg(path).env("LC_ALL", "C");
        let result = output(command).await?;
        if result.is_empty()
            || String::from_utf8_lossy(&result)
                .lines()
                .skip(1)
                .any(|line| line.contains(" allow "))
        {
            return Err(StoreError::Unavailable);
        }
    }
    let _ = path;
    Ok(())
}
