//! Verified descriptors and trusted ancestry. Hostile same-UID mutation is out of scope.
use crate::telegram::types::sending::{SendFailure, SendFailureKind};
use std::fs::{File, Metadata};
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn invalid() -> SendFailure {
    SendFailure::not_sent(SendFailureKind::UploadSourceInvalid, None)
}

fn utility(mut command: Command) -> Result<Vec<u8>, SendFailure> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("LC_ALL", "C");
    let mut child = command.spawn().map_err(|_| invalid())?;
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().map_err(|_| invalid())? {
            if !status.success() {
                return Err(invalid());
            }
            let mut bytes = Vec::new();
            child
                .stdout
                .take()
                .ok_or_else(invalid)?
                .take(16_385)
                .read_to_end(&mut bytes)
                .map_err(|_| invalid())?;
            if bytes.len() > 16_384 {
                return Err(invalid());
            }
            return Ok(bytes);
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            return Err(invalid());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn uid() -> Result<u32, SendFailure> {
    if !cfg!(unix) {
        return Err(invalid());
    }
    let mut command = Command::new("/usr/bin/id");
    command.arg("-u");
    String::from_utf8_lossy(&utility(command)?)
        .trim()
        .parse()
        .map_err(|_| invalid())
}

pub(super) fn acl(path: &Path) -> Result<(), SendFailure> {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("/bin/ls");
        command.args(["-lde", "--"]).arg(path);
        let bytes = utility(command)?;
        if bytes.is_empty()
            || String::from_utf8_lossy(&bytes)
                .lines()
                .skip(1)
                .any(|l| l.contains(" allow "))
        {
            return Err(invalid());
        }
    }
    let _ = path;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileIdentity {
    pub device: u64,
    pub inode: u64,
    pub len: u64,
    pub uid: u32,
    pub mode: u32,
    pub links: u64,
    modified: i64,
    modified_ns: i64,
    changed: i64,
    changed_ns: i64,
}

pub(super) fn identity(metadata: &Metadata) -> Result<FileIdentity, SendFailure> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        Ok(FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            len: metadata.len(),
            uid: metadata.uid(),
            mode: metadata.mode(),
            links: metadata.nlink(),
            modified: metadata.mtime(),
            modified_ns: metadata.mtime_nsec(),
            changed: metadata.ctime(),
            changed_ns: metadata.ctime_nsec(),
        })
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Err(invalid())
    }
}

pub(super) fn directory(path: &Path, uid: u32, private: bool) -> Result<PathBuf, SendFailure> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    let path = path.canonicalize().map_err(|_| invalid())?;
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = std::fs::symlink_metadata(ancestor).map_err(|_| invalid())?;
        let info = identity(&metadata)?;
        let sticky = ancestor != path && info.uid == 0 && info.mode & 0o1000 != 0;
        if !metadata.is_dir()
            || (info.uid != 0 && info.uid != uid)
            || (info.mode & 0o022 != 0 && !sticky)
        {
            return Err(invalid());
        }
        acl(ancestor)?;
    }
    if private {
        let m = identity(&std::fs::metadata(&path).map_err(|_| invalid())?)?;
        if m.uid != uid || m.mode & 0o777 != 0o700 {
            return Err(invalid());
        }
    }
    Ok(path)
}

pub(super) fn canonical_missing(path: &Path) -> Result<PathBuf, SendFailure> {
    let mut at = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(|_| invalid())?.join(path)
    };
    let mut suffix = Vec::new();
    loop {
        match std::fs::symlink_metadata(&at) {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                suffix.push(at.file_name().ok_or_else(invalid)?.to_os_string());
                if !at.pop() {
                    return Err(invalid());
                }
            }
            Err(_) => return Err(invalid()),
        }
    }
    at = at.canonicalize().map_err(|_| invalid())?;
    for name in suffix.into_iter().rev() {
        if name == ".." {
            at.pop();
        } else if name != "." {
            at.push(name);
        }
    }
    Ok(at)
}

pub(super) fn open_source(
    root: &Path,
    root_identity: FileIdentity,
    relative: &Path,
    uid: u32,
) -> Result<(File, FileIdentity, PathBuf), SendFailure> {
    let current = identity(&std::fs::symlink_metadata(root).map_err(|_| invalid())?)?;
    if (current.device, current.inode) != (root_identity.device, root_identity.inode) {
        return Err(invalid());
    }
    acl(root)?;
    let mut path = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Err(invalid());
    }
    for (i, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(invalid());
        };
        path.push(name);
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| invalid())?;
        let info = identity(&metadata)?;
        if metadata.file_type().is_symlink()
            || (info.uid != uid && info.uid != 0)
            || info.mode & 0o022 != 0
        {
            return Err(invalid());
        }
        if i + 1 == components.len() {
            if !metadata.is_file() || info.links != 1 {
                return Err(invalid());
            }
            acl(&path)?;
            let file = File::open(&path).map_err(|_| invalid())?;
            if identity(&file.metadata().map_err(|_| invalid())?)? != info {
                return Err(invalid());
            }
            return Ok((file, info, path));
        }
        if !metadata.is_dir() {
            return Err(invalid());
        }
        acl(&path)?;
    }
    Err(invalid())
}

pub(super) fn create_private(path: &Path) -> Result<File, SendFailure> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|_| invalid())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(invalid())
    }
}

pub(super) fn secure_private(path: &Path, file: &File, uid: u32) -> Result<(), SendFailure> {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("/bin/chmod");
        command.arg("-N").arg(path);
        let _ = utility(command)?;
    }
    let info = identity(&file.metadata().map_err(|_| invalid())?)?;
    if info.uid != uid || info.mode & 0o777 != 0o600 || info.links != 1 {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn same_file(path: &Path, expected: FileIdentity) -> Result<(), SendFailure> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !metadata.is_file() || identity(&metadata)? != expected {
        return Err(invalid());
    }
    Ok(())
}
