use projman_core::{Action, Error, Mutation, Result, Schema, State, document, graph};
use projman_neo4j::{Config, Store};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

pub fn string<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::validation(format!("Missing string '{key}'")))
}
fn page(items: Vec<Value>, params: &Value) -> Result<Value> {
    let offset = params.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(100) as usize;
    if !(1..=1000).contains(&limit) {
        return Err(Error::validation("limit must be between 1 and 1000"));
    }
    let total = items.len();
    let end = offset.saturating_add(limit).min(total);
    let next = if end < total { Some(end) } else { None };
    Ok(
        json!({"items":items.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":total,"next_offset":next}),
    )
}
pub async fn call(config: &Config, method: &str, p: Value) -> Result<Value> {
    if method.starts_with("recovery.") {
        return crate::recovery::call(config, method, p).await;
    }
    if method == "schema.validate" {
        let schema: Schema = serde_json::from_value(p.get("schema").cloned().unwrap_or(p))?;
        schema.validate()?;
        return Ok(json!({"valid":true}));
    }
    if method == "config.show" {
        return Ok(serde_json::to_value(config)?);
    }
    let store = Store::connect(config).await?;
    if method == "doctor" {
        return store.doctor().await;
    }
    if method == "workspace.init" {
        store.initialize().await?;
        return Ok(json!({"workspace":config.workspace,"initialized":true}));
    }
    if matches!(method, "change.apply" | "change.validate") {
        let mutation: Mutation = serde_json::from_value(p)?;
        return Ok(serde_json::to_value(
            store.mutate(&mutation, method == "change.validate").await?,
        )?);
    }
    let state = store.read().await?;
    query_state(&state, method, &p)
}
pub fn query_state(state: &State, method: &str, p: &Value) -> Result<Value> {
    match method {
        "explorer.tree" => Ok(serde_json::to_value(projman_core::explorer::explore(state,&serde_json::from_value(p.clone())?)?)?),
        "node.pick" => Ok(serde_json::to_value(projman_core::explorer::pick(state,&serde_json::from_value(p.clone())?)?)?),
        "proposal.list" => page(state.proposals.values().map(|p| json!({"id":p.draft.id,"digest":p.digest,"status":if p.status=="pending" && (p.draft.base_graph!=state.revision||p.draft.base_schema!=state.schema_revision) {"stale"} else {p.status.as_str()}})).collect(),p),
        "proposal.show" => {
            let id=string(p,"id")?;
            let proposal=state.proposals.get(id).ok_or_else(|| Error::missing("proposal",id))?;
            match projman_core::proposal::review(state,id,None) {
                Ok(review)=>Ok(review),
                Err(error)=>Ok(json!({"proposal":proposal,"review_error":error})),
            }
        }
        "proposal.validate" => projman_core::proposal::review(state,string(p,"id")?,p.get("groups").filter(|v|!v.is_null()).map(|v|serde_json::from_value(v.clone())).transpose()?),
        "workspace.show" => Ok(
            json!({"workspace":state.workspace,"graph_revision":state.revision,"schema_revision":state.schema_revision,"nodes":state.nodes.len(),"edges":state.edges.len()}),
        ),
        "export" => Ok(serde_json::to_value(state)?),
        "schema.export" => {
            let revision = p
                .get("revision")
                .and_then(Value::as_u64)
                .unwrap_or(state.schema_revision);
            Ok(
                json!({"schema_revision":revision,"graph_revision":state.revision,"schema":state.schema_at(revision)?}),
            )
        }
        "schema.preview" => {
            let schema: Schema = serde_json::from_value(
                p.get("schema")
                    .cloned()
                    .ok_or_else(|| Error::validation("schema is required"))?,
            )?;
            Ok(
                json!({"schema_revision":state.schema_revision,"graph_revision":state.revision,"impact":graph::schema_impact(state,&schema)?}),
            )
        }
        "type.list" => Ok(
            json!({"node_types":state.schema()?.node_types,"relationship_types":state.schema()?.relationship_types}),
        ),
        "type.show" => {
            let key = string(p, "key")?;
            match state.schema()?.node_type(key) {
                Ok(ty) => Ok(serde_json::to_value(ty)?),
                Err(_) => Ok(serde_json::to_value(state.schema()?.relation_type(key)?)?),
            }
        }
        "node.get" => Ok(serde_json::to_value(state.node(string(p, "id")?)?)?),
        "node.new" => {
            let ty = state.schema()?.node_type(string(p, "type_key")?)?;
            let properties = projman_core::schema::with_defaults(&ty.properties, BTreeMap::new());
            let text = document::format(ty, &properties, "")?;
            Ok(
                json!({"id":Uuid::new_v4().to_string(),"type_key":ty.key,"schema_revision":state.schema_revision,"revision":0,"properties":properties,"body":"","text":text,"fields":document::fields(ty,&text)?}),
            )
        }
        "node.document" => {
            let node = state.node(string(p, "id")?)?;
            let ty = state
                .schema_at(node.schema_revision)?
                .node_type(&node.type_key)?;
            let text = document::format(ty, &node.properties, &node.body)?;
            Ok(
                json!({"node":node,"text":text,"fields":document::fields(ty,&text)?,"type":ty,"graph_revision":state.revision,"active_schema_revision":state.schema_revision}),
            )
        }
        "document.parse" => {
            let text = string(p, "text")?;
            let ty = state
                .schema_at(
                    p.get("schema_revision")
                        .and_then(Value::as_u64)
                        .unwrap_or(state.schema_revision),
                )?
                .node_type(string(p, "type_key")?)?;
            let doc = document::parse_with_schema(ty, text)?;
            let missing = document::validate(ty, &doc)?;
            Ok(
                json!({"properties":doc.properties,"body":doc.body,"fields":document::fields(ty,text)?,"missing":missing,"active_schema_revision":state.schema_revision}),
            )
        }
        "node.list" | "search" => {
            let search = p
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase();
            let include_archived = p
                .get("include_archived")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let type_key = p.get("type_key").and_then(Value::as_str);
            let relation = p.get("relation").and_then(Value::as_str);
            let source = p.get("source").and_then(Value::as_str);
            let incoming = p.get("direction").and_then(Value::as_str) == Some("incoming");
            if relation.is_some()!=source.is_some() {return Err(Error::validation("relation and source must be supplied together"));}
            if let (Some(relation),Some(source))=(relation,source) {state.schema()?.relation_type(relation)?;state.node(source)?;}
            let mut items = Vec::new();
            for node in state.nodes.values() {
                if (!include_archived && node.archived)
                    || type_key.is_some_and(|t| t != node.type_key)
                {
                    continue;
                }
                if let (Some(relation), Some(source)) = (relation, source) {
                    if node.schema_revision!=state.schema_revision {continue;}
                    if !graph::suggestions(state, source, &node.id)?
                        .iter()
                        .any(|s| {
                            s["type_key"] == relation && (s["direction"] == "incoming") == incoming
                        })
                    {
                        continue;
                    }
                }
                let title = state.title(node);
                if !search.is_empty()
                    && !title.to_lowercase().contains(&search)
                    && !node.body.to_lowercase().contains(&search)
                {
                    continue;
                }
                items.push(json!({"id":node.id,"title":title,"type_key":node.type_key,"revision":node.revision,"schema_revision":node.schema_revision,"missing":node.missing,"archived":node.archived}));
            }
            page(items, p)
        }
        "link.list" => {
            let id = p.get("id").and_then(Value::as_str);
            if let Some(id) = id {
                state.node(id)?;
            }
            page(
                state
                    .edges
                    .values()
                    .filter(|e| id.is_none_or(|id| e.source == id || e.target == id))
                    .map(|e| json!(e))
                    .collect(),
                p,
            )
        }
        "link.suggest" => {
            Ok(json!({"items":graph::suggestions(state,string(p,"source")?,string(p,"target")?)?}))
        }
        "outline" => graph::outline(state, string(p, "id")?, string(p, "family")?),
        "backlinks" => graph::backlinks(state, string(p, "id")?),
        "operation.get" => Ok(serde_json::to_value(
            state
                .receipts
                .get(string(p, "id")?)
                .ok_or_else(|| Error::missing("operation", p["id"].as_str().unwrap_or("")))?,
        )?),
        _ => Err(Error::new("usage", format!("Unknown method '{method}'"))),
    }
}

pub fn change(
    state: &State,
    operations: Vec<projman_core::Operation>,
    operation_id: Option<String>,
    expected_graph: Option<u64>,
    expected_nodes: BTreeMap<String, u64>,
    expected_schema: Option<u64>,
) -> Mutation {
    Mutation {
        operation_id: operation_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        expected_schema: expected_schema.unwrap_or(state.schema_revision),
        expected_graph,
        expected_nodes,
        action: Action::Changes { operations },
    }
}
