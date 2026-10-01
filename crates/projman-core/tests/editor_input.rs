use projman_core::{NodeType, document};
use serde_json::json;

fn definition() -> NodeType {
    serde_json::from_value(
        json!({"key":"task","name":"Task","display_property":"name","properties":[
            {"key":"name","type":"string","required":true},
            {"key":"status","type":"enum","choices":["todo","in progress","done"]},
            {"key":"due","type":"date"},
            {"key":"hours","type":"integer"},
            {"key":"ready","type":"boolean"},
            {"key":"tags","type":"list","items":"string"}
        ]}),
    )
    .unwrap()
}
#[test]
fn editor_accepts_plain_text_and_preserves_typed_values_and_markdown() {
    let ty = definition();
    let text = "--- projman\nname: Updated task: café æøå\nstatus: in progress\ndue: 2026-10-01\nhours: 3\nready: false\ntags: [\"api\", \"UI\"]\n---\n## Notes\n\n---\nFree text\n";
    let doc = document::parse_with_schema(&ty, text).unwrap();
    assert_eq!(doc.properties["name"], "Updated task: café æøå");
    assert_eq!(doc.properties["status"], "in progress");
    assert_eq!(doc.properties["due"], "2026-10-01");
    assert_eq!(doc.properties["hours"], 3);
    assert_eq!(doc.properties["ready"], false);
    assert_eq!(doc.properties["tags"], json!(["api", "UI"]));
    assert_eq!(doc.body, "## Notes\n\n---\nFree text\n");
    assert!(document::validate(&ty, &doc).unwrap().is_empty());
    assert_eq!(document::fields(&ty, text).unwrap()[1].key, "status");
    let rendered = document::format(&ty, &doc.properties, &doc.body).unwrap();
    assert!(rendered.contains("name: \"Updated task: café æøå\""));
    let roundtrip = document::parse(&rendered).unwrap();
    assert_eq!(roundtrip.properties, doc.properties);
    assert_eq!(roundtrip.body, doc.body);
}
#[test]
fn blanks_are_missing_and_quoted_literals_stay_unambiguous() {
    let ty = definition();
    let doc = document::parse_with_schema(&ty, "--- projman\nname: \nhours: \nready: null\n---\n")
        .unwrap();
    assert_eq!(doc.properties["name"], json!(null));
    assert_eq!(document::validate(&ty, &doc).unwrap(), vec!["name"]);
    for (input, expected) in [
        ("true", "true"),
        ("42", "42"),
        ("\"null\"", "null"),
        ("\"  padded  \"", "  padded  "),
        ("\"line\\nnext\"", "line\nnext"),
    ] {
        let text = format!("--- projman\nname: {input}\n---\n");
        assert_eq!(
            document::parse_with_schema(&ty, &text).unwrap().properties["name"],
            expected
        );
    }
}
#[test]
fn malformed_structured_values_are_still_rejected_at_the_correct_field() {
    let ty = definition();
    for (line, field, row) in [
        ("hours: three", "hours", 3),
        ("ready: yes", "ready", 3),
        ("tags: [api]", "tags", 3),
        ("name: \"unterminated", "name", 3),
        ("surprise: text", "surprise", 3),
    ] {
        let doc = format!("--- projman\nstatus: todo\n{line}\n---\n");
        let error = document::parse_with_schema(&ty, &doc).unwrap_err();
        assert_eq!(error.details["field"], field);
        assert_eq!(error.details["line"], row);
    }
    for line in [
        "status: unknown",
        "due: yesterday",
        "hours: 1.5",
        "ready: 1",
        "tags: [3]",
    ] {
        let doc =
            document::parse_with_schema(&ty, &format!("--- projman\nname: Task\n{line}\n---\n"))
                .unwrap();
        let error = document::validate(&ty, &doc).unwrap_err();
        assert_eq!(error.details["line"], 3);
        assert!(error.details["field"].is_string());
    }
    assert!(document::parse_with_schema(&ty, "--- projman\nname: A\nname: B\n---\n").is_err());
}
