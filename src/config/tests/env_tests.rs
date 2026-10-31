//! Pure expansion and subprocess-only environment tests.
use crate::config::env::expand_env_vars_with;
use crate::config::*;

fn expand(input: &str, vars: &[(&str, &str)]) -> anyhow::Result<String> {
    expand_env_vars_with(input, |name| {
        vars.iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string())
            .ok_or(std::env::VarError::NotPresent)
    })
}

#[test]
fn test_expand_env_vars_no_variables() {
    assert_eq!(
        expand("simple string", &[]).expect("literal"),
        "simple string"
    );
}
#[test]
fn test_expand_env_vars_single_variable() {
    assert_eq!(
        expand("prefix_${TEST_VAR}_suffix", &[("TEST_VAR", "test_value")]).expect("expansion"),
        "prefix_test_value_suffix"
    );
}
#[test]
fn test_expand_env_vars_multiple_variables() {
    assert_eq!(
        expand(
            "${VAR1}_middle_${VAR2}",
            &[("VAR1", "value1"), ("VAR2", "value2")]
        )
        .expect("expansion"),
        "value1_middle_value2"
    );
}
#[test]
fn test_expand_env_vars_missing_variable_returns_error() {
    let error = expand("${MISSING_VARIABLE}", &[]).expect_err("missing");
    assert!(error.to_string().contains("MISSING_VARIABLE"));
    assert!(error.to_string().contains("not found"));
    assert!(error.to_string().contains("line 1"));
}
#[test]
fn test_expand_env_vars_no_recursive_expansion() {
    assert_eq!(
        expand(
            "value_${OUTER_VAR}_end",
            &[
                ("OUTER_VAR", "${INNER_VAR}"),
                ("INNER_VAR", "should_not_appear")
            ]
        )
        .expect("expansion"),
        "value_${INNER_VAR}_end"
    );
}
#[test]
fn test_expand_env_vars_incomplete_syntax() {
    assert_eq!(
        expand_env_vars_with("${INCOMPLETE", |_| panic!(
            "incomplete syntax must not look up a variable"
        ))
        .expect("literal"),
        "${INCOMPLETE"
    );
}
#[test]
fn test_expand_env_vars_numeric_unquoting() {
    for (value, expected) in [
        ("12345", "api_id = 12345"),
        ("+12345", "api_id = \"+12345\""),
        ("-123", "api_id = \"-123\""),
        ("1.2", "api_id = \"1.2\""),
        ("", "api_id = \"\""),
    ] {
        assert_eq!(
            expand(r#"api_id = "${VALUE}""#, &[("VALUE", value)]).expect("expansion"),
            expected
        );
    }
}
#[test]
fn test_expand_env_vars_skips_toml_comment_lines() {
    let input = "# ${VAR_NAME} is documentation\napi_id = 12345";
    assert_eq!(
        expand_env_vars_with(input, |_| panic!("comment must not look up a variable"))
            .expect("comment"),
        input
    );
}
#[test]
fn test_expand_env_vars_skips_inline_comment_after_hash() {
    let input = "  # ${SHOULD_BE_IGNORED}";
    assert_eq!(
        expand_env_vars_with(input, |_| panic!("comment must not look up a variable"))
            .expect("comment"),
        input
    );
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
