use bureau::forge::Forge as _;

use super::stub;

const LABELS_READ_PATH: &str = "/Odsp/_apis/wit/workitems/42?fields=System.Tags&api-version=7.1";

#[tokio::test]
async fn update_labels_preserves_other_tags_with_revision_test() {
    let current = r#"{"id":42,"rev":7,"fields":{"System.Tags":"bug; bureau:failed"}}"#;
    let stub = stub(&[("200 OK", current), ("200 OK", "{}")]);
    stub.forge()
        .update_labels(
            "Odsp/42",
            &["bureau:needs-human".to_owned()],
            &["bureau:failed".to_owned()],
        )
        .await
        .expect("update labels");
    let requests = stub.requests();
    let got = (
        requests[0].path.as_str(),
        requests[1].body.contains(r#""path":"/rev","value":7"#),
        requests[1]
            .body
            .contains(r#""value":"bug; bureau:needs-human""#),
    );
    assert_eq!(got, (LABELS_READ_PATH, true, true));
}
