use crate::sending::{StagingConfig, StagingSource};
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

pub(crate) struct Fixture {
    pub dir: tempfile::TempDir,
    pub source: PathBuf,
    pub staging: PathBuf,
    pub config: StagingConfig,
    pub protected_paths: Vec<PathBuf>,
    pub bytes: Vec<u8>,
}
impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("private temporary root");
        let source = dir.path().join("prepared");
        let staging = dir.path().join("staging");
        for path in [&source, &staging] {
            std::fs::create_dir(path).expect("fixture directory");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
                .expect("private mode");
        }
        let config = StagingConfig {
            upload_roots: [("outgoing".into(), source.clone())].into(),
            staging_root: Some(staging.clone()),
            ..StagingConfig::default()
        };
        let session = dir.path().join("secrets/session.db");
        let protected_paths = ["secrets/config.toml", "logs"]
            .into_iter()
            .map(|path| dir.path().join(path))
            .chain(["", "-wal", "-shm", "-journal"].into_iter().map(|suffix| {
                let mut path = session.as_os_str().to_os_string();
                path.push(suffix);
                PathBuf::from(path)
            }))
            .collect();
        let bytes = crate::test_helpers::create_test_jpeg(16, 16);
        std::fs::write(source.join("image.jpg"), &bytes).expect("prepared JPEG");
        Self {
            dir,
            source,
            staging,
            config,
            protected_paths,
            bytes,
        }
    }
    pub fn source(&self, name: &str) -> StagingSource {
        StagingSource::try_new("outgoing", name.as_ref()).expect("valid fixture source")
    }
}
