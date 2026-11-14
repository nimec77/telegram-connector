use crate::config::{Config, OutputFormat};
use crate::sending::SendingRuntimeConfig;
use std::path::Path;

fn config() -> Config {
    toml::from_str("[telegram]\napi_id=1").expect("minimal config")
}

#[test]
fn sending_config_disabled_defaults_and_text_only_require_no_storage() {
    let mut config = config();
    assert!(!config.sending.enabled);
    assert!(config.sending.upload_roots.is_empty());
    assert!(config.sending.staging_root.is_none());
    assert_eq!(config.sending.max_image_bytes, 10_485_760);
    assert_eq!(config.sending.max_media_bytes, 104_857_600);
    assert_eq!(config.sending.max_staging_bytes, 209_715_200);
    assert_eq!(config.sending.max_concurrent_media, 2);
    assert_eq!(config.telegram.timeouts.send_secs, 30);
    assert_eq!(config.telegram.timeouts.upload_send_secs, 120);
    assert_eq!(config.rate_limiting.send_message_cost, 1);
    assert_eq!(config.rate_limiting.send_media_cost, 3);
    config.sending.enabled = true;
    assert!(
        SendingRuntimeConfig::try_from_config(&config, Path::new("/nonexistent/config.toml"))
            .is_ok()
    );
}

#[test]
fn sending_config_rejects_unsatisfiable_or_legacy_enabled_settings() {
    for change in 0..13 {
        let mut config = config();
        config.sending.enabled = true;
        match change {
            0 => config.server.output_format = OutputFormat::Legacy,
            1 => config.sending.max_image_bytes = 10_485_761,
            2 => config.sending.max_media_bytes = 104_857_601,
            3 => config.sending.max_image_bytes = 0,
            4 => config.sending.max_concurrent_media = 0,
            5 => config.sending.max_concurrent_media = 3,
            6 => config.sending.max_staging_bytes = 104_857_599,
            7 => config.telegram.timeouts.send_secs = 0,
            8 => config.telegram.timeouts.upload_send_secs = u64::MAX,
            9 => config.rate_limiting.send_message_cost = 0,
            10 => config.rate_limiting.send_media_cost = config.rate_limiting.max_tokens + 1,
            11 => {
                config
                    .sending
                    .upload_roots
                    .insert("outgoing".into(), "/tmp/files".into());
            }
            12 => {
                config
                    .sending
                    .upload_roots
                    .insert("../bad".into(), "/tmp/files".into());
                config.sending.staging_root = Some("/tmp/staging".into());
            }
            _ => unreachable!(),
        }
        assert!(
            SendingRuntimeConfig::try_from_config(&config, Path::new("/tmp/config.toml")).is_err(),
            "case {change}"
        );
    }
}

#[test]
fn sending_config_loader_rejects_enabled_legacy_and_duplicate_aliases() {
    let dir = tempfile::tempdir().expect("temporary config");
    let file = dir.path().join("config.toml");
    std::fs::write(
        &file,
        "[telegram]\napi_id=1\n[sending]\nenabled=true\n[server]\noutput_format='legacy'",
    )
    .expect("fixture");
    assert!(Config::load_from(Some(&file)).is_err());
    assert!(
        toml::from_str::<Config>(
            "[telegram]\napi_id=1\n[sending.upload_roots]\na='/tmp/a'\na='/tmp/b'"
        )
        .is_err()
    );
}
