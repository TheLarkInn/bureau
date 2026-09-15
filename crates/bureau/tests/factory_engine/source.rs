use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

pub fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, bytes).expect("write fixture");
}

pub fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("local git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn commit(repository: &Path) {
    git(repository, &["add", "-A"]);
    git(
        repository,
        &[
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "offline fixture",
        ],
    );
}

pub fn repository(root: &Path) -> PathBuf {
    let repository = root.join("repository");
    let provider = repository.join(".github/extensions/offline-provider");
    write(
        &provider.join("extension.mjs"),
        "// Offline fixture; never executed.\n",
    );
    write(
        &provider.join("factory.json"),
        br#"{
        "name":"offline-factory","description":"Offline lifecycle fixture","phases":[],
        "argsSchema":{"type":"object","required":["mode"],"additionalProperties":false,
            "properties":{"mode":{"type":"string"}}}
    }"#,
    );
    write(&repository.join("file.txt"), "unchanged\n");
    git(&repository, &["init", "-q", "-b", "main"]);
    commit(&repository);
    repository
}

fn distribution(runtime: &Path, mode: &str) {
    let dist = if mode == "root-dist" {
        runtime.to_path_buf()
    } else {
        runtime.join("dist")
    };
    for file in [
        "copilot-sdk/index.js",
        "copilot-sdk/extension.js",
        "preloads/extension_bootstrap.mjs",
        "preloads/extension_sdk_resolver.mjs",
    ] {
        write(
            &dist.join(file),
            "// Offline placeholder; never executed.\n",
        );
    }
}

pub fn runtime(root: &Path, mode: &str) -> PathBuf {
    let runtime = root.join("runtime-source");
    for (name, source) in [
        ("fake-runtime", include_str!("runtime.py")),
        ("authentication.py", include_str!("authentication.py")),
        ("gh.py", include_str!("gh.py")),
        ("behavior.py", include_str!("behavior.py")),
        ("catalog.py", include_str!("catalog.py")),
        ("lifetime.py", include_str!("lifetime.py")),
    ] {
        write(&runtime.join(name), source.replace("\r\n", "\n"));
    }
    write(&runtime.join("mode"), mode);
    std::fs::set_permissions(
        runtime.join("fake-runtime"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("executable fixture");
    distribution(&runtime, mode);
    runtime
}

pub fn writable(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let _ = std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700));
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            writable(&entry.path());
        }
    }
}

pub fn bureau_executable(root: &Path) {
    let executable = std::env::current_exe().expect("Bureau engine test executable");
    let executable = std::fs::canonicalize(executable).expect("canonical Bureau executable");
    write(
        &root.join("bureau-executable"),
        executable.to_string_lossy().as_bytes(),
    );
}
