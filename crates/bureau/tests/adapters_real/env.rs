use bureau::adapters::{AdapterKind, claude};
use bureau::config::Permission;
use bureau::process::Secret;

use super::{TestDir, copilot_request, request, role, step};

const RUNTIME_VARS: [&str; 9] = [
    "PATH",
    "HOME",
    "COPILOT_HOME",
    "CLAUDE_CONFIG_DIR",
    "XDG_CONFIG_HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "CARGO_NET_OFFLINE",
    "RUSTUP_AUTO_INSTALL",
];

// Reading env is safe; setting it is `unsafe` on edition 2024, so these
// assertions adapt to whatever the test process already has.
#[test]
fn copilot_forwards_and_scrubs_only_known_values() {
    let dir = TestDir::new("env");
    let role = role("/p:a", AdapterKind::Copilot, &[Permission::RepoRead]);
    let req = copilot_request(&role, &step(None), dir.path());
    let token = std::env::var("GH_TOKEN").unwrap_or_default();
    let expected = !token.is_empty();
    let seen = (
        req.env
            .keys()
            .all(|key| key == "GH_TOKEN" || RUNTIME_VARS.contains(&key.as_str())),
        req.env.contains_key("GH_TOKEN"),
        req.secrets.contains(&Secret::new(&token)),
    );
    assert_eq!(seen, (true, expected, expected));
}

#[test]
fn claude_forwards_runtime_and_model_environment_only() {
    let dir = TestDir::new("claude-env");
    let role = role("/p:a", AdapterKind::Claude, &[Permission::ModelInvoke]);
    let secrets = vec![Secret::new("engine-secret")];
    let req = claude::spawn_request(&role, &step(None), &request(dir.path()), secrets, None);
    let known = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"];
    let seen = (
        req.env
            .keys()
            .all(|key| known.contains(&key.as_str()) || RUNTIME_VARS.contains(&key.as_str())),
        req.secrets.contains(&Secret::new("engine-secret")),
    );
    assert_eq!(seen, (true, true));
}
