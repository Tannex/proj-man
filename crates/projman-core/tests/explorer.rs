use projman_core::{Edge, Node, Schema, State, explorer::*};
use serde_json::json;
use std::collections::BTreeMap;

fn fixture() -> State {
    let schema: Schema=serde_json::from_value(json!({"node_types":[{"key":"item","name":"Work Item","display_property":"title","properties":[{"key":"title","type":"string"}]}],"relationship_types":[
        {"key":"relates","name":"Relates to","inverse_name":"Related from","sources":["item"],"targets":["item"],"family":"references","allow_duplicates":true,"allow_self":true},
        {"key":"requires","name":"Requires","inverse_name":"Required by","sources":["item"],"targets":["item"],"family":"dependencies"}
    ]})).unwrap();
    let mut state = State::new("explorer");
    state.schema_revision = 1;
    state.schemas.insert(1, schema);
    for id in ["root", "a", "b", "c", "d", "isolated", "archived"] {
        state.nodes.insert(
            id.into(),
            Node {
                id: id.into(),
                type_key: "item".into(),
                schema_revision: 1,
                revision: 1,
                properties: BTreeMap::from([("title".into(), json!(id.to_uppercase()))]),
                body: String::new(),
                archived: id == "archived",
                missing: vec![],
                created_at: String::new(),
                updated_at: String::new(),
            },
        );
    }
    // DFS would first find d at depth three. BFS must attach it under b at depth two.
    for (id, from, to, position, kind) in [
        ("ra", "root", "a", 0, "relates"),
        ("rb", "root", "b", 1, "relates"),
        ("ac", "a", "c", 0, "relates"),
        ("cd", "c", "d", 0, "relates"),
        ("bd", "b", "d", 0, "relates"),
        ("dr", "d", "root", 0, "relates"),
        ("self", "a", "a", 1, "relates"),
        ("parallel", "root", "a", 2, "relates"),
        ("hidden", "root", "archived", 3, "relates"),
        ("required", "root", "isolated", 0, "requires"),
    ] {
        state.edges.insert(
            id.into(),
            Edge {
                id: id.into(),
                source: from.into(),
                target: to.into(),
                type_key: kind.into(),
                schema_revision: 1,
                revision: 1,
                properties: BTreeMap::new(),
                position,
            },
        );
    }
    state
}
fn options(root: &str) -> ExploreOptions {
    serde_json::from_value(json!({"root":root})).unwrap()
}
#[test]
fn breadth_first_assigns_shortest_parents_and_retains_extra_edges() {
    let state = fixture();
    let before = serde_json::to_value(&state).unwrap();
    let tree = explore(&state, &options("root")).unwrap();
    assert_eq!(
        tree.nodes
            .iter()
            .map(|n| n.node.id.as_str())
            .collect::<Vec<_>>(),
        vec!["root", "a", "b", "isolated", "c", "d"]
    );
    let d = tree.nodes.iter().find(|n| n.node.id == "d").unwrap();
    assert_eq!(d.parent.as_deref(), Some("b"));
    assert_eq!(d.depth, 2);
    for edge in ["cd", "dr", "self", "parallel"] {
        assert!(tree.references.iter().any(|r| r.via.edge_id == edge));
    }
    assert_eq!(tree.references.len(), 4);
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        before,
        "Exploration never reparents stored nodes"
    );
    assert_eq!(
        serde_json::to_value(&tree).unwrap(),
        serde_json::to_value(explore(&state, &options("root")).unwrap()).unwrap()
    );
}
#[test]
fn direction_and_filters_use_canonical_edges_and_inverse_labels() {
    let state = fixture();
    let mut opts = options("isolated");
    opts.direction = Direction::Incoming;
    let tree = explore(&state, &opts).unwrap();
    assert_eq!(tree.nodes[1].node.id, "root");
    assert_eq!(tree.nodes[1].via.as_ref().unwrap().label, "Required by");
    opts = options("root");
    opts.direction = Direction::Both;
    let tree = explore(&state, &opts).unwrap();
    assert_eq!(
        tree.nodes.iter().find(|n| n.node.id == "d").unwrap().depth,
        1
    );
    let ids: Vec<_> = tree
        .nodes
        .iter()
        .filter_map(|n| n.via.as_ref().map(|v| v.edge_id.clone()))
        .chain(tree.references.iter().map(|r| r.via.edge_id.clone()))
        .collect();
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        ids.len(),
        "Both directions must not duplicate canonical edges"
    );
    opts.family = Some("dependencies".into());
    let tree = explore(&state, &opts).unwrap();
    assert_eq!(tree.nodes.len(), 2);
    opts.family = None;
    opts.relation = Some("requires".into());
    assert_eq!(explore(&state, &opts).unwrap().nodes.len(), 2);
    opts.relation = Some("missing".into());
    assert!(explore(&state, &opts).is_err());
}
#[test]
fn bounds_and_archives_are_explicit_and_resumable() {
    let state = fixture();
    let mut opts = options("root");
    opts.max_depth = 1;
    let tree = explore(&state, &opts).unwrap();
    assert!(tree.truncated.depth);
    assert!(tree.nodes.iter().all(|n| n.depth <= 1));
    assert_eq!(
        tree.nodes
            .iter()
            .find(|n| n.node.id == "a")
            .unwrap()
            .more
            .as_deref(),
        Some("depth")
    );
    opts.max_depth = 3;
    opts.max_nodes = 2;
    let tree = explore(&state, &opts).unwrap();
    assert_eq!(tree.nodes.len(), 2);
    assert!(tree.truncated.nodes);
    opts.max_nodes = 500;
    opts.max_references = 0;
    let tree = explore(&state, &opts).unwrap();
    assert!(tree.references.is_empty());
    assert!(tree.truncated.references);
    opts.include_archived = true;
    assert!(
        explore(&state, &opts)
            .unwrap()
            .nodes
            .iter()
            .any(|n| n.node.archived)
    );
    assert!(explore(&state, &options("archived")).is_err());
    assert!(explore(&state, &options("missing")).is_err());
    opts.max_nodes = 0;
    assert!(explore(&state, &opts).is_err());
    opts.max_nodes = 1;
    opts.max_depth = 33;
    assert!(explore(&state, &opts).is_err());
}
#[test]
fn picker_filters_titles_types_and_ids_and_pages_all_matches() {
    let mut state = fixture();
    state
        .nodes
        .get_mut("c")
        .unwrap()
        .properties
        .insert("title".into(), json!("Unicode Æble\nnext line"));
    let opts = PickOptions {
        query: "ÆBLE work c".into(),
        ..Default::default()
    };
    let page = pick(&state, &opts).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].id, "c");
    assert!(!page.items[0].title.contains('\n'));
    let mut opts = PickOptions {
        limit: 2,
        ..Default::default()
    };
    let mut all = Vec::new();
    loop {
        let page = pick(&state, &opts).unwrap();
        all.extend(page.items.into_iter().map(|n| n.id));
        if let Some(next) = page.next_offset {
            opts.offset = next;
        } else {
            break;
        }
    }
    assert_eq!(all.len(), 6);
    assert!(!all.contains(&"archived".into()));
    opts.offset = 100;
    assert!(pick(&state, &opts).unwrap().items.is_empty());
    opts.limit = 0;
    assert!(pick(&state, &opts).is_err());
    assert_eq!(
        pick(
            &state,
            &PickOptions {
                query: "does not match".into(),
                ..Default::default()
            }
        )
        .unwrap()
        .total,
        0
    );
}
