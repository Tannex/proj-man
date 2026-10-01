use crate::schema::{validate_properties, with_defaults};
use crate::{Error, Properties, Result, Schema};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub type_key: String,
    pub schema_revision: u64,
    pub revision: u64,
    pub properties: Properties,
    pub body: String,
    pub archived: bool,
    pub missing: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub type_key: String,
    pub schema_revision: u64,
    pub revision: u64,
    pub properties: Properties,
    pub position: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub operation_id: String,
    pub digest: String,
    #[serde(default)]
    pub request: Option<Mutation>,
    pub graph_revision: u64,
    pub schema_revision: u64,
    pub result: Value,
    pub before: Value,
    pub timestamp: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    pub workspace: String,
    pub revision: u64,
    pub schema_revision: u64,
    pub schemas: BTreeMap<u64, Schema>,
    pub nodes: BTreeMap<String, Node>,
    pub edges: BTreeMap<String, Edge>,
    pub receipts: BTreeMap<String, Receipt>,
    #[serde(default)]
    pub proposals: BTreeMap<String, crate::proposal::Proposal>,
}
impl State {
    pub fn new(workspace: &str) -> Self {
        Self {
            workspace: workspace.into(),
            ..Self::default()
        }
    }
    pub fn schema(&self) -> Result<&Schema> {
        self.schema_at(self.schema_revision)
    }
    pub fn schema_at(&self, revision: u64) -> Result<&Schema> {
        self.schemas
            .get(&revision)
            .ok_or_else(|| Error::missing("schema revision", &revision.to_string()))
    }
    pub fn node(&self, id: &str) -> Result<&Node> {
        self.nodes.get(id).ok_or_else(|| Error::missing("node", id))
    }
    pub fn edge(&self, id: &str) -> Result<&Edge> {
        self.edges.get(id).ok_or_else(|| Error::missing("edge", id))
    }
    pub fn title(&self, node: &Node) -> String {
        let title = self
            .schema_at(node.schema_revision)
            .ok()
            .and_then(|s| s.node_type(&node.type_key).ok())
            .and_then(|t| node.properties.get(&t.display_property))
            .and_then(Value::as_str);
        title
            .filter(|s| !s.trim().is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("{} {}", node.type_key, &node.id[..8.min(node.id.len())]))
    }
    pub fn edge_family(&self, edge: &Edge) -> Result<String> {
        Ok(self
            .schema_at(edge.schema_revision)?
            .relation_type(&edge.type_key)?
            .family()
            .into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mutation {
    pub operation_id: String,
    pub expected_schema: u64,
    #[serde(default)]
    pub expected_graph: Option<u64>,
    #[serde(default)]
    pub expected_nodes: BTreeMap<String, u64>,
    pub action: Action,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    SubmitProposal {
        proposal: crate::proposal::Draft,
    },
    RejectProposal {
        id: String,
        digest: String,
    },
    ApplyProposal {
        id: String,
        digest: String,
        groups: Vec<String>,
    },
    PublishSchema {
        schema: Schema,
        #[serde(default)]
        retain_legacy: bool,
    },
    Changes {
        operations: Vec<Operation>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    CreateNode {
        #[serde(default)]
        id: Option<String>,
        type_key: String,
        #[serde(default)]
        properties: Properties,
        #[serde(default)]
        body: String,
    },
    UpdateNode {
        id: String,
        #[serde(default)]
        set: Properties,
        #[serde(default)]
        unset: Vec<String>,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        archived: Option<bool>,
    },
    MigrateNode {
        id: String,
        #[serde(default)]
        type_key: Option<String>,
        properties: Properties,
    },
    AddLink {
        #[serde(default)]
        id: Option<String>,
        source: String,
        target: String,
        type_key: String,
        #[serde(default)]
        properties: Properties,
        #[serde(default)]
        position: Option<u32>,
    },
    RemoveLink {
        id: String,
    },
    Reorder {
        parent: String,
        family: String,
        edge_ids: Vec<String>,
    },
}

pub fn digest(value: &impl Serialize) -> Result<String> {
    // Canonicalize object order so equivalent JSON requests hash identically.
    let canonical = serde_json::to_value(value)?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&canonical)?)
    ))
}
fn valid_id(value: &str) -> Result<()> {
    Uuid::parse_str(value).map_err(|_| Error::validation(format!("'{value}' is not a UUID")))?;
    Ok(())
}
fn check_revision(state: &State, m: &Mutation, id: &str) -> Result<()> {
    if let Some(node) = state.nodes.get(id) {
        match m.expected_nodes.get(id) {
            Some(rev) if *rev == node.revision => Ok(()),
            expected => Err(
                Error::conflict(format!("Node {id} revision does not match")).with_details(
                    json!({"id":id,"expected":expected,"actual":node.revision,"node":node}),
                ),
            ),
        }
    } else {
        Ok(())
    } // New nodes in the same atomic batch have no existing revision.
}
fn ensure_current(state: &State, id: &str) -> Result<()> {
    let node = state.node(id)?;
    if node.schema_revision != state.schema_revision {
        return Err(Error::conflict(format!(
            "Node {id} requires schema migration"
        )));
    }
    Ok(())
}

/// Pure transition; callers persist it atomically under their storage lock.
/// Validation failures leave the input state untouched.
pub fn apply(state: &State, mutation: &Mutation) -> Result<(State, Receipt)> {
    valid_id(&mutation.operation_id)?;
    let hash = digest(mutation)?;
    if let Some(receipt) = state.receipts.get(&mutation.operation_id) {
        if receipt.digest != hash {
            return Err(Error::conflict(
                "Operation ID was already used for a different request",
            ));
        }
        return Ok((state.clone(), receipt.clone()));
    }
    if mutation.expected_schema != state.schema_revision {
        return Err(Error::conflict("The active schema changed").with_details(
            json!({"expected":mutation.expected_schema,"actual":state.schema_revision}),
        ));
    }
    if mutation
        .expected_graph
        .is_some_and(|rev| rev != state.revision)
    {
        return Err(Error::conflict("The graph changed")
            .with_details(json!({"expected":mutation.expected_graph,"actual":state.revision})));
    }
    if matches!(
        mutation.action,
        Action::SubmitProposal { .. }
            | Action::RejectProposal { .. }
            | Action::ApplyProposal { .. }
    ) {
        return crate::proposal::transition(state, mutation);
    }
    let mut next = state.clone();
    let now = Utc::now().to_rfc3339();
    let mut touched = BTreeSet::new();
    let mut created = Vec::new();
    let mut added_edges = Vec::new();
    let mut removed_edges = Vec::new();
    match &mutation.action {
        Action::SubmitProposal { .. }
        | Action::RejectProposal { .. }
        | Action::ApplyProposal { .. } => unreachable!("Handled above"),
        Action::PublishSchema {
            schema,
            retain_legacy,
        } => {
            if mutation.expected_graph.is_none() {
                return Err(Error::validation(
                    "Schema publication requires expected_graph from preview",
                ));
            }
            let mut schema = schema.clone();
            // Retiring a definition must not release its stable key/identity for
            // accidental reassignment while historical records still refer to it.
            let mut node_history = BTreeMap::new();
            let mut relation_history = BTreeMap::new();
            for revision in state.schemas.values() {
                for definition in &revision.node_types {
                    node_history.insert(definition.key.clone(), definition.clone());
                }
                for definition in &revision.relationship_types {
                    relation_history.insert(definition.key.clone(), definition.clone());
                }
            }
            let previous = Schema {
                node_types: node_history.into_values().collect(),
                relationship_types: relation_history.into_values().collect(),
            };
            schema.assign_ids(&previous)?;
            let impact = schema_impact(state, &schema)?;
            if !impact.is_empty() && !retain_legacy {
                return Err(Error::validation("Schema changes affect existing data; migrate it first or explicitly retain legacy data").with_details(&impact));
            }
            let revision = state.schema_revision + 1;
            next.schemas.insert(revision, schema.clone());
            next.schema_revision = revision;
            for node in next.nodes.values_mut() {
                if !impact.iter().any(|i| i["node_id"] == node.id) {
                    let ty = schema.node_type(&node.type_key)?;
                    node.missing = validate_properties(&ty.properties, &node.properties)?;
                    node.schema_revision = revision;
                    touched.insert(node.id.clone());
                }
            }
            // Edge rules are versioned too. Keep all edges on their old rules when
            // a publication affects any relationship; new edges use the new schema.
            if !impact.iter().any(|i| i.get("edge_id").is_some()) {
                for edge in next.edges.values_mut() {
                    edge.schema_revision = revision;
                    edge.revision += 1;
                }
            }
        }
        Action::Changes { operations } => {
            if operations.is_empty() {
                return Err(Error::validation(
                    "An atomic change needs at least one operation",
                ));
            }
            next.schema()?;
            for op in operations {
                match op {
                    Operation::CreateNode {
                        id,
                        type_key,
                        properties,
                        body,
                    } => {
                        let id = id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
                        valid_id(&id)?;
                        if next.nodes.contains_key(&id) {
                            return Err(Error::conflict(format!("Node {id} already exists")));
                        }
                        let ty = next.schema()?.node_type(type_key)?;
                        let properties = with_defaults(&ty.properties, properties.clone());
                        let missing = validate_properties(&ty.properties, &properties)?;
                        next.nodes.insert(
                            id.clone(),
                            Node {
                                id: id.clone(),
                                type_key: type_key.clone(),
                                schema_revision: next.schema_revision,
                                revision: 1,
                                properties,
                                body: body.clone(),
                                missing,
                                archived: false,
                                created_at: now.clone(),
                                updated_at: now.clone(),
                            },
                        );
                        touched.insert(id.clone());
                        created.push(id);
                    }
                    Operation::UpdateNode {
                        id,
                        set,
                        unset,
                        body,
                        archived,
                    } => {
                        check_revision(state, mutation, id)?;
                        ensure_current(&next, id)?;
                        if unset.iter().any(|key| set.contains_key(key)) {
                            return Err(Error::validation(
                                "A property cannot be set and unset together",
                            ));
                        }
                        let mut node = next.node(id)?.clone();
                        for (key, value) in set {
                            node.properties.insert(key.clone(), value.clone());
                        }
                        for key in unset {
                            if !next
                                .schema()?
                                .node_type(&node.type_key)?
                                .properties
                                .iter()
                                .any(|p| &p.key == key)
                            {
                                return Err(Error::validation(format!("Unknown property '{key}'")));
                            }
                            node.properties.remove(key);
                        }
                        if let Some(body) = body {
                            node.body = body.clone();
                        }
                        if let Some(archived) = archived {
                            node.archived = *archived;
                        }
                        node.missing = validate_properties(
                            &next.schema()?.node_type(&node.type_key)?.properties,
                            &node.properties,
                        )?;
                        next.nodes.insert(id.clone(), node);
                        touched.insert(id.clone());
                    }
                    Operation::MigrateNode {
                        id,
                        type_key,
                        properties,
                    } => {
                        check_revision(state, mutation, id)?;
                        let mut node = next.node(id)?.clone();
                        if let Some(type_key) = type_key {
                            node.type_key = type_key.clone();
                        }
                        let ty = next.schema()?.node_type(&node.type_key)?;
                        node.properties = with_defaults(&ty.properties, properties.clone());
                        node.missing = validate_properties(&ty.properties, &node.properties)?;
                        node.schema_revision = next.schema_revision;
                        next.nodes.insert(id.clone(), node);
                        touched.insert(id.clone());
                        // Incident edges migrate together; graph validation below rejects
                        // incompatible endpoints instead of silently discarding links.
                        for edge in next
                            .edges
                            .values_mut()
                            .filter(|e| &e.source == id || &e.target == id)
                        {
                            edge.schema_revision = next.schema_revision;
                            edge.revision += 1;
                            for endpoint in [&edge.source, &edge.target] {
                                check_revision(state, mutation, endpoint)?;
                                touched.insert(endpoint.clone());
                            }
                        }
                    }
                    Operation::AddLink {
                        id,
                        source,
                        target,
                        type_key,
                        properties,
                        position,
                    } => {
                        for id in [source, target] {
                            check_revision(state, mutation, id)?;
                            ensure_current(&next, id)?;
                            if next.node(id)?.archived {
                                return Err(Error::validation(
                                    "Cannot add links to archived nodes",
                                ));
                            }
                            touched.insert(id.clone());
                        }
                        let relation = next.schema()?.relation_type(type_key)?;
                        let family = relation.family().to_owned();
                        let properties = with_defaults(&relation.properties, properties.clone());
                        let missing = validate_properties(&relation.properties, &properties)?;
                        if !missing.is_empty() {
                            return Err(Error::validation(
                                "Relationship properties must be complete",
                            )
                            .with_details(missing));
                        }
                        let id = id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
                        valid_id(&id)?;
                        if next.edges.contains_key(&id) {
                            return Err(Error::conflict(format!("Edge {id} already exists")));
                        }
                        let siblings: Vec<_> = next
                            .edges
                            .values()
                            .filter(|e| {
                                &e.source == source
                                    && next.edge_family(e).ok().as_deref() == Some(&family)
                            })
                            .map(|e| e.id.clone())
                            .collect();
                        let index = position.unwrap_or(siblings.len() as u32);
                        if relation.ordered && index > siblings.len() as u32 {
                            return Err(Error::validation("Position exceeds sibling count"));
                        }
                        if relation.ordered {
                            for sibling_id in siblings {
                                let sibling = next.edges.get_mut(&sibling_id).unwrap();
                                if sibling.position >= index {
                                    check_revision(state, mutation, &sibling.target)?;
                                    touched.insert(sibling.target.clone());
                                    sibling.position += 1;
                                    sibling.revision += 1;
                                }
                            }
                        }
                        next.edges.insert(
                            id.clone(),
                            Edge {
                                id: id.clone(),
                                source: source.clone(),
                                target: target.clone(),
                                type_key: type_key.clone(),
                                schema_revision: next.schema_revision,
                                revision: 1,
                                properties,
                                position: index,
                            },
                        );
                        added_edges.push(id);
                    }
                    Operation::RemoveLink { id } => {
                        let edge = next.edge(id)?.clone();
                        for endpoint in [&edge.source, &edge.target] {
                            check_revision(state, mutation, endpoint)?;
                            touched.insert(endpoint.clone());
                        }
                        next.edges.remove(id);
                        removed_edges.push(id.clone());
                        let family = next.edge_family(&edge)?;
                        let siblings: Vec<_> = next
                            .edges
                            .values()
                            .filter(|e| {
                                e.source == edge.source
                                    && next.edge_family(e).ok().as_deref() == Some(&family)
                                    && e.position > edge.position
                            })
                            .map(|e| e.id.clone())
                            .collect();
                        if next
                            .schema_at(edge.schema_revision)?
                            .relation_type(&edge.type_key)?
                            .ordered
                        {
                            for id in siblings {
                                let sibling = next.edges.get_mut(&id).unwrap();
                                check_revision(state, mutation, &sibling.target)?;
                                touched.insert(sibling.target.clone());
                                sibling.position -= 1;
                                sibling.revision += 1;
                            }
                        }
                    }
                    Operation::Reorder {
                        parent,
                        family,
                        edge_ids,
                    } => {
                        check_revision(state, mutation, parent)?;
                        ensure_current(&next, parent)?;
                        let actual: BTreeSet<_> = next
                            .edges
                            .values()
                            .filter(|e| {
                                &e.source == parent
                                    && next.edge_family(e).ok().as_deref() == Some(family)
                            })
                            .map(|e| e.id.clone())
                            .collect();
                        if edge_ids.iter().cloned().collect::<BTreeSet<_>>() != actual
                            || edge_ids.len() != actual.len()
                        {
                            return Err(Error::validation(
                                "Reorder must include every edge in the parent's family exactly once",
                            ));
                        }
                        if !next
                            .schema()?
                            .relationship_types
                            .iter()
                            .any(|r| r.family() == family && r.ordered)
                        {
                            return Err(Error::validation("Family is not ordered"));
                        }
                        touched.insert(parent.clone());
                        for (position, id) in edge_ids.iter().enumerate() {
                            let edge = next.edges.get_mut(id).unwrap();
                            check_revision(state, mutation, &edge.target)?;
                            touched.insert(edge.target.clone());
                            edge.position = position as u32;
                            edge.revision += 1;
                        }
                    }
                }
            }
        }
    }
    validate_graph(&next)?;
    for id in &touched {
        let node = next.nodes.get_mut(id).unwrap();
        if let Some(old) = state.nodes.get(id) {
            node.revision = old.revision + 1;
        }
        node.updated_at = now.clone();
    }
    next.revision += 1;
    let updated: BTreeMap<_, _> = touched
        .iter()
        .map(|id| (id.clone(), next.nodes[id].revision))
        .collect();
    let before_nodes: Vec<_> = touched
        .iter()
        .filter_map(|id| state.nodes.get(id))
        .collect();
    let before_edges: Vec<_> = state
        .edges
        .values()
        .filter(|e| next.edges.get(&e.id) != Some(e))
        .collect();
    let receipt = Receipt {
        operation_id: mutation.operation_id.clone(),
        digest: hash,
        request: Some(mutation.clone()),
        graph_revision: next.revision,
        schema_revision: next.schema_revision,
        result: json!({"created_nodes":created,"added_edges":added_edges,"removed_edges":removed_edges,"node_revisions":updated}),
        before: json!({"nodes":before_nodes,"edges":before_edges,"schema_revision":state.schema_revision}),
        timestamp: now,
    };
    next.receipts
        .insert(receipt.operation_id.clone(), receipt.clone());
    Ok((next, receipt))
}

pub fn schema_impact(state: &State, schema: &Schema) -> Result<Vec<Value>> {
    schema.validate()?;
    let mut impact = Vec::new();
    for node in state.nodes.values() {
        match schema
            .node_type(&node.type_key)
            .and_then(|t| validate_properties(&t.properties, &node.properties))
        {
            Ok(missing) => {
                let added: Vec<_> = missing
                    .into_iter()
                    .filter(|m| !node.missing.contains(m))
                    .collect();
                if !added.is_empty() {
                    impact.push(json!({"node_id":node.id,"missing":added}));
                }
            }
            Err(e) => impact.push(json!({"node_id":node.id,"error":e})),
        }
    }
    let mut candidate = state.clone();
    let revision = state.schema_revision + 1;
    candidate.schemas.insert(revision, schema.clone());
    for edge in candidate.edges.values_mut() {
        edge.schema_revision = revision;
    }
    for edge in candidate.edges.values() {
        if let Err(e) = validate_edge(&candidate, edge) {
            impact.push(json!({"edge_id":edge.id,"error":e}));
        }
    }
    Ok(impact)
}
fn validate_edge(state: &State, edge: &Edge) -> Result<()> {
    let relation = state
        .schema_at(edge.schema_revision)?
        .relation_type(&edge.type_key)?;
    let source = state.node(&edge.source)?;
    let target = state.node(&edge.target)?;
    if !relation.sources.contains(&source.type_key) || !relation.targets.contains(&target.type_key)
    {
        return Err(Error::validation(format!(
            "{} does not allow these endpoint types",
            edge.type_key
        )));
    }
    if !relation.allow_self && source.id == target.id {
        return Err(Error::validation("Self links are forbidden"));
    }
    if !validate_properties(&relation.properties, &edge.properties)?.is_empty() {
        return Err(Error::validation(
            "Required relationship properties are missing",
        ));
    }
    let family = relation.family();
    let related: Vec<_> = state
        .edges
        .values()
        .filter(|e| state.edge_family(e).ok().as_deref() == Some(family))
        .collect();
    if relation.ordered {
        let mut positions: Vec<_> = related
            .iter()
            .filter(|e| e.source == edge.source)
            .map(|e| e.position)
            .collect();
        positions.sort_unstable();
        if positions
            .iter()
            .enumerate()
            .any(|(index, position)| *position as usize != index)
        {
            return Err(Error::validation(format!(
                "Ordered family {family} needs contiguous, unique sibling positions; explicitly reorder the family"
            )));
        }
    }
    if relation
        .max_incoming
        .is_some_and(|max| related.iter().filter(|e| e.target == edge.target).count() > max)
    {
        return Err(Error::validation(format!(
            "Incoming cardinality exceeded in family {family}"
        )));
    }
    if relation
        .max_outgoing
        .is_some_and(|max| related.iter().filter(|e| e.source == edge.source).count() > max)
    {
        return Err(Error::validation(format!(
            "Outgoing cardinality exceeded in family {family}"
        )));
    }
    if !relation.allow_duplicates
        && state.edges.values().any(|e| {
            e.id != edge.id
                && e.type_key == edge.type_key
                && e.source == edge.source
                && e.target == edge.target
        })
    {
        return Err(Error::validation("Duplicate relationship"));
    }
    if relation.acyclic {
        let mut adjacency: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for link in &related {
            adjacency
                .entry(&link.source)
                .or_default()
                .push(&link.target);
        }
        let mut seen = BTreeSet::new();
        let mut pending = vec![edge.target.as_str()];
        while let Some(id) = pending.pop() {
            if id == edge.source {
                return Err(Error::validation(format!(
                    "Cycle in relationship family {family}"
                )));
            }
            if seen.insert(id)
                && let Some(targets) = adjacency.get(id)
            {
                pending.extend(targets.iter().copied());
            }
        }
    }
    Ok(())
}
pub fn validate_graph(state: &State) -> Result<()> {
    for node in state.nodes.values() {
        let ty = state
            .schema_at(node.schema_revision)?
            .node_type(&node.type_key)?;
        validate_properties(&ty.properties, &node.properties)?;
        for id in mentions(&node.body) {
            state.node(&id).map_err(|_| {
                Error::validation(format!("Node {} mentions missing node {id}", node.id))
            })?;
        }
    }
    for edge in state.edges.values() {
        validate_edge(state, edge)?;
    }
    Ok(())
}
pub fn mentions(body: &str) -> BTreeSet<String> {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let regex = PATTERN.get_or_init(|| {
        regex::Regex::new(r"\]\(project://([a-zA-Z0-9-]+)\)").expect("constant regex")
    });
    regex.captures_iter(body).map(|c| c[1].to_owned()).collect()
}
pub fn suggestions(state: &State, source: &str, target: &str) -> Result<Vec<Value>> {
    ensure_current(state, source)?;
    ensure_current(state, target)?;
    let mut results = Vec::new();
    for relation in &state.schema()?.relationship_types {
        for (from, to, direction, label) in [
            (source, target, "outgoing", &relation.name),
            (target, source, "incoming", &relation.inverse_name),
        ] {
            if !relation.sources.contains(&state.node(from)?.type_key)
                || !relation.targets.contains(&state.node(to)?.type_key)
            {
                continue;
            }
            if state.node(from)?.archived || state.node(to)?.archived {
                continue;
            }
            let mut candidate = state.clone();
            let edge = Edge {
                id: Uuid::new_v4().to_string(),
                source: from.into(),
                target: to.into(),
                type_key: relation.key.clone(),
                schema_revision: state.schema_revision,
                revision: 1,
                properties: with_defaults(&relation.properties, Properties::new()),
                position: 0,
            };
            // Required edge properties are gathered after choosing the relation.
            let mut rules = relation.clone();
            rules.properties.clear();
            if let Some(rule) = candidate
                .schemas
                .get_mut(&state.schema_revision)
                .unwrap()
                .relationship_types
                .iter_mut()
                .find(|r| r.key == relation.key)
            {
                *rule = rules;
            }
            let mut structural = edge.clone();
            structural.properties.clear();
            candidate.edges.insert(edge.id.clone(), structural.clone());
            if validate_edge(&candidate, &structural).is_ok() {
                let recent_use = state
                    .edges
                    .values()
                    .filter(|e| e.type_key == relation.key)
                    .count();
                results.push(json!({"type_key":relation.key,"direction":direction,"label":label,"source":from,"target":to,"recent_use":recent_use,"properties":relation.properties}));
            }
        }
    }
    results.sort_by_key(|v| {
        (
            std::cmp::Reverse(v["recent_use"].as_u64().unwrap_or(0)),
            v["type_key"].as_str().unwrap_or("").to_string(),
            v["direction"].as_str().unwrap_or("").to_string(),
        )
    });
    Ok(results)
}
pub fn backlinks(state: &State, id: &str) -> Result<Value> {
    state.node(id)?;
    let edges: Vec<_> = state
        .edges
        .values()
        .filter(|e| e.target == id)
        .map(|e| {
            let label = state
                .schema_at(e.schema_revision)
                .ok()
                .and_then(|s| s.relation_type(&e.type_key).ok())
                .map(|r| r.inverse_name.clone());
            json!({"edge":e,"label":label,"node":state.nodes.get(&e.source)})
        })
        .collect();
    let mentions: Vec<_> = state
        .nodes
        .values()
        .filter(|n| mentions(&n.body).contains(id))
        .collect();
    Ok(json!({"edges":edges,"mentions":mentions}))
}
pub fn outline(state: &State, root: &str, family: &str) -> Result<Value> {
    state.node(root)?;
    if !state
        .schema()?
        .relationship_types
        .iter()
        .any(|r| r.family() == family && r.ordered)
    {
        return Err(Error::validation("Choose an ordered outline family"));
    }
    fn walk(state: &State, id: &str, family: &str, seen: &mut BTreeSet<String>) -> Result<Value> {
        if !seen.insert(id.into()) {
            return Err(Error::validation(
                "Outline contains a cycle or multiple parents",
            ));
        }
        let node = state.node(id)?;
        let mut edges: Vec<_> = state
            .edges
            .values()
            .filter(|e| e.source == id && state.edge_family(e).ok().as_deref() == Some(family))
            .collect();
        edges.sort_by_key(|e| (e.position, e.id.clone()));
        let children: Result<Vec<_>> = edges
            .into_iter()
            .map(|e| walk(state, &e.target, family, seen))
            .collect();
        Ok(
            json!({"id":id,"title":state.title(node),"type_key":node.type_key,"archived":node.archived,"children":children?}),
        )
    }
    walk(state, root, family, &mut BTreeSet::new())
}
