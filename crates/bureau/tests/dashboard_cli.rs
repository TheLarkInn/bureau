//! Binary-level dashboard launcher tests with a fake Node executable.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let suffix = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("bureau-dashboard-{}-{suffix}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let node = root.join("node");
    let server = root.join("serve.mjs");
    let capture = root.join("arguments");
    std::fs::write(
        &node,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$BUREAU_DASHBOARD_CAPTURE\"\n",
    )
    .expect("write fake node");
    std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755))
        .expect("make fake node executable");
    std::fs::write(&server, "").expect("write server");
    (node, server, capture)
}

fn dashboard(root: &Path, args: &[&str]) -> (Output, String) {
    let (node, server, capture) = fixture(root);
    let output = Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(["dashboard", "--node"])
        .arg(node)
        .args(["--server"])
        .arg(server)
        .args(args)
        .env("BUREAU_DASHBOARD_CAPTURE", &capture)
        .output()
        .expect("run dashboard");
    let captured = std::fs::read_to_string(capture).expect("read arguments");
    (output, captured)
}

fn asset_node(root: &Path) -> (PathBuf, PathBuf) {
    let node = root.join("asset-node");
    let capture = root.join("asset-arguments");
    let script = "#!/bin/sh\nroot=$(dirname \"$1\")\ntest -f \"$root/web/index.html\" || exit 9\nprintf '%s\\n' \"$@\" > \"$BUREAU_DASHBOARD_CAPTURE\"\n";
    std::fs::write(&node, script).expect("write asset node");
    std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755))
        .expect("make asset node executable");
    (node, capture)
}

#[test]
fn dashboard_forwards_browser_server_options() {
    let dir = TestDir::new();
    let config = dir.0.join("config");
    let config_text = config.to_string_lossy();
    let args = ["--dir", &config_text, "--pipeline", "review"];
    let (output, captured) = dashboard(&dir.0, &args);
    let expected = ["--dir", &config_text, "--pipeline", "review", "--open"];
    assert!(
        output.status.success()
            && expected
                .iter()
                .all(|value| captured.lines().any(|line| line == *value)),
        "{captured}"
    );
}

#[test]
fn dashboard_forwards_runtime_options() {
    let dir = TestDir::new();
    let args = ["--dev", "--no-open", "--port", "7331"];
    let (output, captured) = dashboard(&dir.0, &args);
    let expected = ["--dev", "--port", "7331"];
    assert!(
        output.status.success()
            && expected
                .iter()
                .all(|value| captured.lines().any(|line| line == *value))
            && !captured.lines().any(|line| line == "--open"),
        "{captured}"
    );
}

#[test]
fn dashboard_rejects_port_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(["dashboard", "--port", "0"])
        .output()
        .expect("run dashboard");
    assert!(!output.status.success());
}

#[test]
fn dashboard_materializes_embedded_assets() {
    let dir = TestDir::new();
    let (node, capture) = asset_node(&dir.0);
    let output = Command::new(env!("CARGO_BIN_EXE_bureau"))
        .args(["dashboard", "--node"])
        .arg(node)
        .arg("--no-open")
        .env("BUREAU_HOME", &dir.0)
        .env("BUREAU_DASHBOARD_CAPTURE", &capture)
        .output()
        .expect("run embedded dashboard");
    let server = std::fs::read_to_string(capture).expect("read embedded arguments");
    assert!(
        output.status.success()
            && server
                .lines()
                .next()
                .is_some_and(|path| path.contains("/dashboard/")),
        "{server}"
    );
}
