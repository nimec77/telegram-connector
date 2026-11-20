//! Private path selection tests run with child-process environment isolation.
use super::*;

fn path_child_command(case: &str) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        .args([
            "--exact",
            "config::loading::tests::environment_child",
            "--ignored",
            "--test-threads=1",
        ])
        .env("CONFIG_TEST_CASE", case)
        .env_remove("TELEGRAM_MCP_CONFIG");
    command
}

fn assert_child_success(command: &mut std::process::Command) {
    let output = command.output().expect("config subprocess");
    assert!(
        output.status.success(),
        "config child failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "child entrypoint did not run"
    );
}

#[test]
fn test_resolve_path_from_env() {
    let mut command = path_child_command("path_env");
    command.env("TELEGRAM_MCP_CONFIG", "/custom/path/config.toml");
    assert_child_success(&mut command);
}
#[test]
fn test_resolve_path_default() {
    let dir = tempfile::tempdir().expect("isolated home");
    let mut command = path_child_command("path_default");
    command
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path());
    assert_child_success(&mut command);
}
#[test]
#[ignore = "subprocess entrypoint exercised by normal parent path tests"]
fn environment_child() {
    let case = std::env::var("CONFIG_TEST_CASE").expect("parent configured child");
    match case.as_str() {
        "path_env" => assert_eq!(
            Config::resolve_config_path().expect("env path"),
            PathBuf::from("/custom/path/config.toml")
        ),
        "path_default" => {
            let path = Config::resolve_config_path().expect("default config path");
            assert!(path.to_string_lossy().contains("telegram-connector"));
            assert!(path.ends_with("config.toml"));
        }
        _ => panic!("unknown path test case"),
    }
}
