//! Private session storage paths, checked before SQLite can follow an entry.
use crate::error::Error;
use grammers_session::storages::SqliteSession;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
fn prepare(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::{
        DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
    };
    let failure = || Error::Auth("Unable to prepare private session storage".into());
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)
        .map_err(|_| failure())?;
    let metadata = std::fs::symlink_metadata(parent).map_err(|_| failure())?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o022 != 0 {
        return Err(Error::Auth(
            "Session parent must be a directory not writable by group or others".into(),
        ));
    }
    // Validate every existing entry before SQLite can create, migrate or journal.
    for entry in entry_paths(path) {
        match std::fs::symlink_metadata(&entry) {
            Ok(metadata) if !metadata.is_file() || metadata.nlink() != 1 => {
                return Err(Error::Auth(
                    "Session database and sidecars must be single regular files".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(failure()),
        }
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(|_| failure())?;
        }
        Err(_) => return Err(failure()),
    }
    for entry in entry_paths(path) {
        match std::fs::symlink_metadata(&entry) {
            Ok(metadata) if metadata.is_file() && metadata.nlink() == 1 => {
                std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o600))
                    .map_err(|_| failure())?;
            }
            Ok(_) => return Err(failure()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(failure()),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn prepare(path: &Path) -> Result<(), Error> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|_| Error::Auth("Unable to create session directory".into()))?;
    }
    // Platform ACL policy is not claimed by the Unix mode checks.
    Ok(())
}

pub(crate) async fn open_private_session(path: &Path) -> Result<Arc<SqliteSession>, Error> {
    prepare(path)?;
    let session = SqliteSession::open(path)
        .await
        .map_err(|_| Error::Auth("Unable to open session storage".into()))?;
    // SQLite inherits the private database mode for newly created WAL/SHM files.
    // Recheck anything opened during initialization as well.
    prepare(path)?;
    Ok(Arc::new(session))
}
