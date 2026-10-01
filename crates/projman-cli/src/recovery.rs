//! Private local recovery files, independent of database availability.
use projman_core::{Error, Mutation, Operation, Result, document};
use projman_neo4j::{Config, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, io::Write, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    pub id: String,
    pub workspace: String,
    pub node_id: String,
    pub type_key: String,
    pub expected_schema: u64,
    pub expected_revision: u64,
    pub text: String,
    #[serde(default)]
    pub staged_operations: Vec<Operation>,
    #[serde(default)]
    pub expected_nodes: BTreeMap<String, u64>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub pending_mutation: Option<Mutation>,
}
fn root() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("projman/recovery"));
    }
    Ok(PathBuf::from(
        std::env::var_os("HOME")
            .ok_or_else(|| Error::new("configuration", "HOME or XDG_STATE_HOME is required"))?,
    )
    .join(".local/state/projman/recovery"))
}
fn path(id: &str) -> Result<PathBuf> {
    Uuid::parse_str(id).map_err(|_| Error::validation("Recovery ID must be a UUID"))?;
    Ok(root()?.join(format!("{id}.json")))
}
pub fn save(config: &Config, mut recovery: Recovery) -> Result<Value> {
    if recovery.id.is_empty() {
        recovery.id = Uuid::new_v4().to_string();
    }
    if recovery.workspace != config.workspace {
        return Err(Error::validation("Recovery belongs to another workspace"));
    }
    let path = path(&recovery.id)?;
    if path.exists() {
        load(config, &recovery.id)?;
    }
    fs::create_dir_all(path.parent().unwrap())?;
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(serde_json::to_string_pretty(&recovery)?.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&path)
        .map_err(|e| Error::new("io", e.to_string()))?;
    Ok(json!({"id":recovery.id,"path":path}))
}
pub fn load(config: &Config, id: &str) -> Result<Recovery> {
    let contents = fs::read_to_string(path(id)?).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::missing("recovery", id)
        } else {
            Error::from(e)
        }
    })?;
    let recovery: Recovery = serde_json::from_str(&contents)?;
    if recovery.workspace != config.workspace {
        return Err(Error::missing("recovery", id));
    }
    Ok(recovery)
}
pub async fn call(config: &Config, method: &str, p: Value) -> Result<Value> {
    match method {
        "recovery.save" => save(config, serde_json::from_value(p)?),
        "recovery.list" => {
            let mut entries = Vec::new();
            if root()?.exists() {
                for entry in fs::read_dir(root()?)? {
                    let file = entry?.path();
                    if file.extension().is_some_and(|s| s == "json") {
                        let parsed: std::result::Result<Recovery, _> =
                            serde_json::from_str(&fs::read_to_string(&file)?);
                        if let Ok(recovery) = parsed
                            && recovery.workspace == config.workspace
                        {
                            entries.push(json!({"id":recovery.id,"node_id":recovery.node_id,"type_key":recovery.type_key,"expected_revision":recovery.expected_revision,"path":file}));
                        }
                    }
                }
            }
            entries.sort_by_key(|e| e["id"].as_str().unwrap_or("").to_owned());
            Ok(json!({"items":entries}))
        }
        "recovery.show" | "recovery.export" => Ok(serde_json::to_value(load(
            config,
            crate::app::string(&p, "id")?,
        )?)?),
        "recovery.remove" => {
            let id = crate::app::string(&p, "id")?;
            load(config, id)?;
            fs::remove_file(path(id)?)?;
            Ok(json!({"removed":id}))
        }
        "recovery.restore" => {
            let id = crate::app::string(&p, "id")?;
            let mut recovery = load(config, id)?;
            let store = Store::connect(config).await?;
            if let Some(mutation) = &recovery.pending_mutation {
                let receipt = store.mutate(mutation, false).await?;
                fs::remove_file(path(id)?)?;
                return Ok(serde_json::to_value(receipt)?);
            }
            let state = store.read().await?;
            let schema_revision = state
                .nodes
                .get(&recovery.node_id)
                .map_or(recovery.expected_schema, |node| node.schema_revision);
            let ty = state
                .schema_at(schema_revision)?
                .node_type(&recovery.type_key)?;
            let parsed = document::parse_with_schema(ty, &recovery.text)?;
            document::validate(ty, &parsed)?;
            let op = if recovery.expected_revision == 0 {
                Operation::CreateNode {
                    id: Some(recovery.node_id.clone()),
                    type_key: recovery.type_key.clone(),
                    properties: parsed.properties,
                    body: parsed.body,
                }
            } else {
                let node = state.node(&recovery.node_id)?;
                let unset = node
                    .properties
                    .keys()
                    .filter(|key| !parsed.properties.contains_key(*key))
                    .cloned()
                    .collect();
                Operation::UpdateNode {
                    id: recovery.node_id.clone(),
                    set: parsed.properties,
                    unset,
                    body: Some(parsed.body),
                    archived: None,
                }
            };
            if recovery.operation_id.is_none() {
                recovery.operation_id = Some(Uuid::new_v4().to_string());
                save(config, recovery.clone())?;
            }
            let mut operations = vec![op];
            operations.extend(recovery.staged_operations.clone());
            let mut expected = recovery.expected_nodes.clone();
            expected.insert(recovery.node_id.clone(), recovery.expected_revision);
            let mutation = Mutation {
                operation_id: recovery.operation_id.clone().unwrap(),
                expected_schema: recovery.expected_schema,
                expected_graph: None,
                expected_nodes: expected,
                action: projman_core::Action::Changes { operations },
            };
            recovery.pending_mutation = Some(mutation.clone());
            save(config, recovery)?;
            let receipt = store.mutate(&mutation, false).await?;
            // A successful, acknowledged restore is the only automatic removal.
            fs::remove_file(path(id)?)?;
            Ok(serde_json::to_value(receipt)?)
        }
        _ => Err(Error::new(
            "usage",
            format!("Unknown recovery method '{method}'"),
        )),
    }
}
