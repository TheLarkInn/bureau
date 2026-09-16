use serde_json::json;

use super::{agent, factory};

const INVALID_NAMES: &[&str] = &[
    "",
    " ",
    " review",
    "review ",
    "review.name",
    "review/name",
    "review\\name",
    "review:name",
    "réview",
    "${factory}",
    "review\nname",
];

const INVALID_EXTENSIONS: &[&str] = &[
    "",
    "project:",
    "user:review",
    "global:review",
    "session:review",
    "plugin:review",
    "review",
    "project:.",
    "project:..",
    "project:../review",
    "project:review/child",
    "project:review\\child",
    "project:review..child",
    "project:.review",
    "project:review.",
    "project:review:child",
    "project: review",
    "project:réview",
    "project:${extension}",
    "project:review%2fchild",
    " project:review",
    "/project:review",
];

#[test]
fn supported_factory_names_are_valid() {
    for name in ["review", "Review_2", "review-change", "42", "-", "_"] {
        let mut selection = factory("");
        selection.name = name.to_owned();
        assert!(agent(Some(selection)).field_errors().is_empty(), "{name}");
    }
}

#[test]
fn malformed_factory_names_are_rejected() {
    for name in INVALID_NAMES {
        let mut selection = factory("");
        selection.name = (*name).to_owned();
        let errors = agent(Some(selection)).field_errors();
        assert!(
            errors[0].contains("copilot_factory.name"),
            "{name}: {errors:?}"
        );
    }
}

#[test]
fn a_source_id_is_one_safe_repository_local_directory() {
    for extension in [
        "project:review",
        "project:Review_2",
        "project:review-change.v1",
    ] {
        let mut selection = factory("");
        selection.extension = extension.to_owned();
        assert!(
            agent(Some(selection)).field_errors().is_empty(),
            "{extension}"
        );
    }
}

#[test]
fn unsafe_or_nonlocal_source_ids_are_rejected() {
    for extension in INVALID_EXTENSIONS {
        let mut selection = factory("");
        selection.extension = (*extension).to_owned();
        let errors = agent(Some(selection)).field_errors();
        assert!(
            errors[0].contains("copilot_factory.extension"),
            "{extension}: {errors:?}"
        );
    }
}

#[test]
fn arguments_cannot_be_primitives_arrays_or_encoded_objects() {
    let cases = [
        json!(false),
        json!(1),
        json!(-1.5),
        json!([]),
        json!([{}]),
        json!("{}"),
        json!("${inputs_from.prepare}"),
    ];
    for args in cases {
        let mut selection = factory("");
        selection.args = args;
        let errors = agent(Some(selection)).field_errors();
        assert!(errors[0].contains("copilot_factory.args"), "{errors:?}");
    }
}

#[test]
fn objects_and_null_need_no_argument_defaults() {
    for fields in [
        "",
        "args: null",
        "args: {}",
        "args: {value: [true, 2, null]}",
    ] {
        let errors = agent(Some(factory(fields))).field_errors();
        assert!(errors.is_empty(), "{fields}: {errors:?}");
    }
}

#[test]
fn positive_independent_limits_are_valid() {
    let cases = [
        "limits: {}",
        "limits: {max_concurrent_subagents: 1}",
        "limits: {max_concurrent_subagents: 500}",
        "limits: {max_total_subagents: 1}",
        "limits: {max_total_subagents: 4294967295}",
        "limits: {timeout_seconds: 0.125}",
        "limits: {timeout_seconds: 3000000}",
        "limits: {max_ai_credits: 0.125}",
        "limits: {max_concurrent_subagents: 2, max_total_subagents: 1}",
    ];
    for fields in cases {
        let errors = agent(Some(factory(fields))).field_errors();
        assert!(errors.is_empty(), "{fields}: {errors:?}");
    }
}

#[test]
fn concurrency_rejects_values_above_the_verified_source_c_ceiling() {
    for count in [501, u32::MAX] {
        let selection = factory(&format!("limits: {{max_concurrent_subagents: {count}}}"));
        let errors = agent(Some(selection)).field_errors();
        assert_eq!(
            errors,
            ["`copilot_factory.limits.max_concurrent_subagents` must not exceed 500"]
        );
    }
}

#[test]
fn the_concurrency_ceiling_does_not_hide_other_limit_errors() {
    let selection = factory(
        "limits: {max_concurrent_subagents: 501, max_total_subagents: 0, timeout_seconds: 0}",
    );
    let errors = agent(Some(selection)).field_errors();
    let fields = [
        "max_concurrent_subagents",
        "max_total_subagents",
        "timeout_seconds",
    ];
    let reported = fields.map(|field| errors.iter().any(|error| error.contains(field)));
    assert_eq!((errors.len(), reported), (3, [true; 3]), "{errors:?}");
}

#[test]
fn zero_integer_limits_are_reported_independently() {
    let selection = factory("limits: {max_concurrent_subagents: 0, max_total_subagents: 0}");
    let errors = agent(Some(selection)).field_errors();
    assert_eq!(
        errors,
        [
            "`copilot_factory.limits.max_concurrent_subagents` must be positive",
            "`copilot_factory.limits.max_total_subagents` must be positive",
        ]
    );
}

#[test]
fn numeric_limits_must_be_positive_and_finite() {
    for number in ["0", "-0.0", "-1.5", ".nan", ".inf", "-.inf"] {
        let selection = factory(&format!(
            "limits: {{timeout_seconds: {number}, max_ai_credits: {number}}}"
        ));
        let errors = agent(Some(selection)).field_errors();
        assert_eq!(
            errors,
            [
                "`copilot_factory.limits.timeout_seconds` must be positive and finite",
                "`copilot_factory.limits.max_ai_credits` must be positive and finite",
            ],
            "{number}"
        );
    }
}
