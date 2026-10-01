use serde_json::Value;
use std::fmt::Write;

/// Human output is deliberately separate from the versioned JSON contract.
pub fn render(data: &Value) -> String {
    if let Some(text) = data.get("text").and_then(Value::as_str) {
        return text.into();
    }
    if data.get("root").is_some()
        && data["nodes"].is_array()
        && data.get("options").is_some()
        && let Ok(tree) = serde_json::from_value::<projman_core::explorer::Tree>(data.clone())
    {
        use std::collections::BTreeMap;
        let nodes: BTreeMap<_, _> = tree
            .nodes
            .iter()
            .map(|node| (node.node.id.as_str(), node))
            .collect();
        fn visit(
            id: &str,
            depth: usize,
            tree: &projman_core::explorer::Tree,
            nodes: &BTreeMap<&str, &projman_core::explorer::TreeNode>,
            output: &mut String,
        ) {
            let node = nodes[id];
            let link = node
                .via
                .as_ref()
                .map(|via| {
                    format!(
                        "{} {}: ",
                        if via.direction == projman_core::explorer::Direction::Incoming {
                            "<-"
                        } else {
                            "->"
                        },
                        via.label
                    )
                })
                .unwrap_or_default();
            let _ = writeln!(
                output,
                "{}{}{} [{}] {}{}",
                "  ".repeat(depth),
                link,
                node.node.title,
                node.node.type_key,
                id,
                if node.more.is_some() { " ..." } else { "" }
            );
            for child in tree
                .nodes
                .iter()
                .filter(|n| n.parent.as_deref() == Some(id))
            {
                visit(&child.node.id, depth + 1, tree, nodes, output);
            }
            for reference in tree.references.iter().filter(|r| r.from == id) {
                let target = nodes[reference.to.as_str()];
                let _ = writeln!(
                    output,
                    "{}↪ {}: {} ({})",
                    "  ".repeat(depth + 1),
                    reference.via.label,
                    target.node.title,
                    reference.to
                );
            }
        }
        let mut text = format!("BFS explorer · graph revision {}\n", tree.graph_revision);
        visit(&tree.root, 0, &tree, &nodes, &mut text);
        if tree.truncated.depth || tree.truncated.nodes || tree.truncated.references {
            let _ = writeln!(
                text,
                "Bounded result: depth={}, nodes={}, references={}",
                tree.truncated.depth, tree.truncated.nodes, tree.truncated.references
            );
        }
        return text;
    }
    if let Some(proposal) = data.get("proposal") {
        let mut text = format!(
            "Proposal {} [{}]\nDigest: {}\n",
            string(&proposal["draft"]["id"]),
            string(&proposal["status"]),
            string(&proposal["digest"])
        );
        if let Some(groups) = proposal["draft"]["groups"].as_array() {
            for group in groups {
                let _ = writeln!(
                    text,
                    "  {}: {}\n    {}",
                    string(&group["id"]),
                    string(&group["title"]),
                    string(&group["rationale"])
                );
            }
        }
        if let Some(diff) = data["diff"].as_str() {
            text.push('\n');
            text.push_str(diff);
        }
        if !data["review_error"].is_null() {
            let _ = writeln!(
                text,
                "Review unavailable: {}",
                string(&data["review_error"]["message"])
            );
        }
        return text;
    }
    if let Some(id) = data.get("operation_id").and_then(Value::as_str) {
        let mut text = format!(
            "Operation {id}\nGraph revision {}, schema revision {}\n",
            data["graph_revision"], data["schema_revision"]
        );
        for (key, label) in [
            ("created_nodes", "Created node"),
            ("added_edges", "Added link"),
            ("removed_edges", "Removed link"),
        ] {
            if let Some(ids) = data["result"][key].as_array() {
                for id in ids {
                    let _ = writeln!(text, "{label}: {}", string(id));
                }
            }
        }
        if let Some(nodes) = data["result"]["node_revisions"].as_object() {
            for (id, revision) in nodes {
                let _ = writeln!(text, "Node {id}: revision {revision}");
            }
        }
        if let Some(id) = data["result"]["proposal_id"].as_str() {
            let _ = writeln!(text, "Proposal {id}: {}", data["result"]);
        }
        return text;
    }
    if data["children"].is_array() {
        fn tree(data: &Value, depth: usize, output: &mut String) {
            let _ = writeln!(
                output,
                "{}{} [{}] {}",
                "  ".repeat(depth),
                string(&data["title"]),
                string(&data["type_key"]),
                string(&data["id"])
            );
            if let Some(children) = data["children"].as_array() {
                for child in children {
                    tree(child, depth + 1, output);
                }
            }
        }
        let mut result = String::new();
        tree(data, 0, &mut result);
        return result;
    }
    if let Some(items) = data["items"].as_array() {
        let mut text = String::new();
        for item in items {
            let label = item
                .get("title")
                .or_else(|| item.get("label"))
                .or_else(|| item.get("status"))
                .map(string)
                .unwrap_or_default();
            if item.get("source").is_some() {
                let _ = writeln!(
                    text,
                    "{} {}: {} -> {} {}",
                    string(&item["id"]),
                    string(&item["type_key"]),
                    string(&item["source"]),
                    string(&item["target"]),
                    label
                );
            } else {
                let _ = writeln!(
                    text,
                    "{}  {}  {}",
                    string(&item["id"]),
                    string(&item["type_key"]),
                    label
                );
            }
        }
        if items.is_empty() {
            text.push_str("(empty)\n");
        }
        if let Some(next) = data["next_offset"].as_u64() {
            let _ = writeln!(text, "More results: --offset {next}");
        }
        return text;
    }
    serde_json::to_string_pretty(data).expect("JSON value is serializable")
}
fn string(value: &Value) -> String {
    value.as_str().unwrap_or("").into()
}
