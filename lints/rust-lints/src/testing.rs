//! Test runner for UI tests that runs compiletest directly on the `ui/`
//! directory (no temp-dir copy), enabling `DYLINT_BLESS=1` support.

#[cfg(test)]
pub use runner::run_ui_test;

#[cfg(test)]
#[expect(
    unsafe_code,
    clippy::indexing_slicing,
    clippy::let_underscore_must_use,
    clippy::multiple_unsafe_ops_per_block,
    clippy::undocumented_unsafe_blocks,
    unclear_exports,
    reason = "test-only module; env var mutation, panics, and indexing are acceptable"
)]
mod runner {
    use std::env::{consts, var_os};
    use std::ffi::OsString;
    use std::fs::{read_dir, remove_file};
    use std::path::PathBuf;
    use std::sync::{LazyLock, Mutex, OnceLock};

    use cargo_metadata::{Metadata, Target, TargetKind};
    use compiletest_rs as compiletest;
    use regex::Regex;

    struct TestEnv {
        driver: PathBuf,
        linking_flags: Vec<String>,
    }

    static ENV: OnceLock<TestEnv> = OnceLock::new();
    static MUTEX: Mutex<()> = Mutex::new(());

    fn init() -> &'static TestEnv {
        ENV.get_or_init(|| {
            assert!(
                std::process::Command::new("cargo")
                    .arg("build")
                    .status()
                    .expect("cargo build")
                    .success(),
                "cargo build failed"
            );

            let metadata = cargo_metadata::MetadataCommand::new()
                .exec()
                .expect("cargo metadata");
            let dylint_library_path = metadata.target_directory.join("debug");

            unsafe {
                std::env::set_var("DYLINT_LIBRARY_PATH", &dylint_library_path);
            }

            let dylint_libs =
                dylint_testing::dylint_libs(env!("CARGO_PKG_NAME")).expect("dylint_libs");
            unsafe {
                std::env::set_var("CLIPPY_DISABLE_DOCS_LINKS", "true");
                std::env::set_var("DYLINT_LIBS", &dylint_libs);
            }

            let driver = dylint::driver_builder::get(
                &dylint::opts::Dylint::default(),
                env!("RUSTUP_TOOLCHAIN"),
            )
            .expect("driver");

            let linking_flags = compute_linking_flags(&metadata);

            TestEnv {
                driver,
                linking_flags,
            }
        })
    }

    fn compute_linking_flags(metadata: &Metadata) -> Vec<String> {
        let package = metadata
            .packages
            .iter()
            .find(|p| p.name == env!("CARGO_PKG_NAME"))
            .expect("own package");

        let example = package
            .targets
            .iter()
            .find(|t| t.kind.contains(&TargetKind::Example))
            .expect("at least one example target");

        remove_example_artifact(metadata, example);

        let output = std::process::Command::new("cargo")
            .args(["build", "--example", &example.name, "--verbose"])
            .env_remove("CARGO_TERM_COLOR")
            .output()
            .expect("cargo build --verbose");

        let stderr = String::from_utf8_lossy(&output.stderr);
        parse_linking_flags(&stderr, &example.name)
    }

    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*Running\s*`(.*)`$").expect("regex"));

    fn parse_linking_flags(stderr: &str, example_name: &str) -> Vec<String> {
        let crate_name = example_name.replace('-', "_");

        let rustc_args: Vec<String> = stderr
            .lines()
            .find_map(|line| {
                let caps = RE.captures(line)?;
                let args: Vec<String> = caps[1].split(' ').map(str::to_owned).collect();
                let is_rustc = std::path::Path::new(&args[0])
                    .file_stem()
                    .is_some_and(|s| s == "rustc");
                let matches_example = args
                    .windows(2)
                    .any(|w| w[0] == "--crate-name" && w[1] == crate_name);
                (is_rustc && matches_example).then_some(args)
            })
            .unwrap_or_else(|| panic!("no rustc invocation found for example `{example_name}`"));

        let mut flags = Vec::new();
        let mut iter = rustc_args.into_iter();
        while let Some(flag) = iter.next() {
            if flag.starts_with("--edition=") {
                flags.push(flag);
            } else if flag == "--extern" || flag == "-L" {
                if let Some(arg) = iter.next() {
                    flags.push(flag);
                    flags.push(arg.trim_matches('\'').to_owned());
                }
            }
        }
        flags
    }

    fn remove_example_artifact(metadata: &Metadata, target: &Target) {
        let examples = metadata.target_directory.join("debug/examples");
        let Ok(entries) = read_dir(&examples) else {
            return;
        };
        let target_name = target.name.replace('-', "_");
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == format!("{target_name}{}", consts::EXE_SUFFIX)
                || name.starts_with(&format!("{target_name}-"))
            {
                let _ = remove_file(entry.path());
            }
        }
    }

    /// Restores an env var on drop.
    struct VarGuard {
        key: &'static str,
        prev: Option<OsString>,
    }

    impl VarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, prev }
        }
    }

    impl Drop for VarGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => unsafe { std::env::set_var(self.key, v) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    /// Run a UI test on the original `ui/{example}/` directory.
    ///
    /// Set `DYLINT_BLESS=1` to update `.stderr` files in place.
    pub fn run_ui_test(example: &str, dylint_toml: Option<&str>, extra_rustc_flags: &[&str]) {
        let env = init();
        let _lock = MUTEX.lock().expect("test mutex poisoned");

        let _toml_guard = dylint_toml.map(|v| VarGuard::set("DYLINT_TOML", v));

        let src_base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ui")
            .join(example);

        let mut flags = env.linking_flags.join(" ");
        for f in extra_rustc_flags {
            flags.push(' ');
            flags.push_str(f);
        }
        flags.push_str(" --emit=metadata -Zui-testing");

        let bless = std::env::var_os("DYLINT_BLESS").is_some();

        let config = compiletest::Config {
            mode: compiletest::common::Mode::Ui,
            rustc_path: env.driver.clone(),
            src_base,
            target_rustcflags: Some(flags),
            bless,
            ..compiletest::Config::default()
        };

        compiletest::run_tests(&config);
    }
}
