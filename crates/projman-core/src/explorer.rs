//! Read-only breadth-first spanning trees and paginated root discovery.
use crate::{Error, Node, Result, State};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Outgoing,
    Incoming,
    Both,
}
fn depth_default() -> usize {
    3
}
fn nodes_default() -> usize {
    500
}
fn references_default() -> usize {
    1000
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExploreOptions {
    pub root: String,
    #[serde(default)]
    pub direction: Direction,
    #[serde(default)]
    pub relation: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default = "depth_default")]
    pub max_depth: usize,
    #[serde(default = "nodes_default")]
    pub max_nodes: usize,
    #[serde(default = "references_default")]
    pub max_references: usize,
    #[serde(default)]
    pub include_archived: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub type_key: String,
    pub type_name: String,
    pub archived: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub edge_id: String,
    pub type_key: String,
    pub family: String,
    pub label: String,
    pub direction: Direction,
    pub position: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    #[serde(flatten)]
    pub node: Summary,
    pub depth: usize,
    pub parent: Option<String>,
    pub via: Option<Link>,
    /// There are eligible neighbours outside this bounded result.
    pub more: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub from: String,
    pub to: String,
    pub via: Link,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Truncated {
    pub depth: bool,
    pub nodes: bool,
    pub references: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tree {
    pub root: String,
    pub graph_revision: u64,
    pub options: ExploreOptions,
    /// Discovery order is level order. Clients can nest these by `parent`.
    pub nodes: Vec<TreeNode>,
    pub references: Vec<Reference>,
    pub truncated: Truncated,
}
#[derive(Debug, Clone)]
struct Neighbour {
    id: String,
    via: Link,
}

fn display(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = text.chars().map(|c| if c.is_control() { ' ' } else { c });
    let mut result: String = chars.by_ref().take(180).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}
fn summary(state: &State, node: &Node) -> Result<Summary> {
    let ty = state
        .schema_at(node.schema_revision)?
        .node_type(&node.type_key)?;
    Ok(Summary {
        id: node.id.clone(),
        title: display(&state.title(node)),
        type_key: node.type_key.clone(),
        type_name: display(&ty.name),
        archived: node.archived,
    })
}

pub fn explore(state: &State, options: &ExploreOptions) -> Result<Tree> {
    if options.max_depth > 32
        || !(1..=5000).contains(&options.max_nodes)
        || options.max_references > 5000
    {
        return Err(Error::validation(
            "Explorer limits: max_depth 0–32, max_nodes 1–5000, max_references 0–5000",
        ));
    }
    let root = state.node(&options.root)?;
    if root.archived && !options.include_archived {
        return Err(Error::validation(
            "Root is archived; enable include_archived to explore it",
        ));
    }
    // Historical types remain navigable while nodes await schema migration.
    if options.relation.as_ref().is_some_and(|key| {
        !state
            .schemas
            .values()
            .any(|s| s.relationship_types.iter().any(|r| &r.key == key))
    }) {
        return Err(Error::validation("Unknown relationship filter"));
    }
    if options.family.as_ref().is_some_and(|family| {
        !state
            .schemas
            .values()
            .any(|s| s.relationship_types.iter().any(|r| r.family() == family))
    }) {
        return Err(Error::validation("Unknown relationship family filter"));
    }
    let mut adjacency: BTreeMap<String, Vec<Neighbour>> = BTreeMap::new();
    let mut summaries = BTreeMap::new();
    for node in state
        .nodes
        .values()
        .filter(|n| options.include_archived || !n.archived)
    {
        summaries.insert(node.id.clone(), summary(state, node)?);
    }
    for edge in state.edges.values() {
        if !summaries.contains_key(&edge.source) || !summaries.contains_key(&edge.target) {
            continue;
        }
        let relation = state
            .schema_at(edge.schema_revision)?
            .relation_type(&edge.type_key)?;
        if options
            .relation
            .as_ref()
            .is_some_and(|r| r != &edge.type_key)
            || options
                .family
                .as_ref()
                .is_some_and(|f| f != relation.family())
        {
            continue;
        }
        let link = |direction, label: &str| Link {
            edge_id: edge.id.clone(),
            type_key: edge.type_key.clone(),
            family: relation.family().into(),
            label: display(label),
            direction,
            position: edge.position,
        };
        if options.direction != Direction::Incoming {
            adjacency
                .entry(edge.source.clone())
                .or_default()
                .push(Neighbour {
                    id: edge.target.clone(),
                    via: link(Direction::Outgoing, &relation.name),
                });
        }
        if options.direction != Direction::Outgoing
            && (edge.source != edge.target || options.direction == Direction::Incoming)
        {
            adjacency
                .entry(edge.target.clone())
                .or_default()
                .push(Neighbour {
                    id: edge.source.clone(),
                    via: link(Direction::Incoming, &relation.inverse_name),
                });
        }
    }
    for neighbours in adjacency.values_mut() {
        neighbours.sort_by_cached_key(|n| {
            (
                n.via.type_key.clone(),
                n.via.position,
                summaries[&n.id].title.to_lowercase(),
                n.id.clone(),
                n.via.edge_id.clone(),
            )
        });
    }
    let mut nodes = vec![TreeNode {
        node: summaries[&root.id].clone(),
        depth: 0,
        parent: None,
        via: None,
        more: None,
    }];
    let mut seen = BTreeSet::from([root.id.clone()]);
    let mut queue = VecDeque::from([0]);
    let mut tree_edges = BTreeSet::new();
    while let Some(index) = queue.pop_front() {
        let parent = nodes[index].node.id.clone();
        let depth = nodes[index].depth;
        if depth >= options.max_depth {
            continue;
        }
        for neighbour in adjacency.get(&parent).into_iter().flatten() {
            if seen.contains(&neighbour.id) || nodes.len() >= options.max_nodes {
                continue;
            }
            // Mark on enqueue: shared nodes are assigned to their nearest parent.
            seen.insert(neighbour.id.clone());
            tree_edges.insert(neighbour.via.edge_id.clone());
            nodes.push(TreeNode {
                node: summaries[&neighbour.id].clone(),
                depth: depth + 1,
                parent: Some(parent.clone()),
                via: Some(neighbour.via.clone()),
                more: None,
            });
            queue.push_back(nodes.len() - 1);
        }
    }
    let mut truncated = Truncated::default();
    let mut references = Vec::new();
    let mut referenced_edges = tree_edges;
    for node in &mut nodes {
        for neighbour in adjacency.get(&node.node.id).into_iter().flatten() {
            if !seen.contains(&neighbour.id) {
                if node.depth >= options.max_depth {
                    node.more = Some("depth".into());
                    truncated.depth = true;
                } else {
                    node.more = Some("nodes".into());
                    truncated.nodes = true;
                }
            } else if referenced_edges.insert(neighbour.via.edge_id.clone()) {
                // A canonical edge appears once, including in bidirectional mode.
                if references.len() < options.max_references {
                    references.push(Reference {
                        from: node.node.id.clone(),
                        to: neighbour.id.clone(),
                        via: neighbour.via.clone(),
                    });
                } else {
                    truncated.references = true;
                }
            }
        }
    }
    Ok(Tree {
        root: root.id.clone(),
        graph_revision: state.revision,
        options: options.clone(),
        nodes,
        references,
        truncated,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PickOptions {
    pub query: String,
    pub offset: usize,
    pub limit: usize,
    pub include_archived: bool,
}
impl Default for PickOptions {
    fn default() -> Self {
        Self {
            query: String::new(),
            offset: 0,
            limit: 50,
            include_archived: false,
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct PickPage {
    pub items: Vec<Summary>,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub graph_revision: u64,
}
pub fn pick(state: &State, options: &PickOptions) -> Result<PickPage> {
    if !(1..=1000).contains(&options.limit) {
        return Err(Error::validation("Picker limit must be between 1 and 1000"));
    }
    let query = options.query.to_lowercase();
    let terms: Vec<_> = query.split_whitespace().collect();
    let mut items = Vec::new();
    for node in state
        .nodes
        .values()
        .filter(|n| options.include_archived || !n.archived)
    {
        let ty = state
            .schema_at(node.schema_revision)?
            .node_type(&node.type_key)?;
        let searchable = format!(
            "{} {} {} {}",
            state.title(node),
            node.type_key,
            ty.name,
            node.id
        )
        .to_lowercase();
        if terms.iter().all(|term| searchable.contains(term)) {
            items.push(summary(state, node)?);
        }
    }
    items.sort_by_cached_key(|n| (n.title.to_lowercase(), n.type_key.clone(), n.id.clone()));
    let total = items.len();
    let end = options.offset.saturating_add(options.limit).min(total);
    Ok(PickPage {
        items: items
            .into_iter()
            .skip(options.offset)
            .take(options.limit)
            .collect(),
        total,
        next_offset: (end < total).then_some(end),
        graph_revision: state.revision,
    })
}
