use super::super::{ModelSourceError, for_pipeline};
use super::{Fixture, pipeline};

fn unavailable(error: &anyhow::Error) -> Option<&str> {
    error
        .downcast_ref::<ModelSourceError>()
        .and_then(ModelSourceError::unavailable_reference)
}

#[test]
fn only_declared_model_source_unavailability_is_deferrable() {
    let fixture = Fixture::new();
    let mut settings = fixture.missing_settings();
    settings.credentials.insert(
        "unsafe".into(),
        bureau::setup::CredentialSource::File {
            path: fixture.0.parent().expect("fixture directory").to_path_buf(),
        },
    );
    for (reference, expected) in [
        ("missing", Some("missing")),
        ("absent", None),
        ("unsafe", None),
    ] {
        let error =
            for_pipeline(&pipeline(reference), Some(&settings)).expect_err("strict source failure");
        assert_eq!(unavailable(&error), expected);
    }
}

#[test]
fn missing_settings_are_not_an_inferred_model_source() {
    let error = for_pipeline(&pipeline("model-ref"), None).expect_err("strict missing settings");
    assert_eq!(unavailable(&error), None);
}

#[test]
fn a_non_model_credential_error_cannot_be_reclassified_by_its_message() {
    let fixture = Fixture::new();
    let error = bureau::credential::resolve(&fixture.missing_settings(), "missing")
        .expect_err("repository source unavailable");
    assert_eq!(unavailable(&anyhow::Error::new(error)), None);
}
