use crate::mcp::tools::types::sending::UploadSource;
use crate::sending::SendingRuntimeConfig;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

pub(crate) struct Fixture {
    pub dir: tempfile::TempDir,
    pub source: PathBuf,
    pub staging: PathBuf,
    pub config: SendingRuntimeConfig,
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
        let mut config: crate::Config = toml::from_str("[telegram]\napi_id=1").expect("config");
        config.sending.enabled = true;
        config
            .sending
            .upload_roots
            .insert("outgoing".into(), source.clone());
        config.sending.staging_root = Some(staging.clone());
        config.telegram.session_file = dir.path().join("secrets/session.db");
        config.logging.file_path = dir.path().join("logs");
        let config =
            SendingRuntimeConfig::try_from_config(&config, &dir.path().join("secrets/config.toml"))
                .expect("runtime settings");
        let bytes = crate::test_helpers::create_test_jpeg(16, 16);
        std::fs::write(source.join("image.jpg"), &bytes).expect("prepared JPEG");
        Self {
            dir,
            source,
            staging,
            config,
            bytes,
        }
    }
    pub fn source(&self, name: &str) -> UploadSource {
        UploadSource {
            root: "outgoing".into(),
            relative_path: name.into(),
        }
    }
}
