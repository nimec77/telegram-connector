//! Private storage with trusted canonical ancestry and explicit macOS ACL policy.
use crate::error::Error;
use grammers_session::storages::SqliteSession;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
fn entry_paths(path: &Path) -> Vec<PathBuf> {
    ["", "-wal", "-shm", "-journal"]
        .into_iter()
        .map(|suffix| {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            PathBuf::from(name)
        })
        .collect()
}

#[cfg(unix)]
async fn utility_output(mut command: tokio::process::Command) -> Result<Vec<u8>, Error> {
    use std::process::Stdio;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let failure = || Error::Auth("Unable to verify session ownership or access controls".into());
    let output = tokio::time::timeout(std::time::Duration::from_secs(2), command.output())
        .await
        .map_err(|_| failure())?
        .map_err(|_| failure())?;
    if !output.status.success() {
        return Err(failure());
    }
    Ok(output.stdout)
}

#[cfg(unix)]
async fn effective_uid() -> Result<u32, Error> {
    // std has no safe Unix get-euid API. Use the trusted OS utility already
    // present on supported macOS, with captured output and a finite deadline.
    let mut command = tokio::process::Command::new("/usr/bin/id");
    command.arg("-u").env("LC_ALL", "C");
    let output = utility_output(command).await?;
    String::from_utf8_lossy(&output)
        .trim()
        .parse()
        .map_err(|_| Error::Auth("Unable to identify session owner".into()))
}

#[cfg(target_os = "macos")]
async fn reject_directory_grants(path: &Path) -> Result<(), Error> {
    let mut command = tokio::process::Command::new("/bin/ls");
    command.args(["-lde", "--"]).arg(path).env("LC_ALL", "C");
    let output = utility_output(command).await?;
    if output.is_empty()
        || String::from_utf8_lossy(&output)
            .lines()
            .skip(1)
            .any(|line| line.contains(" allow "))
    {
        return Err(Error::Auth(
            "Session ancestry cannot contain access-granting ACLs; use a private path".into(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn clear_file_acl(path: &Path) -> Result<(), Error> {
    let mut command = tokio::process::Command::new("/bin/chmod");
    // BSD chmod -N treats -- as a filename. The verified operand is absolute.
    command.arg("-N").arg(path);
    utility_output(command).await?;
    Ok(())
}

#[cfg(unix)]
async fn prepare(path: &Path) -> Result<PathBuf, Error> {
    use std::os::unix::fs::{
        DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
    };
    let failure = || Error::Auth("Unable to prepare private session storage".into());
    let uid = effective_uid().await?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|_| failure())?;
    if !std::fs::symlink_metadata(parent)
        .map_err(|_| failure())?
        .is_dir()
    {
        return Err(Error::Auth(
            "Session parent must be a regular directory".into(),
        ));
    }
    let parent = parent.canonicalize().map_err(|_| failure())?;
    // Walk the canonical path from its trusted root. Root-owned sticky temporary
    // roots protect each next owned component; non-sticky writable ancestors do not.
    let ancestors: Vec<_> = parent.ancestors().collect();
    for directory in ancestors.into_iter().rev() {
        let metadata = std::fs::symlink_metadata(directory).map_err(|_| failure())?;
        let mode = metadata.permissions().mode();
        let trusted_owner = metadata.uid() == uid || metadata.uid() == 0;
        let protected_sticky = metadata.uid() == 0 && mode & 0o1000 != 0;
        if !metadata.is_dir() || !trusted_owner || (mode & 0o022 != 0 && !protected_sticky) {
            return Err(Error::Auth("Session ancestry must have trusted ownership and exclude unprotected group/other writes".into()));
        }
        #[cfg(target_os = "macos")]
        reject_directory_grants(directory).await?;
    }
    let path = parent.join(path.file_name().ok_or_else(failure)?);
    // All subsequent path-based operations use the verified canonical parent,
    // so an untrusted alias cannot be retargeted between checks and SQLite open.
    for entry in entry_paths(&path) {
        match std::fs::symlink_metadata(&entry) {
            Ok(metadata)
                if !metadata.is_file() || metadata.nlink() != 1 || metadata.uid() != uid =>
            {
                return Err(Error::Auth(
                    "Session database and sidecars must be owned single regular files".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(failure()),
        }
    }
    match std::fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map_err(|_| failure())?;
        }
        Err(_) => return Err(failure()),
    }
    for entry in entry_paths(&path) {
        match std::fs::symlink_metadata(&entry) {
            Ok(metadata)
                if metadata.is_file() && metadata.nlink() == 1 && metadata.uid() == uid =>
            {
                #[cfg(target_os = "macos")]
                clear_file_acl(&entry).await?;
                std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o600))
                    .map_err(|_| failure())?;
            }
            Ok(_) => return Err(failure()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(failure()),
        }
    }
    Ok(path)
}

#[cfg(not(unix))]
async fn prepare(path: &Path) -> Result<PathBuf, Error> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|_| Error::Auth("Unable to create session directory".into()))?;
    }
    Ok(path.to_path_buf())
}

pub(crate) async fn open_private_session(path: &Path) -> Result<Arc<SqliteSession>, Error> {
    let path = prepare(path).await?;
    let session = SqliteSession::open(&path)
        .await
        .map_err(|_| Error::Auth("Unable to open session storage".into()))?;
    prepare(&path).await?;
    Ok(Arc::new(session))
}
