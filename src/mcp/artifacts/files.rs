//! Filesystem identity checks; called only inside bounded blocking jobs.
use super::StoreError;
use std::fs::{File, Metadata};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(super) struct OwnedFile {
    pub(super) path: PathBuf,
    pub(super) size: u64,
    identity: (u64, u64),
}
impl OwnedFile {
    pub(super) fn create(path: PathBuf, uid: u32) -> Result<(Self, File), StoreError> {
        #[cfg(unix)]
        {
            use std::fs::OpenOptions;
            use std::os::unix::fs::OpenOptionsExt as _;
            let file = OpenOptions::new()
                .write(true)
                .read(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map_err(|_| StoreError::Unavailable)?;
            let metadata = file.metadata().map_err(|_| StoreError::Unavailable)?;
            validate(&metadata, uid, 0)?;
            Ok((
                Self {
                    path,
                    size: 0,
                    identity: identity(&metadata),
                },
                file,
            ))
        }
        #[cfg(not(unix))]
        {
            let _ = (path, uid);
            Err(StoreError::Unavailable)
        }
    }
    pub(super) fn validate(&self, uid: u32) -> Result<(), StoreError> {
        let metadata = std::fs::symlink_metadata(&self.path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                StoreError::Missing
            } else {
                StoreError::Unavailable
            }
        })?;
        validate(&metadata, uid, self.size)?;
        if identity(&metadata) != self.identity {
            return Err(StoreError::Unavailable);
        }
        Ok(())
    }
    pub(super) fn open(&self, uid: u32) -> Result<File, StoreError> {
        self.validate(uid)?;
        let file = File::open(&self.path).map_err(|_| StoreError::Unavailable)?;
        let metadata = file.metadata().map_err(|_| StoreError::Unavailable)?;
        validate(&metadata, uid, self.size)?;
        if identity(&metadata) != self.identity {
            return Err(StoreError::Unavailable);
        }
        Ok(file)
    }
    pub(super) fn remove(&self, uid: u32) -> Result<(), StoreError> {
        match self.validate(uid) {
            Ok(()) => std::fs::remove_file(&self.path).map_err(|_| StoreError::Unavailable),
            Err(StoreError::Missing) => Ok(()),
            Err(error) => Err(error),
        }
    }
}
#[cfg(unix)]
fn identity(metadata: &Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt as _;
    (metadata.dev(), metadata.ino())
}
#[cfg(not(unix))]
fn identity(_: &Metadata) -> (u64, u64) {
    (0, 0)
}
#[cfg(unix)]
fn validate(metadata: &Metadata, uid: u32, size: u64) -> Result<(), StoreError> {
    use std::os::unix::fs::MetadataExt as _;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != uid
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() != size
    {
        return Err(StoreError::Unavailable);
    }
    Ok(())
}
#[cfg(not(unix))]
fn validate(_: &Metadata, _: u32, _: u64) -> Result<(), StoreError> {
    Err(StoreError::Unavailable)
}

pub(super) fn prepare(root: &Path, uid: u32) -> Result<(PathBuf, Vec<PathBuf>), StoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)
            .map_err(|_| StoreError::Unavailable)?;
        if !std::fs::symlink_metadata(root)
            .map_err(|_| StoreError::Unavailable)?
            .is_dir()
        {
            return Err(StoreError::Unavailable);
        }
        let root = root.canonicalize().map_err(|_| StoreError::Unavailable)?;
        let mut paths = Vec::new();
        for ancestor in root.ancestors() {
            let m = std::fs::symlink_metadata(ancestor).map_err(|_| StoreError::Unavailable)?;
            let protected_sticky = ancestor != root && m.uid() == 0 && m.mode() & 0o1000 != 0;
            if !m.is_dir()
                || (m.uid() != uid && m.uid() != 0)
                || (m.mode() & 0o022 != 0 && !protected_sticky)
                || (ancestor == root && (m.uid() != uid || m.mode() & 0o777 != 0o700))
            {
                return Err(StoreError::Unavailable);
            }
            paths.push(ancestor.to_path_buf());
        }
        Ok((root, paths))
    }
    #[cfg(not(unix))]
    {
        let _ = (root, uid);
        Err(StoreError::Unavailable)
    }
}
