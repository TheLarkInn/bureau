use std::collections::BTreeMap;
use std::process::Command;

use bureau::adapters::{claude, copilot, runtime_env};
use bureau::contract::{SCHEMA_VERSION, StepOutcome, StepRequest, Trust, WorkItem};
use bureau::runlog::{EventKind, read_events};

use super::rig::{Rig, det_step};

const VALUES: [(&str, &str); 9] = [
    ("HOME", "/nonexistent/bureau-runtime"),
    ("COPILOT_HOME", "/nonexistent/bureau-runtime/copilot"),
    ("CLAUDE_CONFIG_DIR", "/nonexistent/bureau-runtime/claude"),
    ("XDG_CONFIG_HOME", "/nonexistent/bureau-runtime/config"),
    ("CARGO_HOME", "/opt/bureau/rust/cargo"),
    ("RUSTUP_HOME", "/opt/bureau/rust/rustup"),
    ("CARGO_NET_OFFLINE", "true"),
    ("RUSTUP_AUTO_INSTALL", "0"),
    ("BUREAU_RUST_IDENTITY", "daemon-runtime-sentinel"),
];

const REJECTED: [&str; 10] = [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "CARGO_REGISTRIES_CRATES_IO_TOKEN",
    "RUSTUP_TOOLCHAIN",
    "RUSTC",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "DYLINT_DRIVER_PATH",
];

fn values(mode: &str) -> BTreeMap<String, String> {
    let mut values: BTreeMap<_, _> = VALUES
        .into_iter()
        .map(|(key, value)| {
            let value = if mode == "empty" { "" } else { value };
            (key.to_owned(), value.to_owned())
        })
        .collect();
    values.insert("PATH".to_owned(), std::env::var("PATH").expect("test PATH"));
    values
}

fn step_stdout(rig: &Rig, run: &str) -> String {
    let events = read_events(&rig.dir.path().join("runs").join(run)).expect("run events");
    let finished = events
        .iter()
        .find(|event| event.kind == EventKind::StepFinished)
        .expect("deterministic step finished");
    finished.data["result"]["outputs"]["stdout"]
        .as_str()
        .expect("captured step output")
        .to_owned()
}

fn observed(rig: &Rig, run: &str) -> BTreeMap<String, String> {
    let mut environment: BTreeMap<_, _> = step_stdout(rig, run)
        .lines()
        .map(|line| {
            let (key, value) = line.split_once('=').expect("environment entry");
            (key.to_owned(), value.to_owned())
        })
        .collect();
    for key in ["PWD", "SHLVL", "_", "BUREAU_PROCESS_TOKEN"] {
        environment.remove(key);
    }
    environment
}

fn request(rig: &Rig) -> StepRequest {
    StepRequest {
        schema: SCHEMA_VERSION.to_owned(),
        run_id: "runtime".to_owned(),
        step: "environment".to_owned(),
        worktree: rig.dir.path().to_path_buf(),
        item: WorkItem::default(),
        trust: Trust::Derived,
        inputs: BTreeMap::new(),
        artifacts: BTreeMap::new(),
    }
}

fn agent_environments(rig: &Rig) -> [BTreeMap<String, String>; 2] {
    let plan = rig.plan(vec![det_step("environment", "env", Some("done"))]);
    let role = &plan.roles["worker"];
    let step = &plan.pipeline.steps[0];
    let request = request(rig);
    [
        copilot::spawn_request(role, step, &request, Vec::new(), None).env,
        claude::spawn_request(role, step, &request, Vec::new(), None).env,
    ]
}

async fn check_forwarding(mode: &str) {
    let mut expected = values(mode);
    expected.retain(|_, value| !value.is_empty());
    let rig = Rig::new();
    assert_eq!(
        (runtime_env(), agent_environments(&rig)),
        (expected.clone(), [expected.clone(), expected.clone()])
    );
    let plan = rig.plan(vec![det_step("environment", "env", Some("done"))]);
    let result = rig.engine().run(&plan).await;
    assert_eq!(
        (result.outcome, observed(&rig, &result.run_id)),
        (StepOutcome::NoWork, expected),
        "{result:?}"
    );
}

fn single_test_passed(stdout: &[u8]) -> bool {
    String::from_utf8_lossy(stdout).lines().any(|line| {
        line.starts_with("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; ")
    })
}

#[test]
fn isolated_proof_requires_one_executed_test() {
    for (counts, expected) in [
        ("ok. 1 passed; 0 failed; 0 ignored", true),
        ("ok. 0 passed; 0 failed; 0 ignored", false),
        ("ok. 0 passed; 0 failed; 1 ignored", false),
        ("FAILED. 0 passed; 1 failed; 0 ignored", false),
        ("incomplete", false),
    ] {
        let summary = format!("test result: {counts}; 0 measured; 16 filtered out;");
        assert_eq!(single_test_passed(summary.as_bytes()), expected);
    }
}

fn isolated(mode: &str) {
    let result = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "runtime_environment::explicit_runtime_survives_the_engine_boundary",
            "--nocapture",
        ])
        .env_clear()
        .envs(values(mode))
        .envs(REJECTED.map(|key| (key, "not-authorized")))
        .env("TMPDIR", std::env::temp_dir())
        .env("BUREAU_RUNTIME_ENV_TEST", mode)
        .output()
        .expect("isolated runtime test");
    assert!(
        result.status.success() && single_test_passed(&result.stdout),
        "{result:?}"
    );
}

#[test]
fn explicit_runtime_survives_the_engine_boundary() {
    if let Ok(mode) = std::env::var("BUREAU_RUNTIME_ENV_TEST") {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("offline runtime")
            .block_on(check_forwarding(&mode));
    } else {
        for mode in ["present", "empty"] {
            isolated(mode);
        }
    }
}

const OFFLINE_CHECK: &str = r#"node --input-type=module -e '
const { default: assert } = await import("node:assert/strict");
const { workspace, runCheck } = await import("./scripts/maintenance-checks.mjs");
const { loadPolicy } = await import("./scripts/maintenance-policy.mjs");
const { requirePreparedRust } = await import("./scripts/maintenance-rust.mjs");
const { descriptorSnapshot } = await import("./scripts/maintenance-mount.mjs");
const policy = await loadPolicy();
assert.equal(process.getuid(), 0);
assert.notEqual((await descriptorSnapshot(policy.rust_bin)).metadata.uid, 0n);
const stale = JSON.parse(process.env.BUREAU_RUST_IDENTITY);
stale.entries[0][2] = (BigInt(stale.entries[0][2]) + 1n).toString();
await assert.rejects(requirePreparedRust(process.cwd(), policy, {
  environment: { ...process.env, BUREAU_RUST_IDENTITY: JSON.stringify(stale) },
}), /identity receipt differs/u);
const source = { commit: workspace().commit, category: "chaos",
  id: `TheLarkInn/bureau#${policy.source_issues.chaos}`, cycle: "offline-runtime-qualification" };
const { evidence, log } = await runCheck(source, policy, { seed: 0 });
if (!evidence.complete || evidence.checks !== 1 || evidence.findings.length) {
  throw new Error("the real offline maintenance check did not pass");
}
console.log(JSON.stringify(evidence));
console.log("rust-identity-proof: matched unmapped owners; rejected mismatched inode");
console.log(log);
'"#;

const PREPARE_RUST: &str = r#"
const { loadPolicy } = await import("./scripts/maintenance-policy.mjs");
const { requirePreparedRust } = await import("./scripts/maintenance-rust.mjs");
const result = await requirePreparedRust(process.cwd(), await loadPolicy(), { rootOwner: true });
console.log(result.BUREAU_RUST_IDENTITY);
"#;

fn source_root() -> &'static std::path::Path {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("source root")
}

fn startup_identity() -> String {
    let result = Command::new("node")
        .args([
            "--max-old-space-size=64",
            "--input-type=module",
            "-e",
            PREPARE_RUST,
        ])
        .current_dir(source_root())
        .env_clear()
        .envs(runtime_env())
        .env("TMPDIR", std::env::temp_dir())
        .output()
        .expect("host-root runtime admission");
    assert!(result.status.success(), "{result:?}");
    String::from_utf8(result.stdout)
        .expect("identity receipt")
        .trim()
        .to_owned()
}

fn isolated_native() {
    let result = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "runtime_environment::offline_tools_execute_through_engine_and_maintenance_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .envs(runtime_env())
        .env("BUREAU_RUST_IDENTITY", startup_identity())
        .env("BUREAU_NATIVE_QUALIFICATION_CHILD", "1")
        .env("TMPDIR", std::env::temp_dir())
        .output()
        .expect("isolated native runtime test");
    assert!(
        result.status.success() && single_test_passed(&result.stdout),
        "{result:?}"
    );
    println!("{}", String::from_utf8_lossy(&result.stdout));
}

async fn qualify_offline_tools() {
    let rig = Rig::new();
    let mut step = det_step("offline-runtime", OFFLINE_CHECK, Some("done"));
    step.timeout_secs = Some(360);
    let mut plan = rig.plan(vec![step]);
    plan.repos.get_mut("main").expect("fixture repo").url =
        source_root().to_string_lossy().into_owned();
    let result = rig.engine().run(&plan).await;
    assert_eq!(result.outcome, StepOutcome::NoWork, "{result:?}");
    let output = step_stdout(&rig, &result.run_id);
    assert!(
        output.contains("bureau-maintenance-evidence-v1"),
        "{output}"
    );
    println!("{output}");
}

#[tokio::test]
#[ignore = "Requires the provisioned immutable maintenance runtime and a bounded native resource slot"]
async fn offline_tools_execute_through_engine_and_maintenance_child() {
    if std::env::var_os("BUREAU_NATIVE_QUALIFICATION_CHILD").is_some() {
        qualify_offline_tools().await;
    } else {
        isolated_native();
    }
}
