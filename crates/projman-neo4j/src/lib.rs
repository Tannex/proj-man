//! Real Neo4j persistence. The domain transition runs under a workspace write
//! lock so revision checks and graph invariants hold across CLI processes.
use neo4rs::{BoltType, ConfigBuilder, Graph, Txn, query};
use projman_core::{Error, Mutation, Receipt, Result, State, graph};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone, Deserialize, Serialize)]
pub struct Config {
    pub uri: String,
    pub user: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub database: String,
    pub workspace: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            uri: "bolt://127.0.0.1:17687".into(),
            user: "neo4j".into(),
            password: String::new(),
            database: "neo4j".into(),
            workspace: "personal".into(),
        }
    }
}
fn db_error(e: neo4rs::Error) -> Error {
    if let neo4rs::Error::Neo4j(ref error) = e
        && error
            .code()
            .starts_with("Neo.ClientError.Schema.Constraint")
    {
        return Error::conflict(format!("Neo4j: {e}"));
    }
    Error::new("unavailable", format!("Neo4j: {e}"))
}
fn decode_error(e: impl std::fmt::Display) -> Error {
    Error::new("storage", format!("Invalid stored record: {e}"))
}
#[derive(Clone)]
pub struct Store {
    graph: Graph,
    workspace: String,
}
impl Store {
    pub async fn connect(config: &Config) -> Result<Self> {
        if config.workspace.is_empty() || config.workspace.len() > 128 {
            return Err(Error::validation(
                "Workspace name must contain 1–128 characters",
            ));
        }
        let options = ConfigBuilder::default()
            .uri(&config.uri)
            .user(&config.user)
            .password(&config.password)
            .db(config.database.as_str())
            .max_connections(8)
            .build()
            .map_err(db_error)?;
        let graph = tokio::time::timeout(Duration::from_secs(10), Graph::connect(options))
            .await
            .map_err(|_| Error::new("unavailable", "Neo4j connection timed out"))?
            .map_err(db_error)?;
        Ok(Self {
            graph,
            workspace: config.workspace.clone(),
        })
    }
    pub async fn doctor(&self) -> Result<Value> {
        let mut rows = self.graph.execute(query("CALL dbms.components() YIELD name, versions, edition RETURN name, versions, edition")).await.map_err(db_error)?;
        let mut components = Vec::new();
        while let Some(row) = rows.next().await.map_err(db_error)? {
            components.push(json!({"name":row.get::<String>("name").map_err(decode_error)?,
                "versions":row.get::<Vec<String>>("versions").map_err(decode_error)?,"edition":row.get::<String>("edition").map_err(decode_error)?}));
        }
        Ok(json!({"database":components,"workspace":self.workspace,"connected":true}))
    }
    pub async fn initialize(&self) -> Result<()> {
        // Constraints are intentionally limited to Community-supported uniqueness.
        for statement in [
            "CREATE CONSTRAINT pm_workspace_id IF NOT EXISTS FOR (n:PMWorkspace) REQUIRE n.id IS UNIQUE",
            "CREATE CONSTRAINT pm_node_id IF NOT EXISTS FOR (n:PMNode) REQUIRE n.id IS UNIQUE",
            "CREATE CONSTRAINT pm_schema_key IF NOT EXISTS FOR (n:PMSchema) REQUIRE n.key IS UNIQUE",
            "CREATE CONSTRAINT pm_receipt_key IF NOT EXISTS FOR (n:PMReceipt) REQUIRE n.key IS UNIQUE",
            "CREATE CONSTRAINT pm_proposal_key IF NOT EXISTS FOR (n:PMProposal) REQUIRE n.key IS UNIQUE",
            "CREATE CONSTRAINT pm_edge_id IF NOT EXISTS FOR ()-[r:PM_LINK]-() REQUIRE r.id IS UNIQUE",
        ] {
            self.graph.run(query(statement)).await.map_err(db_error)?;
        }
        self.graph.run(query("MERGE (w:PMWorkspace {id:$w}) ON CREATE SET w.revision=0, w.schema_revision=0, w.lock_epoch=0")
            .param("w",self.workspace.clone())).await.map_err(db_error)?;
        Ok(())
    }
    async fn locked(&self) -> Result<Txn> {
        let mut tx = self.graph.start_txn().await.map_err(db_error)?;
        let mut rows = tx.execute(query("MATCH (w:PMWorkspace {id:$w}) SET w.lock_epoch=w.lock_epoch+1 RETURN w.id AS id")
            .param("w",self.workspace.clone())).await.map_err(db_error)?;
        if rows.next(tx.handle()).await.map_err(db_error)?.is_none() {
            tx.rollback().await.map_err(db_error)?;
            return Err(Error::new(
                "configuration",
                "Workspace is not initialized; run 'projman workspace init'",
            ));
        }
        // Consume the stream before issuing another query on this connection.
        while rows.next(tx.handle()).await.map_err(db_error)?.is_some() {}
        Ok(tx)
    }
    pub async fn read(&self) -> Result<State> {
        let mut tx = self.locked().await?;
        let state = self.load(&mut tx).await;
        let rollback = tx.rollback().await.map_err(db_error);
        state.and_then(|s| rollback.map(|_| s))
    }
    pub async fn mutate(&self, mutation: &Mutation, dry_run: bool) -> Result<Receipt> {
        let mut tx = self.locked().await?;
        let result = async {
            let old = self.load(&mut tx).await?;
            let (new, receipt) = graph::apply(&old, mutation)?;
            if !dry_run && !old.receipts.contains_key(&mutation.operation_id) {
                self.persist(&mut tx, &old, &new, &receipt).await?;
            }
            Ok(receipt)
        }
        .await;
        match result {
            Ok(receipt) if !dry_run => {
                tx.commit().await.map_err(|e| db_error(e).with_details(json!({"operation_id":mutation.operation_id,"outcome":"unknown","next":"Inspect the operation receipt before retrying"})))?;
                Ok(receipt)
            }
            Ok(receipt) => {
                tx.rollback().await.map_err(db_error)?;
                Ok(receipt)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }
    async fn load(&self, tx: &mut Txn) -> Result<State> {
        let mut state = State::new(&self.workspace);
        let mut rows = tx.execute(query("MATCH (w:PMWorkspace {id:$w}) RETURN w.revision AS revision,w.schema_revision AS schema_revision")
            .param("w",self.workspace.clone())).await.map_err(db_error)?;
        while let Some(row) = rows.next(tx.handle()).await.map_err(db_error)? {
            state.revision = row.get::<i64>("revision").map_err(decode_error)? as u64;
            state.schema_revision = row.get::<i64>("schema_revision").map_err(decode_error)? as u64;
        }
        for (kind, statement) in [
            (
                "proposal",
                "MATCH (n:PMProposal {workspace:$w}) RETURN n.data AS data",
            ),
            (
                "schema",
                "MATCH (n:PMSchema {workspace:$w}) RETURN n.data AS data,n.revision AS revision",
            ),
            (
                "node",
                "MATCH (n:PMNode {workspace:$w}) RETURN n.data AS data",
            ),
            (
                "edge",
                "MATCH (:PMNode {workspace:$w})-[r:PM_LINK]->(:PMNode {workspace:$w}) RETURN r.data AS data",
            ),
            (
                "receipt",
                "MATCH (n:PMReceipt {workspace:$w}) RETURN n.data AS data",
            ),
        ] {
            let mut rows = tx
                .execute(query(statement).param("w", self.workspace.clone()))
                .await
                .map_err(db_error)?;
            while let Some(row) = rows.next(tx.handle()).await.map_err(db_error)? {
                let data: String = row.get("data").map_err(decode_error)?;
                match kind {
                    "proposal" => {
                        let proposal: projman_core::proposal::Proposal =
                            serde_json::from_str(&data).map_err(decode_error)?;
                        state.proposals.insert(proposal.draft.id.clone(), proposal);
                    }
                    "schema" => {
                        let revision: i64 = row.get("revision").map_err(decode_error)?;
                        state.schemas.insert(
                            revision as u64,
                            serde_json::from_str(&data).map_err(decode_error)?,
                        );
                    }
                    "node" => {
                        let n: projman_core::Node =
                            serde_json::from_str(&data).map_err(decode_error)?;
                        state.nodes.insert(n.id.clone(), n);
                    }
                    "edge" => {
                        let e: projman_core::Edge =
                            serde_json::from_str(&data).map_err(decode_error)?;
                        state.edges.insert(e.id.clone(), e);
                    }
                    "receipt" => {
                        let r: Receipt = serde_json::from_str(&data).map_err(decode_error)?;
                        state.receipts.insert(r.operation_id.clone(), r);
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok(state)
    }
    async fn persist(
        &self,
        tx: &mut Txn,
        old: &State,
        new: &State,
        receipt: &Receipt,
    ) -> Result<()> {
        for (id, proposal) in &new.proposals {
            let data = serde_json::to_string(proposal)?;
            if old
                .proposals
                .get(id)
                .map(serde_json::to_string)
                .transpose()?
                .as_ref()
                == Some(&data)
            {
                continue;
            }
            tx.run(
                query("MERGE (p:PMProposal {key:$key}) SET p.workspace=$w,p.data=$data")
                    .param("key", format!("{}/{}", self.workspace, id))
                    .param("w", self.workspace.clone())
                    .param("data", data),
            )
            .await
            .map_err(db_error)?;
        }
        for (revision, schema) in &new.schemas {
            if !old.schemas.contains_key(revision) {
                tx.run(
                    query(
                        "CREATE (:PMSchema {key:$key,workspace:$w,revision:$revision,data:$data})",
                    )
                    .param("key", format!("{}/{}", self.workspace, revision))
                    .param("w", self.workspace.clone())
                    .param("revision", *revision as i64)
                    .param("data", serde_json::to_string(schema)?),
                )
                .await
                .map_err(db_error)?;
            }
        }
        // Remove changed edges first; this also supports reparenting with an unchanged edge ID.
        for edge in old.edges.values() {
            if new.edges.get(&edge.id) != Some(edge) {
                tx.run(
                    query("MATCH (:PMNode {workspace:$w})-[r:PM_LINK {id:$id}]->() DELETE r")
                        .param("w", self.workspace.clone())
                        .param("id", edge.id.clone()),
                )
                .await
                .map_err(db_error)?;
            }
        }
        for node in new.nodes.values() {
            if old.nodes.get(&node.id) == Some(node) {
                continue;
            }
            let mut props = HashMap::<String, BoltType>::new();
            props.insert("id".into(), node.id.clone().into());
            props.insert("workspace".into(), self.workspace.clone().into());
            props.insert("type_key".into(), node.type_key.clone().into());
            props.insert("revision".into(), (node.revision as i64).into());
            props.insert("display_title".into(), new.title(node).into());
            props.insert("data".into(), serde_json::to_string(node)?.into());
            for (key, value) in &node.properties {
                if !value.is_null() {
                    let definition = new
                        .schema_at(node.schema_revision)?
                        .node_type(&node.type_key)?
                        .properties
                        .iter()
                        .find(|p| &p.key == key)
                        .ok_or_else(|| Error::validation("Unknown property projection"))?;
                    // Neo4j arrays must have one stored scalar type. JSON number
                    // fields accept both integer and fractional syntax.
                    let projected = if definition.value_type == projman_core::ValueType::Number {
                        BoltType::from(
                            value
                                .as_f64()
                                .ok_or_else(|| Error::validation("Expected numeric projection"))?,
                        )
                    } else if definition.value_type == projman_core::ValueType::List
                        && definition.items == Some(projman_core::ValueType::Number)
                    {
                        let values: Vec<f64> = value
                            .as_array()
                            .ok_or_else(|| Error::validation("Expected array projection"))?
                            .iter()
                            .map(|v| {
                                v.as_f64()
                                    .ok_or_else(|| Error::validation("Expected numeric list item"))
                            })
                            .collect::<Result<_>>()?;
                        BoltType::from(values)
                    } else {
                        BoltType::try_from(value.clone()).map_err(db_error)?
                    };
                    props.insert(format!("p_{key}"), projected);
                }
            }
            let statement = if old.nodes.contains_key(&node.id) {
                "MATCH (n:PMNode {id:$id,workspace:$w}) SET n=$props"
            } else {
                "CREATE (n:PMNode) SET n=$props"
            };
            tx.run(
                query(statement)
                    .param("id", node.id.clone())
                    .param("w", self.workspace.clone())
                    .param("props", props),
            )
            .await
            .map_err(db_error)?;
        }
        for edge in new.edges.values() {
            if old.edges.get(&edge.id) == Some(edge) {
                continue;
            }
            tx.run(query("MATCH (a:PMNode {id:$source,workspace:$w}), (b:PMNode {id:$target,workspace:$w}) CREATE (a)-[:PM_LINK {id:$id,type_key:$type,position:$position,data:$data}]->(b)")
                .param("w",self.workspace.clone()).param("source",edge.source.clone()).param("target",edge.target.clone()).param("id",edge.id.clone())
                .param("type",edge.type_key.clone()).param("position",edge.position as i64).param("data",serde_json::to_string(edge)?)).await.map_err(db_error)?;
        }
        tx.run(
            query("CREATE (:PMReceipt {key:$key,workspace:$w,data:$data})")
                .param(
                    "key",
                    format!("{}/{}", self.workspace, receipt.operation_id),
                )
                .param("w", self.workspace.clone())
                .param("data", serde_json::to_string(receipt)?),
        )
        .await
        .map_err(db_error)?;
        tx.run(
            query(
                "MATCH (w:PMWorkspace {id:$w}) SET w.revision=$revision,w.schema_revision=$schema",
            )
            .param("w", self.workspace.clone())
            .param("revision", new.revision as i64)
            .param("schema", new.schema_revision as i64),
        )
        .await
        .map_err(db_error)?;
        Ok(())
    }
}
