//! Parsed-value expansion and subprocess-only environment tests.
use crate::config::env::parse_config_with;
use crate::config::*;
use secrecy::ExposeSecret;

fn parse(input: &str, vars: &[(&str, &str)]) -> anyhow::Result<Config> {
    parse_config_with(input, |name| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
            .ok_or(std::env::VarError::NotPresent)
    })
}

fn hash(config: &Config) -> &str {
    config
        .telegram
        .api_hash
        .as_ref()
        .expect("hash")
        .expose_secret()
}

#[test]
fn replacement_bytes_are_data_in_every_string_form() {
    let replacement = "\"'\\\n雪 # $ \n[sending]\nenabled = true\n${NEXT}";
    for delimiter in ["\"", "'", "\"\"\"", "'''"] {
        for template in ["${VALUE}", "prefix_${VALUE}_${VALUE}_suffix"] {
            let input = format!(
                "[telegram]\napi_id = 123\napi_hash = {delimiter}{template}{delimiter}\nphone_number = '+123'\n"
            );
            let config = parse(&input, &[("VALUE", replacement)]).expect("replacement is data");
            let expected = if template == "${VALUE}" {
                replacement.to_owned()
            } else {
                format!("prefix_{replacement}_{replacement}_suffix")
            };
            assert_eq!(hash(&config), expected);
            assert_eq!(config.telegram.api_id, 123);
            assert!(!config.sending.enabled);
            assert_eq!(
                config
                    .telegram
                    .phone_number
                    .as_ref()
                    .expect("phone")
                    .expose_secret(),
                "+123"
            );
        }
    }
}

#[test]
fn comments_and_keys_never_expand() {
    let input = r#"
# ${MISSING}
"${ROOT}" = [{ "${KEY}" = 'ignored' }] # ${INLINE}
[telegram] # ${HEADER_COMMENT}
api_id = 123 # ${INLINE}
api_hash = 'hash # ${VALUE}' # ${INLINE}
"${DOTTED}".child = 1
["${TABLE}".nested]
"${KEY}" = 'ignored'
[["${ARRAY_TABLE}"]]
ignored = 1
"#;
    let config = parse(input, &[("VALUE", "expanded")]).expect("only values expand");
    assert_eq!(hash(&config), "hash # expanded");
}

#[test]
fn numeric_looking_credentials_and_paths_remain_strings() {
    let input = "[telegram]\napi_id = 1\napi_hash = \"${VALUE}\"\nphone_number = '${VALUE}'\nsession_file = \"${VALUE}\"\n[logging]\nfile_path = '${VALUE}'\n";
    let config = parse(input, &[("VALUE", "12345")]).expect("string destinations");
    assert_eq!(hash(&config), "12345");
    assert_eq!(
        config
            .telegram
            .phone_number
            .as_ref()
            .expect("phone")
            .expose_secret(),
        "12345"
    );
    assert_eq!(config.telegram.session_file, std::path::Path::new("12345"));
    assert_eq!(config.logging.file_path, std::path::Path::new("12345"));
}

#[test]
fn whole_placeholders_support_each_scalar_family() {
    for quote in ["", "\"", "'", "\"\"\"", "'''"] {
        let value = format!("{quote}${{VALUE}}{quote}");
        let config = parse(
            &format!("[telegram]\napi_id = {value}\n"),
            &[("VALUE", "-2147483648")],
        )
        .expect("i32");
        assert_eq!(config.telegram.api_id, i32::MIN);
        let config = parse(
            &format!("[telegram]\napi_id = 1\n[logging]\nmax_log_days = {value}\n"),
            &[("VALUE", "4294967295")],
        )
        .expect("u32");
        assert_eq!(config.logging.max_log_days, u32::MAX);
        let config = parse(&format!("[telegram]\napi_id = 1\n[observability]\nslow_write_threshold_ms = {value}\nresponse_buffer_size = {value}\n"), &[("VALUE", "18446744073709551615")]).expect("u64 and usize on macOS");
        assert_eq!(config.observability.slow_write_threshold_ms, u64::MAX);
        assert_eq!(config.observability.response_buffer_size, usize::MAX);
        let config = parse(
            &format!("[telegram]\napi_id = 1\n[rate_limiting]\nrefill_rate = {value}\n"),
            &[("VALUE", "1.25e2")],
        )
        .expect("f64");
        assert_eq!(config.rate_limiting.refill_rate, 125.0);
        let config = parse(
            &format!("[telegram]\napi_id = 1\n[logging]\nfile_enabled = {value}\n"),
            &[("VALUE", "false")],
        )
        .expect("bool");
        assert!(!config.logging.file_enabled);
    }
}

#[test]
fn numeric_literals_keep_native_range_and_quoted_type_behavior() {
    let config = parse(
        "[telegram]\napi_id = 1\n[observability]\nslow_write_threshold_ms = 18446744073709551615\n",
        &[],
    )
    .expect("native u64");
    assert_eq!(config.observability.slow_write_threshold_ms, u64::MAX);
    assert!(parse("[telegram]\napi_id = '123'\n", &[]).is_err());
    assert!(
        parse(
            "[telegram]\napi_id = 1\n[logging]\nfile_enabled = 'true'\n",
            &[]
        )
        .is_err()
    );
}

#[test]
fn whole_scalar_replacements_reject_structure_trailing_syntax_and_wrong_types() {
    for replacement in [
        "[1]",
        "{ value = 1 }",
        "1\n[sending]\nenabled = true",
        "1 # comment",
        "1 extra",
        "'1'",
        "true",
        "1.5",
        "2147483648",
        "-2147483649",
        "",
    ] {
        for quote in ["", "\"", "'"] {
            let input =
                format!("[telegram]\napi_id = {quote}${{VALUE}}{quote}\napi_hash = 'unchanged'\n");
            assert!(
                parse(&input, &[("VALUE", replacement)]).is_err(),
                "accepted scalar {replacement:?}"
            );
        }
    }
    for (table, field, value) in [
        ("logging", "max_log_days", "4294967296"),
        ("logging", "max_log_days", "-1"),
        ("observability", "response_buffer_size", "-1"),
        (
            "observability",
            "slow_write_threshold_ms",
            "18446744073709551616",
        ),
        ("logging", "file_enabled", "1"),
        ("rate_limiting", "refill_rate", "true"),
    ] {
        assert!(
            parse(
                &format!("[telegram]\napi_id = 1\n[{table}]\n{field} = '${{VALUE}}'\n"),
                &[("VALUE", value)]
            )
            .is_err()
        );
    }
}

#[test]
fn incomplete_placeholders_stay_literal_and_replacements_are_not_rescanned() {
    let config = parse(
        "[telegram]\napi_id = 1\napi_hash = '${INCOMPLETE'\nsession_file = '~/${OUTER}'\n",
        &[("OUTER", "${NEXT}")],
    )
    .expect("literal and nonrecursive");
    assert_eq!(hash(&config), "${INCOMPLETE");
    assert_eq!(
        config.telegram.session_file,
        std::path::Path::new("~/${NEXT}")
    );
}

#[test]
fn unknown_fields_keep_existing_serde_policy() {
    assert!(
        parse(
            "[telegram]\napi_id = 1\nunknown = '${VALUE}'\n[unknown]\nvalue = '${VALUE}'\n",
            &[("VALUE", "data")]
        )
        .is_ok()
    );
    assert!(
        parse(
            "[telegram]\napi_id = 1\n[sending]\nunknown = '${VALUE}'\n",
            &[("VALUE", "data")]
        )
        .is_err()
    );
}

#[test]
fn missing_variables_use_original_physical_locations() {
    // Hand-counted original source positions, including decoded escapes and folding.
    let cases = [
        ("'${MISSING}'", 3, 13),
        ("\"\\n${MISSING}\"", 3, 15),
        ("\"\"\"\nfirst\n雪 ${MISSING}\n\"\"\"", 5, 3),
        ("'''\r\nfirst\r\n  ${MISSING}\r\n'''", 5, 3),
        ("\"\"\"\nfirst\\\n  \n   ${MISSING}\"\"\"", 6, 4),
        ("\"\\e\\x24{MISSING}\"", 3, 15),
        ("\"\\u0024{MISSING}\"", 3, 13),
        ("\"\\U0001f600\\U00000024{MISSING}\"", 3, 23),
        ("\"${FIRST}${MISSING}\"", 3, 21),
    ];
    for (value, line, column) in cases {
        let input = format!("[telegram]\napi_id = 1\napi_hash = {value}\n");
        let error =
            parse(&input, &[("FIRST", "first\nextra\nlines")]).expect_err("missing variable");
        let message = format!("{error:#}");
        assert!(message.contains("MISSING"), "{message}");
        assert!(
            message.contains(&format!("line {line}, column {column}")),
            "{value:?}: {message}"
        );
    }
}

#[test]
fn failed_child_cannot_leak_environment_into_parent() {
    let before = std::env::var_os("CONFIG_ISOLATION_PANIC_PROBE");
    let output = super::environment_child("panic")
        .env("CONFIG_ISOLATION_PANIC_PROBE", "child-only-value")
        .output()
        .expect("panic child");
    assert!(!output.status.success(), "child intentionally panics");
    assert!(String::from_utf8_lossy(&output.stdout).contains("intentional child isolation panic"));
    assert_eq!(std::env::var_os("CONFIG_ISOLATION_PANIC_PROBE"), before);
}
#[test]
#[ignore = "subprocess entrypoint exercised by normal parent tests"]
fn environment_child() {
    let case = std::env::var("CONFIG_TEST_CASE").expect("parent configured child");
    match case.as_str() {
        "panic" => {
            assert_eq!(
                std::env::var("CONFIG_ISOLATION_PANIC_PROBE").expect("probe"),
                "child-only-value"
            );
            // Narrow test-only panic verifies isolation also on failed assertions.
            panic!("intentional child isolation panic");
        }
        _ => super::load_tests::run_child_case(&case),
    }
}

#[cfg(unix)]
#[test]
fn non_unicode_env_secret_is_redacted_from_loader_diagnostics() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().expect("config dir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[telegram]\napi_id = 1\napi_hash = \"${NON_UNICODE_CONFIG_SECRET}\"\n",
    )
    .expect("config");
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "config::tests::env_tests::non_unicode_env_secret_child",
            "--ignored",
        ])
        .env("NON_UNICODE_CONFIG_PATH", &path)
        .env(
            "NON_UNICODE_CONFIG_SECRET",
            std::ffi::OsString::from_vec(b"FAKE_NON_UNICODE_SECRET_72\xff".to_vec()),
        )
        .output()
        .expect("child");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[cfg(unix)]
#[test]
#[ignore = "subprocess entrypoint invoked by the normal parent regression"]
fn non_unicode_env_secret_child() {
    let path = std::env::var_os("NON_UNICODE_CONFIG_PATH").expect("parent configured child");
    let err = Config::load_from(Some(std::path::Path::new(&path)))
        .expect_err("non-Unicode secret must fail")
        .context("configuration startup failed");
    let mut diagnostics = vec![
        err.to_string(),
        format!("{err:#}"),
        format!("{err:?}"),
        format!("{err:#?}"),
    ];
    for cause in err.chain() {
        diagnostics.push(cause.to_string());
        diagnostics.push(format!("{cause:?}"));
    }
    assert!(
        diagnostics
            .iter()
            .all(|text| !text.contains("FAKE_NON_UNICODE_SECRET_72")),
        "secret-bearing source must be discarded"
    );
    assert!(format!("{err:#}").contains("NON_UNICODE_CONFIG_SECRET"));
    assert!(format!("{err:#}").contains("line 3"));
}

#[test]
fn placeholders_work_in_dotted_and_inline_scalar_destinations() {
    let input = r#"
telegram = { api_id = ${ID}, timeouts = { resolve_secs = ${SECS} }, api_hash = '${HASH}' }
sending.enabled = ${BOOL}
logging = { max_log_days = '${DAYS}', file_enabled = '${BOOL}' }
ignored = ["${HASH}", { nested = '${HASH}' }]
"#;
    let config = parse(
        input,
        &[
            ("ID", "+42"),
            ("SECS", "0x1e"),
            ("HASH", "123"),
            ("BOOL", "false"),
            ("DAYS", "1_000"),
        ],
    )
    .expect("nested scalar destinations");
    assert_eq!(config.telegram.api_id, 42);
    assert_eq!(config.telegram.timeouts.resolve_secs, 30);
    assert_eq!(hash(&config), "123");
    assert_eq!(config.logging.max_log_days, 1000);
    assert!(!config.sending.enabled);
}

#[test]
fn bare_placeholders_never_legitimize_keys_headers_or_partial_values() {
    for input in [
        "[telegram]\napi_id = 1\n${KEY} = 1\n",
        "[${TABLE}]\napi_id = 1\n",
        "[telegram]\napi_id = prefix${VALUE}\n",
        "[telegram]\napi_id = ${VALUE}suffix\n",
        "[telegram]\napi_id = ${VALUE}\"suffix\"\n",
    ] {
        assert!(
            parse(
                input,
                &[("KEY", "ignored"), ("TABLE", "telegram"), ("VALUE", "1")]
            )
            .is_err()
        );
    }
}

#[test]
fn errors_discard_parser_secrets_and_keep_original_locations() {
    for (input, vars, line) in [
        (
            "[telegram]\napi_id = 1\napi_hash = '${VALUE}'\n[logging]\nfile_enabled = 'SECRET_TOKEN'\n",
            vec![("VALUE", "\n\n\nSECRET_REPLACEMENT")],
            5,
        ),
        (
            "[telegram]\napi_id = '${VALUE}'\n",
            vec![("VALUE", "SECRET_REPLACEMENT")],
            2,
        ),
        (
            "[telegram]\napi_id = 1\n'SECRET_TOKEN' = 1\n'SECRET_TOKEN' = 2\n",
            vec![],
            4,
        ),
        (
            "[telegram]\napi_id = 1\napi_hash = 'SECRET_TOKEN';\n",
            vec![],
            3,
        ),
        (
            "[telegram]\napi_id = 1\n[sending]\n'SECRET_TOKEN' = true\n",
            vec![],
            4,
        ),
    ] {
        let error = parse(input, &vars).expect_err("invalid configuration");
        let message = format!("{error:#}");
        assert!(
            message.contains(&format!("line {line}, column ")),
            "{message}"
        );
        assert_safe_diagnostics(&error, &["SECRET_TOKEN", "SECRET_REPLACEMENT"]);
    }
}

fn assert_safe_diagnostics(error: &anyhow::Error, secrets: &[&str]) {
    let mut diagnostics = vec![
        error.to_string(),
        format!("{error:#}"),
        format!("{error:?}"),
        format!("{error:#?}"),
    ];
    for cause in error.chain() {
        diagnostics.push(cause.to_string());
        diagnostics.push(format!("{cause:?}"));
    }
    for text in diagnostics {
        assert!(
            secrets.iter().all(|secret| !text.contains(secret)),
            "diagnostics leaked a fixture secret"
        );
    }
}

#[test]
fn all_declared_scalar_destinations_expand_by_type() {
    let input = r#"
[telegram]
api_id = '${INT}'
max_download_bytes = '${INT}'
[telegram.timeouts]
send_secs = '${INT}'
upload_send_secs = '${INT}'
resolve_secs = '${INT}'
history_secs = '${INT}'
search_secs = '${INT}'
download_secs = '${INT}'
[rate_limiting]
send_message_cost = '${INT}'
send_media_cost = '${INT}'
max_tokens = '${INT}'
media_download_cost = '${INT}'
transcription_cost = '${INT}'
refill_rate = '${FLOAT}'
[transcription]
default_timeout_seconds = '${INT}'
max_timeout_seconds = '${INT}'
[logging]
max_log_days = '${INT}'
file_enabled = '${BOOL}'
[search]
deadline_seconds = '${INT}'
[server]
shutdown_timeout_seconds = '${INT}'
max_request_bytes = '${INT}'
max_in_flight_requests = '${INT}'
[sending]
max_image_bytes = '${INT}'
max_media_bytes = '${INT}'
max_staging_bytes = '${INT}'
max_concurrent_media = '${INT}'
enabled = '${BOOL}'
[limits]
response_byte_budget = '${INT}'
media_batch_max_total_bytes = '${INT}'
[observability]
slow_write_threshold_ms = '${INT}'
response_buffer_size = '${INT}'
max_buffered_payload_bytes = '${INT}'
[media_artifacts]
ttl_seconds = '${INT}'
max_total_bytes = '${INT}'
max_entries = '${INT}'
enabled = '${BOOL}'
file_export_enabled = '${BOOL}'
"#;
    for input in [
        input.to_owned(),
        input.replace("'${", "${").replace("}'", "}"),
    ] {
        let config = parse(&input, &[("INT", "7"), ("FLOAT", "2.5"), ("BOOL", "true")])
            .expect("all declared scalar destinations");
        let integers = [
            config.telegram.api_id as u64,
            config.telegram.max_download_bytes,
            config.telegram.timeouts.send_secs,
            config.telegram.timeouts.upload_send_secs,
            config.telegram.timeouts.resolve_secs,
            config.telegram.timeouts.history_secs,
            config.telegram.timeouts.search_secs,
            config.telegram.timeouts.download_secs,
            config.rate_limiting.send_message_cost as u64,
            config.rate_limiting.send_media_cost as u64,
            config.rate_limiting.max_tokens as u64,
            config.rate_limiting.media_download_cost as u64,
            config.rate_limiting.transcription_cost as u64,
            config.transcription.default_timeout_seconds as u64,
            config.transcription.max_timeout_seconds as u64,
            config.logging.max_log_days as u64,
            config.search.deadline_seconds,
            config.server.shutdown_timeout_seconds,
            config.server.max_request_bytes as u64,
            config.server.max_in_flight_requests as u64,
            config.sending.max_image_bytes,
            config.sending.max_media_bytes,
            config.sending.max_staging_bytes,
            config.sending.max_concurrent_media as u64,
            config.limits.response_byte_budget,
            config.limits.media_batch_max_total_bytes,
            config.observability.slow_write_threshold_ms,
            config.observability.response_buffer_size as u64,
            config.observability.max_buffered_payload_bytes as u64,
            config.media_artifacts.ttl_seconds,
            config.media_artifacts.max_total_bytes,
            config.media_artifacts.max_entries as u64,
        ];
        assert!(integers.iter().all(|&value| value == 7));
        assert_eq!(config.rate_limiting.refill_rate, 2.5);
        assert!(
            config.logging.file_enabled
                && config.sending.enabled
                && config.media_artifacts.enabled
                && config.media_artifacts.file_export_enabled
        );
    }
}

#[test]
fn quoted_dots_in_keys_do_not_impersonate_scalar_paths() {
    let input = r#"
[telegram]
api_id = 1
[sending.upload_roots]
"telegram.api_id" = '${VALUE}'
"${KEY}" = '${VALUE}'
"#;
    let config = parse(input, &[("VALUE", "123")]).expect("map values are strings");
    assert_eq!(
        config.sending.upload_roots["telegram.api_id"],
        std::path::Path::new("123")
    );
    assert_eq!(
        config.sending.upload_roots["${KEY}"],
        std::path::Path::new("123")
    );
}

#[test]
fn decoded_escapes_delimiters_and_crlf_are_preserved_before_interpolation() {
    let cases = [
        (
            r#""\e\b\f\n\r\t\\\"\x24{VALUE}""#,
            "\u{1b}\u{8}\u{c}\n\r\t\\\"replacement",
        ),
        (
            "\"\"\"\r\nfirst\r\n${VALUE}\"\"\"\"\"",
            "first\r\nreplacement\"\"",
        ),
        ("'''\r\nfirst\r\n${VALUE}'''''", "first\r\nreplacement''"),
        (
            "\"\"\"\nfirst\\ \t\r\n \n  ${VALUE}\"\"\"",
            "firstreplacement",
        ),
        (r#""\u0024\u007bVALUE\u007d""#, "replacement"),
        (r#""😀\U00000024{VALUE}""#, "😀replacement"),
    ];
    for (value, expected) in cases {
        let config = parse(
            &format!("[telegram]\napi_id = 1\napi_hash = {value}\n"),
            &[("VALUE", "replacement")],
        )
        .expect("authoritative TOML decoding");
        assert_eq!(hash(&config), expected);
    }
}

#[test]
fn multiline_and_escaped_placeholder_locations_are_original() {
    let cases = [
        ("\"\"\"\r\nfirst\r\n  ${MISSING}\"\"\"", 5, 3),
        ("\"\"\"\nfirst\\ \t\r\n \n  ${MISSING}\"\"\"", 6, 3),
        (r#""\e\b\f\n\r\t\\\"\x24{MISSING}""#, 3, 29),
        ("'''one''two ${MISSING}'''''", 3, 24),
        (r#""\u0024\u007bMISSING\u007d""#, 3, 13),
        (r#""😀\U00000024{MISSING}""#, 3, 14),
    ];
    for (value, line, column) in cases {
        let error = parse(
            &format!("[telegram]\napi_id = 1\napi_hash = {value}\n"),
            &[],
        )
        .expect_err("missing variable");
        assert!(
            format!("{error:#}").contains(&format!("line {line}, column {column}")),
            "{value:?}: {error:#}"
        );
    }
}

#[test]
fn bare_whole_placeholders_keep_string_and_unknown_destinations_as_data() {
    let input = "ignored = ${VALUE}\n[telegram]\napi_id = 1\napi_hash = ${VALUE}\nsession_file = ${VALUE}\nunknown = ${VALUE}\n";
    let config = parse(input, &[("VALUE", "12345")]).expect("string and unknown destinations");
    assert_eq!(hash(&config), "12345");
    assert_eq!(config.telegram.session_file, std::path::Path::new("12345"));
    let config = parse(
        input,
        &[("VALUE", "\"\n[sending]\nenabled = true\n${NEXT}")],
    )
    .expect("bare replacement remains data");
    assert!(!config.sending.enabled);
    assert_eq!(hash(&config), "\"\n[sending]\nenabled = true\n${NEXT}");
}

#[test]
fn bare_missing_variable_reports_original_column_after_unicode() {
    let error =
        parse("[telegram]\napi_id = 1\n'雪' = ${MISSING}\n", &[]).expect_err("missing variable");
    assert!(format!("{error:#}").contains("line 3, column 7"));
}

#[cfg(unix)]
#[test]
fn non_unicode_lookup_error_uses_multiline_source_location_without_secret_chain() {
    use std::os::unix::ffi::OsStringExt;
    let error = parse_config_with(
        "[telegram]\napi_id = 1\napi_hash = \"\"\"\nfirst\n  \\x24{INVALID_UNICODE}\"\"\"\n",
        |_| {
            Err(std::env::VarError::NotUnicode(
                std::ffi::OsString::from_vec(b"SECRET_REPLACEMENT\xff".to_vec()),
            ))
        },
    )
    .expect_err("non-Unicode fixture");
    assert!(format!("{error:#}").contains("INVALID_UNICODE"));
    assert!(format!("{error:#}").contains("line 5, column 3"));
    assert_safe_diagnostics(&error, &["SECRET_REPLACEMENT"]);
}
