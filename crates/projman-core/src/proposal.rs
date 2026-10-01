//! Proposal approval is an application operation, independent of any AI provider.
use crate::{Action, Error, Mutation, Operation, Receipt, Result, State, graph};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub title: String,
    pub rationale: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub operations: Vec<Operation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub id: String,
    pub base_schema: u64,
    pub base_graph: u64,
    pub expected_nodes: BTreeMap<String, u64>,
    pub groups: Vec<Group>,
    #[serde(default)]
    pub questions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub draft: Draft,
    pub digest: String,
    pub status: String,
    pub accepted_groups: Vec<String>,
    pub context_nodes: BTreeMap<String, crate::Node>,
    pub schema: crate::Schema,
}
fn selected(proposal: &Proposal, groups: &[String]) -> Result<Vec<Operation>> {
    let selected: BTreeSet<_> = groups.iter().collect();
    if selected.is_empty() || selected.len() != groups.len() {
        return Err(Error::validation(
            "Select distinct, nonempty proposal groups",
        ));
    }
    for id in &selected {
        let group = proposal
            .draft
            .groups
            .iter()
            .find(|g| &g.id == *id)
            .ok_or_else(|| Error::validation(format!("Unknown group '{id}'")))?;
        for dependency in &group.depends_on {
            if !selected.contains(dependency) {
                return Err(Error::validation(format!(
                    "Group {id} requires reviewed group {dependency}"
                )));
            }
        }
    }
    // Preserve the reviewed group order; it must be a valid execution order.
    Ok(proposal
        .draft
        .groups
        .iter()
        .filter(|g| selected.contains(&g.id))
        .flat_map(|g| g.operations.clone())
        .collect())
}
pub fn validate(state: &State, proposal: &Proposal, groups: &[String]) -> Result<(State, Receipt)> {
    if proposal.status != "pending" {
        return Err(Error::conflict("Proposal is already applied or rejected"));
    }
    if proposal.draft.base_graph != state.revision
        || proposal.draft.base_schema != state.schema_revision
    {
        return Err(Error::conflict(
            "Proposal is stale; reconcile and review a new proposal",
        ));
    }
    for (id, revision) in &proposal.draft.expected_nodes {
        if state.node(id)?.revision != *revision {
            return Err(Error::conflict(format!(
                "Proposal read-set revision does not match node {id}"
            )));
        }
    }
    graph::apply(
        state,
        &Mutation {
            operation_id: Uuid::new_v4().to_string(),
            expected_schema: proposal.draft.base_schema,
            expected_graph: Some(proposal.draft.base_graph),
            expected_nodes: proposal.draft.expected_nodes.clone(),
            action: Action::Changes {
                operations: selected(proposal, groups)?,
            },
        },
    )
}
pub fn review(state: &State, id: &str, groups: Option<Vec<String>>) -> Result<Value> {
    let proposal = state
        .proposals
        .get(id)
        .ok_or_else(|| Error::missing("proposal", id))?;
    let selection =
        groups.unwrap_or_else(|| proposal.draft.groups.iter().map(|g| g.id.clone()).collect());
    let (preview, receipt) = validate(state, proposal, &selection)?;
    let nodes: Vec<_> = preview
        .nodes
        .values()
        .filter(|n| state.nodes.get(&n.id) != Some(n))
        .map(|n| json!({"id":n.id,"before":state.nodes.get(&n.id),"after":n}))
        .collect();
    let added: Vec<_> = preview
        .edges
        .values()
        .filter(|e| state.edges.get(&e.id) != Some(e))
        .collect();
    let removed: Vec<_> = state
        .edges
        .values()
        .filter(|e| preview.edges.get(&e.id) != Some(e))
        .collect();
    let mut diff = String::new();
    for change in &nodes {
        for field in ["properties", "body"] {
            if change["before"][field] == change["after"][field] {
                continue;
            }
            let before = if field == "body" {
                change["before"][field].as_str().unwrap_or("").to_owned()
            } else {
                serde_json::to_string_pretty(&change["before"][field])?
            };
            let after = if field == "body" {
                change["after"][field].as_str().unwrap_or("").to_owned()
            } else {
                serde_json::to_string_pretty(&change["after"][field])?
            };
            diff.push_str(&format!(
                "--- {}/{field} before\n+++ {}/{field} after\n@@ -1,{} +1,{} @@\n",
                change["id"].as_str().unwrap_or(""),
                change["id"].as_str().unwrap_or(""),
                before.lines().count(),
                after.lines().count()
            ));
            for line in before.lines() {
                diff.push_str(&format!("-{line}\n"));
            }
            for line in after.lines() {
                diff.push_str(&format!("+{line}\n"));
            }
        }
    }
    for edge in &removed {
        diff.push_str(&format!(
            "- link {} {} -> {} ({})\n",
            edge.type_key, edge.source, edge.target, edge.id
        ));
    }
    for edge in &added {
        diff.push_str(&format!(
            "+ link {} {} -> {} ({})\n",
            edge.type_key, edge.source, edge.target, edge.id
        ));
    }
    Ok(
        json!({"proposal":proposal,"selected_groups":selection,"nodes":nodes,"edges":{"added":added,"removed":removed},"validation":receipt.result,"diff":diff}),
    )
}
pub fn transition(state: &State, m: &Mutation) -> Result<(State, Receipt)> {
    let mut next = state.clone();
    let result;
    match &m.action {
        Action::SubmitProposal { proposal } => {
            Uuid::parse_str(&proposal.id)
                .map_err(|_| Error::validation("Proposal ID must be a UUID"))?;
            if state.proposals.contains_key(&proposal.id) {
                return Err(Error::conflict("Proposal ID already exists"));
            }
            if proposal.groups.is_empty() {
                return Err(Error::validation("A proposal needs at least one group"));
            }
            let mut known = BTreeSet::new();
            let mut draft = proposal.clone();
            for group in &mut draft.groups {
                if group.id.is_empty()
                    || group.operations.is_empty()
                    || !known.insert(group.id.clone())
                {
                    return Err(Error::validation(
                        "Proposal groups need unique IDs and operations",
                    ));
                }
                if group
                    .depends_on
                    .iter()
                    .any(|d| d == &group.id || !known.contains(d))
                {
                    return Err(Error::validation("Dependencies must name preceding groups"));
                }
                for operation in &mut group.operations {
                    if let Operation::CreateNode { id, .. } | Operation::AddLink { id, .. } =
                        operation
                        && id.is_none()
                    {
                        *id = Some(Uuid::new_v4().to_string());
                    }
                }
            }
            let proposal = Proposal {
                digest: graph::digest(&draft)?,
                draft,
                status: "pending".into(),
                accepted_groups: vec![],
                context_nodes: draft_context(state, &proposal.expected_nodes)?,
                schema: state.schema()?.clone(),
            };
            let all = proposal
                .draft
                .groups
                .iter()
                .map(|g| g.id.clone())
                .collect::<Vec<_>>();
            validate(state, &proposal, &all)?;
            result = json!({"proposal_id":proposal.draft.id,"digest":proposal.digest,"status":"pending"});
            next.proposals.insert(proposal.draft.id.clone(), proposal);
        }
        Action::RejectProposal { id, digest } => {
            let proposal = next
                .proposals
                .get_mut(id)
                .ok_or_else(|| Error::missing("proposal", id))?;
            if &proposal.digest != digest {
                return Err(Error::conflict("Reviewed proposal digest does not match"));
            }
            if proposal.status != "pending" {
                return Err(Error::conflict("Proposal is already applied or rejected"));
            }
            proposal.status = "rejected".into();
            result = json!({"proposal_id":id,"status":"rejected"});
        }
        Action::ApplyProposal { id, digest, groups } => {
            let proposal = state
                .proposals
                .get(id)
                .ok_or_else(|| Error::missing("proposal", id))?;
            if &proposal.digest != digest {
                return Err(Error::conflict("Reviewed proposal digest does not match"));
            }
            let (mut applied, mut receipt) = validate(state, proposal, groups)?;
            let validated_id = receipt.operation_id.clone();
            applied.receipts.remove(&validated_id);
            receipt.operation_id = m.operation_id.clone();
            receipt.digest = graph::digest(m)?;
            receipt.request = Some(m.clone());
            receipt.result["proposal_id"] = json!(id);
            receipt.result["accepted_groups"] = json!(groups);
            let proposal = applied.proposals.get_mut(id).unwrap();
            proposal.status = "applied".into();
            proposal.accepted_groups = groups.clone();
            applied
                .receipts
                .insert(m.operation_id.clone(), receipt.clone());
            return Ok((applied, receipt));
        }
        _ => return Err(Error::new("internal", "Expected proposal operation")),
    }
    let receipt = Receipt {
        operation_id: m.operation_id.clone(),
        digest: graph::digest(m)?,
        request: Some(m.clone()),
        graph_revision: state.revision,
        schema_revision: state.schema_revision,
        result,
        before: json!({}),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    next.receipts
        .insert(m.operation_id.clone(), receipt.clone());
    Ok((next, receipt))
}
fn draft_context(
    state: &State,
    expected: &BTreeMap<String, u64>,
) -> Result<BTreeMap<String, crate::Node>> {
    expected
        .keys()
        .map(|id| Ok((id.clone(), state.node(id)?.clone())))
        .collect()
}
