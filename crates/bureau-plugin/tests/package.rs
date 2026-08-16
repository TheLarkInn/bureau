use std::path::Path;

#[test]
fn bundled_package_and_install_plan_are_valid() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = repository.join("plugins/bureau");
    let package = bureau_plugin::inspect_package(&root).expect("package");
    let commands = bureau_plugin::install_commands(&repository).expect("commands");
    let actual = (
        package.name,
        commands[0].argv[1..4].to_vec(),
        commands[1].argv[1..].to_vec(),
    );
    let expected = (
        "bureau".to_owned(),
        ["plugin", "marketplace", "add"].map(str::to_owned).to_vec(),
        ["plugin", "install", "bureau@bureau"]
            .map(str::to_owned)
            .to_vec(),
    );
    assert_eq!(actual, expected);
}

#[test]
fn install_result_accepts_idempotence_and_rejects_failure() {
    let values = (
        bureau_plugin::validate_install_result(true, b"").is_ok(),
        bureau_plugin::validate_install_result(false, b"already installed").is_ok(),
        bureau_plugin::validate_install_result(false, b"permission denied").is_err(),
    );
    assert_eq!(values, (true, true, true));
}
