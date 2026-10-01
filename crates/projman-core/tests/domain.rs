use projman_core::{document, graph, *};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

fn schema() -> Schema {
    serde_json::from_value(json!({
        "node_types":[
            {"key":"investigation","name":"Investigation","display_property":"summary","properties":[
                {"key":"summary","type":"string","required":true},
                {"key":"priority","type":"enum","choices":["low","high"],"default":"low"},
                {"key":"estimate","type":"integer","min":0}
            ]},
            {"key":"deliverable","name":"Deliverable","display_property":"label","properties":[{"key":"label","type":"string","required":true}]}
        ],
        "relationship_types":[
            {"key":"contains","name":"Contains","inverse_name":"Part of","sources":["investigation"],"targets":["investigation","deliverable"],"family":"breakdown","acyclic":true,"max_incoming":1,"ordered":true},
            {"key":"requires","name":"Requires","inverse_name":"Required by","sources":["investigation","deliverable"],"targets":["investigation","deliverable"],"acyclic":true}
        ]
    })).unwrap()
}
fn mutate(state: &State, action: Action) -> Mutation {
    Mutation {
        operation_id: Uuid::new_v4().to_string(),
        expected_schema: state.schema_revision,
        expected_graph: Some(state.revision),
        expected_nodes: state
            .nodes
            .iter()
            .map(|(id, n)| (id.clone(), n.revision))
            .collect(),
        action,
    }
}
fn initial() -> State {
    let state = State::new("test");
    graph::apply(
        &state,
        &mutate(
            &state,
            Action::PublishSchema {
                schema: schema(),
                retain_legacy: false,
            },
        ),
    )
    .unwrap()
    .0
}
fn create(id: &str, ty: &str, properties: Value) -> Operation {
    Operation::CreateNode {
        id: Some(id.into()),
        type_key: ty.into(),
        properties: serde_json::from_value(properties).unwrap(),
        body: String::new(),
    }
}
fn change(state: &State, ops: Vec<Operation>) -> State {
    graph::apply(state, &mutate(state, Action::Changes { operations: ops }))
        .unwrap()
        .0
}
fn link(from: &str, to: &str, kind: &str) -> Operation {
    Operation::AddLink {
        id: None,
        source: from.into(),
        target: to.into(),
        type_key: kind.into(),
        properties: BTreeMap::new(),
        position: None,
    }
}
fn id() -> String {
    Uuid::new_v4().to_string()
}

#[test]
fn custom_types_defaults_drafts_and_atomic_validation() {
    let state = initial();
    let a = id();
    let b = id();
    let state = change(
        &state,
        vec![
            create(&a, "investigation", json!({})),
            create(&b, "deliverable", json!({"label":"Ship"})),
        ],
    );
    assert_eq!(state.nodes[&a].missing, vec!["summary"]);
    assert_eq!(state.nodes[&a].properties["priority"], "low");
    assert_eq!(state.title(&state.nodes[&b]), "Ship");
    let invalid = mutate(
        &state,
        Action::Changes {
            operations: vec![
                create(&id(), "investigation", json!({})),
                create(&id(), "deliverable", json!({"summary":"wrong field"})),
            ],
        },
    );
    assert!(graph::apply(&state, &invalid).is_err());
    assert_eq!(state.nodes.len(), 2);
}
#[test]
fn stale_revision_and_idempotent_request() {
    let a = id();
    let state = initial();
    let request = mutate(
        &state,
        Action::Changes {
            operations: vec![create(&a, "investigation", json!({"summary":"A"}))],
        },
    );
    let (state, receipt) = graph::apply(&state, &request).unwrap();
    let (again, replayed) = graph::apply(&state, &request).unwrap();
    assert_eq!(again.revision, state.revision);
    assert_eq!(receipt.digest, replayed.digest);
    let mut reused = request.clone();
    reused.expected_schema = 999;
    assert_eq!(graph::apply(&state, &reused).unwrap_err().code, "conflict");
    let mut edit = mutate(
        &state,
        Action::Changes {
            operations: vec![Operation::UpdateNode {
                id: a.clone(),
                set: BTreeMap::new(),
                unset: vec![],
                body: Some("hello".into()),
                archived: None,
            }],
        },
    );
    edit.expected_nodes.insert(a, 0);
    edit.expected_graph = None;
    assert_eq!(graph::apply(&state, &edit).unwrap_err().code, "conflict");
}
#[test]
fn typed_edges_cycles_duplicates_and_parent_rules() {
    let (a, b, c) = (id(), id(), id());
    let state = change(
        &initial(),
        vec![
            create(&a, "investigation", json!({})),
            create(&b, "investigation", json!({})),
            create(&c, "deliverable", json!({})),
        ],
    );
    let state = change(
        &state,
        vec![link(&a, &b, "contains"), link(&b, &c, "contains")],
    );
    for op in [
        link(&a, &b, "contains"),
        link(&a, &c, "contains"),
        link(&b, &a, "contains"),
        link(&c, &a, "contains"),
    ] {
        assert!(
            graph::apply(
                &state,
                &mutate(
                    &state,
                    Action::Changes {
                        operations: vec![op]
                    }
                )
            )
            .is_err()
        );
    }
    assert_eq!(
        graph::outline(&state, &a, "breakdown").unwrap()["children"][0]["children"][0]["id"],
        c
    );
    let state = change(&state, vec![link(&c, &a, "requires")]);
    assert!(
        graph::apply(
            &state,
            &mutate(
                &state,
                Action::Changes {
                    operations: vec![link(&a, &c, "requires")]
                }
            )
        )
        .is_err()
    );
    assert!(
        graph::suggestions(&state, &a, &c)
            .unwrap()
            .iter()
            .all(|s| !(s["type_key"] == "contains" && s["direction"] == "outgoing"))
    );
}
#[test]
fn schema_breaking_change_requires_review_and_explicit_migration() {
    let a = id();
    let state = change(
        &initial(),
        vec![create(&a, "investigation", json!({"summary":"Keep"}))],
    );
    let mut next = schema();
    next.node_types[0].properties.push(
        serde_json::from_value(json!({"key":"decision","type":"string","required":true})).unwrap(),
    );
    assert_eq!(graph::schema_impact(&state, &next).unwrap().len(), 1);
    assert!(
        graph::apply(
            &state,
            &mutate(
                &state,
                Action::PublishSchema {
                    schema: next.clone(),
                    retain_legacy: false
                }
            )
        )
        .is_err()
    );
    let state = graph::apply(
        &state,
        &mutate(
            &state,
            Action::PublishSchema {
                schema: next,
                retain_legacy: true,
            },
        ),
    )
    .unwrap()
    .0;
    assert_eq!(state.nodes[&a].schema_revision, 1);
    assert_eq!(state.schema_revision, 2);
    let state = change(
        &state,
        vec![Operation::MigrateNode {
            id: a.clone(),
            type_key: None,
            properties: state.nodes[&a].properties.clone(),
        }],
    );
    assert_eq!(state.nodes[&a].missing, vec!["decision"]);
    assert_eq!(state.nodes[&a].schema_revision, 2);
}
#[test]
fn document_roundtrip_unicode_multiline_and_schema_order() {
    let schema = schema();
    let ty = &schema.node_types[0];
    let props = serde_json::from_value(
        json!({"summary":"æøå \"quoted\"\ntext","priority":"high","estimate":3}),
    )
    .unwrap();
    let body = "## Hello\n\n```text\n---\n```\nTrailing newlines\n\n";
    let text = document::format(ty, &props, body).unwrap();
    let parsed = document::parse(&text).unwrap();
    assert_eq!(parsed.properties, props);
    assert_eq!(parsed.body, body);
    let fields = document::fields(ty, &text).unwrap();
    assert!(fields[0].required);
    assert_eq!(fields[0].key, "summary");
    assert!(document::parse("--- projman\nx: 1\nx: 2\n---\n").is_err());
    assert!(document::parse("--- projman\nx: unquoted\n---\n").is_err());
    assert!(document::parse("--- projman\nx: 1\n").is_err());
}
#[test]
fn body_mentions_and_rename_keep_identity() {
    let (a, b) = (id(), id());
    let state = change(
        &initial(),
        vec![
            create(&a, "investigation", json!({"summary":"A"})),
            create(&b, "investigation", json!({"summary":"B"})),
        ],
    );
    let state = change(
        &state,
        vec![Operation::UpdateNode {
            id: a.clone(),
            set: BTreeMap::new(),
            unset: vec![],
            body: Some(format!("See [B](project://{b})")),
            archived: None,
        }],
    );
    let state = change(
        &state,
        vec![Operation::UpdateNode {
            id: b.clone(),
            set: serde_json::from_value(json!({"summary":"Renamed"})).unwrap(),
            unset: vec![],
            body: None,
            archived: None,
        }],
    );
    assert_eq!(
        graph::backlinks(&state, &b).unwrap()["mentions"][0]["id"],
        a
    );
    assert!(
        graph::apply(
            &state,
            &mutate(
                &state,
                Action::Changes {
                    operations: vec![Operation::UpdateNode {
                        id: a,
                        set: BTreeMap::new(),
                        unset: vec![],
                        body: Some(format!("[gone](project://{})", id())),
                        archived: None
                    }]
                }
            )
        )
        .is_err()
    );
}

#[test]
fn type_identities_survive_retirement_and_return() {
    let original = initial();
    let node_id = original
        .schema()
        .unwrap()
        .node_type("deliverable")
        .unwrap()
        .id
        .clone();
    let relation_id = original
        .schema()
        .unwrap()
        .relation_type("contains")
        .unwrap()
        .id
        .clone();
    let mut retired = schema();
    retired.node_types.retain(|t| t.key == "investigation");
    retired.relationship_types.clear();
    let retired = graph::apply(
        &original,
        &mutate(
            &original,
            Action::PublishSchema {
                schema: retired,
                retain_legacy: false,
            },
        ),
    )
    .unwrap()
    .0;
    let restored = graph::apply(
        &retired,
        &mutate(
            &retired,
            Action::PublishSchema {
                schema: schema(),
                retain_legacy: false,
            },
        ),
    )
    .unwrap()
    .0;
    assert_eq!(
        restored
            .schema()
            .unwrap()
            .node_type("deliverable")
            .unwrap()
            .id,
        node_id
    );
    assert_eq!(
        restored
            .schema()
            .unwrap()
            .relation_type("contains")
            .unwrap()
            .id,
        relation_id
    );
    let mut reassigned = schema();
    reassigned.node_types[0].id = node_id;
    assert!(
        graph::apply(
            &retired,
            &mutate(
                &retired,
                Action::PublishSchema {
                    schema: reassigned,
                    retain_legacy: false
                }
            )
        )
        .is_err()
    );
}
