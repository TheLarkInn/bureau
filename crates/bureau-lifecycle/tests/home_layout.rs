//! Offline tests for local home resolution and its fixed layout.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use bureau_lifecycle::home::{BUREAU_HOME, Directory, Environment, Error, Home};

#[derive(Default)]
struct TestEnvironment {
    values: BTreeMap<String, OsString>,
}

impl TestEnvironment {
    fn with(mut self, name: &str, value: &str) -> Self {
        self.values.insert(name.to_owned(), OsString::from(value));
        self
    }
}

impl Environment for TestEnvironment {
    fn value(&self, name: &str) -> Option<OsString> {
        self.values.get(name).cloned()
    }
}

#[test]
fn override_selects_the_complete_fixed_layout() {
    let environment = TestEnvironment::default()
        .with("HOME", "/users/ignored")
        .with(BUREAU_HOME, "/var/lib/bureau");
    let home = Home::from_environment(&environment).expect("resolve override");
    let layout = home.layout();
    let actual = [
        layout.root(),
        layout.settings(),
        layout.credentials(),
        layout.state_db(),
        layout.runs(),
        layout.checkout_cache(),
        layout.config_cache(),
    ];
    let expected = expected_paths("/var/lib/bureau");
    assert_eq!(actual, expected.each_ref().map(PathBuf::as_path));
}

#[test]
fn user_home_defaults_to_dot_bureau() {
    let environment = TestEnvironment::default().with("HOME", "/users/test");
    let home = Home::from_environment(&environment).expect("resolve default");
    assert_eq!(home.layout().root(), PathBuf::from("/users/test/.bureau"));
}

#[test]
fn invalid_environment_is_reported_without_io() {
    let missing = Home::from_environment(&TestEnvironment::default());
    let empty = TestEnvironment::default().with(BUREAU_HOME, "");
    let empty = Home::from_environment(&empty);
    assert_eq!(
        (missing, empty),
        (Err(Error::MissingUserHome), Err(Error::EmptyOverride))
    );
}

#[test]
fn expected_directories_map_only_to_layout_directories() {
    let home = Home::new(PathBuf::from("local"));
    let actual = Directory::ALL
        .map(|directory| (directory, home.layout().directory(directory).to_path_buf()));
    let expected = [
        (Directory::Home, PathBuf::from("local")),
        (Directory::Credentials, PathBuf::from("local/credentials")),
        (Directory::Runs, PathBuf::from("local/runs")),
        (
            Directory::CheckoutCache,
            PathBuf::from("local/checkout-cache"),
        ),
        (Directory::ConfigCache, PathBuf::from("local/config-cache")),
    ];
    assert_eq!(actual, expected);
}

fn expected_paths(root: &str) -> [PathBuf; 7] {
    let root = PathBuf::from(root);
    [
        root.clone(),
        root.join("settings.yaml"),
        root.join("credentials"),
        root.join("state.db"),
        root.join("runs"),
        root.join("checkout-cache"),
        root.join("config-cache"),
    ]
}
