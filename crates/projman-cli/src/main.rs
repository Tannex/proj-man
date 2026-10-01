mod app;
mod display;
mod recovery;
mod transport;
use clap::{Args, Parser, Subcommand};
use projman_core::{Action, Error, Mutation, Operation, Properties, Result, document};
use projman_neo4j::{Config, Store};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::PathBuf,
};
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "projman",
    version,
    about = "Personal planning with custom schemas and a Neo4j graph"
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    non_interactive: bool,
    #[arg(
        long,
        global = true,
        env = "PROJMAN_NEO4J_URI",
        default_value = "bolt://127.0.0.1:17687"
    )]
    uri: String,
    #[arg(
        long,
        global = true,
        env = "PROJMAN_NEO4J_USER",
        default_value = "neo4j"
    )]
    user: String,
    #[arg(
        long,
        global = true,
        env = "PROJMAN_NEO4J_DATABASE",
        default_value = "neo4j"
    )]
    database: String,
    #[arg(
        long,
        global = true,
        env = "PROJMAN_WORKSPACE",
        default_value = "personal"
    )]
    workspace: String,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Explore a bounded breadth-first spanning tree from any node
    Explore {
        root: String,
        #[arg(long, default_value="outgoing", value_parser=["outgoing","incoming","both"])]
        direction: String,
        #[arg(long)]
        relation: Option<String>,
        #[arg(long)]
        family: Option<String>,
        #[arg(long, default_value_t = 3)]
        max_depth: usize,
        #[arg(long, default_value_t = 500)]
        max_nodes: usize,
        #[arg(long, default_value_t = 1000)]
        max_references: usize,
        #[arg(long)]
        include_archived: bool,
    },
    /// Submit, review, reject, and explicitly apply manual proposals
    Proposal {
        #[command(subcommand)]
        command: ProposalCmd,
    },
    /// Check Neo4j connectivity and runtime capabilities
    Doctor,
    /// Show effective connection and workspace settings
    Config,
    /// Initialize or inspect a logical workspace
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCmd,
    },
    /// Validate, preview, publish, export, and migrate custom schemas
    Schema {
        #[command(subcommand)]
        command: SchemaCmd,
    },
    /// Inspect custom node and relationship definitions
    Type {
        #[command(subcommand)]
        command: TypeCmd,
    },
    /// Create, edit, list, and archive nodes
    Node {
        #[command(subcommand)]
        command: NodeCmd,
    },
    /// Suggest, add, remove, and order typed relationships
    Link {
        #[command(subcommand)]
        command: LinkCmd,
    },
    /// Search node titles and Markdown
    Search {
        query: String,
        #[command(flatten)]
        page: Page,
    },
    /// Display a tree from an ordered relationship family
    Outline {
        id: String,
        #[arg(long)]
        family: String,
    },
    /// Show inverse relationships and Markdown mentions
    Backlinks { id: String },
    /// Export the selected workspace and its history as JSON
    Export,
    /// Inspect and restore local editing snapshots
    Recovery {
        #[command(subcommand)]
        command: RecoveryCmd,
    },
    /// Validate or atomically apply a structured change batch
    Change {
        #[command(subcommand)]
        command: ChangeCmd,
    },
    /// Inspect a mutation receipt by operation UUID
    Operation { id: String },
    /// Run the optional newline-framed editor protocol over stdio
    Serve {
        #[arg(long, required = true)]
        stdio: bool,
    },
}
#[derive(Subcommand)]
enum ProposalCmd {
    List,
    Submit {
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        operation_id: Option<String>,
    },
    Show {
        id: String,
    },
    Validate {
        id: String,
        #[arg(long, value_delimiter = ',')]
        groups: Option<Vec<String>>,
    },
    Reject {
        id: String,
        #[arg(long)]
        digest: String,
        #[arg(long)]
        operation_id: Option<String>,
    },
    Apply {
        id: String,
        #[arg(long)]
        digest: String,
        #[arg(long, value_delimiter = ',', required = true)]
        groups: Vec<String>,
        #[arg(long)]
        operation_id: Option<String>,
    },
}
#[derive(Subcommand)]
enum WorkspaceCmd {
    Init,
    Show,
}
#[derive(Subcommand)]
enum TypeCmd {
    List,
    Show { key: String },
}
#[derive(Subcommand)]
enum ChangeCmd {
    Validate(Input),
    Apply(Input),
}
#[derive(Subcommand)]
enum RecoveryCmd {
    List,
    Show { id: String },
    Export { id: String },
    Restore { id: String },
    Remove { id: String },
    Save(Input),
}
#[derive(Args)]
struct Input {
    #[arg(long, conflicts_with = "stdin")]
    file: Option<PathBuf>,
    #[arg(long)]
    stdin: bool,
}
impl Input {
    fn text(&self) -> Result<String> {
        match (&self.file, self.stdin) {
            (Some(file), false) => Ok(std::fs::read_to_string(file)?),
            (None, true) => {
                let mut text = String::new();
                std::io::stdin()
                    .take(4 * 1024 * 1024 + 1)
                    .read_to_string(&mut text)?;
                if text.len() > 4 * 1024 * 1024 {
                    return Err(Error::validation("Input exceeds 4 MiB"));
                }
                Ok(text)
            }
            _ => Err(Error::new("usage", "Supply --file or --stdin")),
        }
    }
    fn value(&self) -> Result<Value> {
        Ok(serde_json::from_str(&self.text()?)?)
    }
    fn optional(&self) -> Result<Value> {
        if self.file.is_none() && !self.stdin {
            Ok(json!({}))
        } else {
            self.value()
        }
    }
}
#[derive(Args)]
struct WriteOptions {
    #[arg(long)]
    operation_id: Option<String>,
    #[arg(long)]
    expect_schema: Option<u64>,
    #[arg(long)]
    expect_graph: Option<u64>,
}
#[derive(Args)]
struct Page {
    #[arg(long, default_value_t = 0)]
    offset: usize,
    #[arg(long, default_value_t = 100)]
    limit: usize,
}
impl Page {
    fn params(&self) -> Value {
        json!({"offset":self.offset,"limit":self.limit})
    }
}
#[derive(Subcommand)]
enum SchemaCmd {
    Validate(Input),
    Preview(Input),
    Publish {
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        expect_schema: u64,
        #[arg(long)]
        expect_graph: u64,
        #[arg(long)]
        retain_legacy: bool,
        #[arg(long)]
        operation_id: Option<String>,
    },
    Export {
        #[arg(long)]
        revision: Option<u64>,
    },
    Migrate {
        id: String,
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        expect_graph: u64,
        #[command(flatten)]
        write: MigrationWrite,
    },
}
#[derive(Args)]
struct MigrationWrite {
    #[arg(long)]
    operation_id: Option<String>,
    #[arg(long)]
    expect_schema: Option<u64>,
}
#[derive(Subcommand)]
enum NodeCmd {
    /// Filter root candidates by title, type, or ID (all query terms must match)
    Pick {
        #[arg(default_value = "")]
        query: String,
        #[arg(long)]
        include_archived: bool,
        #[command(flatten)]
        page: Page,
    },
    Create {
        #[arg(long = "type")]
        type_key: String,
        #[arg(long)]
        id: Option<String>,
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        body_file: Option<PathBuf>,
        #[command(flatten)]
        write: WriteOptions,
    },
    Get {
        id: String,
        #[arg(long)]
        document: bool,
    },
    List {
        #[arg(long = "type")]
        type_key: Option<String>,
        #[arg(long, requires = "source")]
        relation: Option<String>,
        #[arg(long, requires = "relation")]
        source: Option<String>,
        #[arg(long, requires = "relation")]
        incoming: bool,
        #[arg(long)]
        include_archived: bool,
        #[command(flatten)]
        page: Page,
    },
    Update {
        id: String,
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        expect_revision: u64,
        #[command(flatten)]
        write: WriteOptions,
    },
    Archive {
        id: String,
        #[arg(long)]
        restore: bool,
        #[arg(long)]
        expect_revision: u64,
        #[command(flatten)]
        write: WriteOptions,
    },
    Edit {
        id: String,
        #[arg(long)]
        editor: Option<String>,
    },
}
#[derive(Subcommand)]
enum LinkCmd {
    List {
        #[arg(long)]
        node: Option<String>,
        #[command(flatten)]
        page: Page,
    },
    Suggest {
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
    },
    Add {
        #[arg(long)]
        source: String,
        #[arg(long)]
        target: String,
        #[arg(long = "type")]
        type_key: String,
        #[arg(long)]
        position: Option<u32>,
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        expect_graph: u64,
        #[command(flatten)]
        write: MigrationWrite,
    },
    Remove {
        id: String,
        #[arg(long)]
        expect_graph: u64,
        #[command(flatten)]
        write: MigrationWrite,
    },
    Reorder {
        parent: String,
        #[arg(long)]
        family: String,
        #[arg(long, value_delimiter = ',')]
        edges: Vec<String>,
        #[arg(long)]
        expect_graph: u64,
        #[command(flatten)]
        write: MigrationWrite,
    },
    Reparent {
        id: String,
        #[arg(long)]
        parent: String,
        #[arg(long)]
        expect_graph: u64,
        #[command(flatten)]
        write: MigrationWrite,
    },
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeInput {
    #[serde(default)]
    properties: Properties,
    #[serde(default)]
    body: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkInput {
    #[serde(default)]
    properties: Properties,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationInput {
    properties: Properties,
    #[serde(default)]
    type_key: Option<String>,
}
fn config(cli: &Cli) -> Config {
    Config {
        uri: cli.uri.clone(),
        user: cli.user.clone(),
        database: cli.database.clone(),
        workspace: cli.workspace.clone(),
        password: std::env::var("PROJMAN_NEO4J_PASSWORD").unwrap_or_default(),
    }
}
fn output(value: &Result<Value>, as_json: bool) {
    if as_json {
        let envelope = match value {
            Ok(data) => json!({"protocol_version":1,"ok":true,"data":data}),
            Err(error) => json!({"protocol_version":1,"ok":false,"error":error}),
        };
        println!(
            "{}",
            serde_json::to_string(&envelope).expect("serializable output")
        );
    } else {
        match value {
            Ok(data) => println!("{}", display::render(data)),
            Err(error) => {
                eprintln!("{error}");
                if !error.details.is_null() {
                    eprintln!("{}", error.details);
                }
            }
        }
    }
}
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let json_mode = args.iter().any(|a| a == "--json");
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                print!("{e}");
                return;
            }
            output(&Err(Error::new("usage", e.to_string())), json_mode);
            std::process::exit(2);
        }
    };
    if matches!(cli.command, Command::Serve { .. }) {
        if let Err(e) = serve(config(&cli)).await {
            eprintln!("{e}");
            std::process::exit(e.exit_code());
        }
        return;
    }
    let result = run(&cli).await;
    output(&result, cli.json);
    if let Err(error) = result {
        std::process::exit(error.exit_code());
    }
}
async fn execute(
    config: &Config,
    operations: Vec<Operation>,
    write: &WriteOptions,
    expected: BTreeMap<String, u64>,
) -> Result<Value> {
    let store = Store::connect(config).await?;
    let state = store.read().await?;
    let previous = write
        .operation_id
        .as_ref()
        .and_then(|id| state.receipts.get(id))
        .and_then(|r| r.request.as_ref());
    let mutation = app::change(
        &state,
        operations,
        write.operation_id.clone(),
        write.expect_graph,
        expected,
        write.expect_schema.or(previous.map(|m| m.expected_schema)),
    );
    Ok(serde_json::to_value(store.mutate(&mutation, false).await?)?)
}
async fn graph_execute(
    config: &Config,
    operations: Vec<Operation>,
    graph: u64,
    write: &MigrationWrite,
) -> Result<Value> {
    let store = Store::connect(config).await?;
    let state = store.read().await?;
    let previous = write
        .operation_id
        .as_ref()
        .and_then(|id| state.receipts.get(id))
        .and_then(|r| r.request.as_ref());
    let expected = previous
        .map(|m| m.expected_nodes.clone())
        .unwrap_or_else(|| {
            state
                .nodes
                .iter()
                .map(|(id, n)| (id.clone(), n.revision))
                .collect()
        });
    let mutation = app::change(
        &state,
        operations,
        write.operation_id.clone(),
        Some(graph),
        expected,
        write.expect_schema.or(previous.map(|m| m.expected_schema)),
    );
    Ok(serde_json::to_value(store.mutate(&mutation, false).await?)?)
}
async fn run(cli: &Cli) -> Result<Value> {
    let cfg = config(cli);
    match &cli.command {
        Command::Explore { root,direction,relation,family,max_depth,max_nodes,max_references,include_archived } =>
            app::call(&cfg,"explorer.tree",json!({"root":root,"direction":direction,"relation":relation,"family":family,"max_depth":max_depth,"max_nodes":max_nodes,"max_references":max_references,"include_archived":include_archived})).await,
        Command::Proposal { command } => match command {
            ProposalCmd::List => app::call(&cfg, "proposal.list", json!({})).await,
            ProposalCmd::Show { id } => app::call(&cfg, "proposal.show", json!({"id":id})).await,
            ProposalCmd::Validate { id, groups } => {
                app::call(&cfg, "proposal.validate", json!({"id":id,"groups":groups})).await
            }
            ProposalCmd::Submit {
                input,
                operation_id,
            } => {
                proposal_action(
                    &cfg,
                    Action::SubmitProposal {
                        proposal: serde_json::from_value(input.value()?)?,
                    },
                    operation_id.clone(),
                )
                .await
            }
            ProposalCmd::Reject {
                id,
                digest,
                operation_id,
            } => {
                proposal_action(
                    &cfg,
                    Action::RejectProposal {
                        id: id.clone(),
                        digest: digest.clone(),
                    },
                    operation_id.clone(),
                )
                .await
            }
            ProposalCmd::Apply {
                id,
                digest,
                groups,
                operation_id,
            } => {
                proposal_action(
                    &cfg,
                    Action::ApplyProposal {
                        id: id.clone(),
                        digest: digest.clone(),
                        groups: groups.clone(),
                    },
                    operation_id.clone(),
                )
                .await
            }
        },
        Command::Doctor => app::call(&cfg, "doctor", json!({})).await,
        Command::Config => app::call(&cfg, "config.show", json!({})).await,
        Command::Workspace { command } => {
            app::call(
                &cfg,
                match command {
                    WorkspaceCmd::Init => "workspace.init",
                    WorkspaceCmd::Show => "workspace.show",
                },
                json!({}),
            )
            .await
        }
        Command::Type { command } => match command {
            TypeCmd::List => app::call(&cfg, "type.list", json!({})).await,
            TypeCmd::Show { key } => app::call(&cfg, "type.show", json!({"key":key})).await,
        },
        Command::Schema { command } => match command {
            SchemaCmd::Validate(input) => {
                app::call(&cfg, "schema.validate", json!({"schema":input.value()?})).await
            }
            SchemaCmd::Preview(input) => {
                app::call(&cfg, "schema.preview", json!({"schema":input.value()?})).await
            }
            SchemaCmd::Export { revision } => {
                app::call(&cfg, "schema.export", json!({"revision":revision})).await
            }
            SchemaCmd::Publish {
                input,
                expect_schema,
                expect_graph,
                retain_legacy,
                operation_id,
            } => {
                let mutation = Mutation {
                    operation_id: operation_id
                        .clone()
                        .unwrap_or_else(|| Uuid::new_v4().to_string()),
                    expected_schema: *expect_schema,
                    expected_graph: Some(*expect_graph),
                    expected_nodes: BTreeMap::new(),
                    action: Action::PublishSchema {
                        schema: serde_json::from_value(input.value()?)?,
                        retain_legacy: *retain_legacy,
                    },
                };
                app::call(&cfg, "change.apply", serde_json::to_value(mutation)?).await
            }
            SchemaCmd::Migrate {
                id,
                input,
                expect_graph,
                write,
            } => {
                let data: MigrationInput = serde_json::from_value(input.value()?)?;
                graph_execute(
                    &cfg,
                    vec![Operation::MigrateNode {
                        id: id.clone(),
                        type_key: data.type_key,
                        properties: data.properties,
                    }],
                    *expect_graph,
                    write,
                )
                .await
            }
        },
        Command::Node { command } => match command {
            NodeCmd::Pick { query,include_archived,page } => {
                let mut params=page.params();params["query"]=json!(query);params["include_archived"]=json!(include_archived);
                app::call(&cfg,"node.pick",params).await
            }
            NodeCmd::Create {
                type_key,
                id,
                input,
                body_file,
                write,
            } => {
                let data: NodeInput = serde_json::from_value(input.optional()?)?;
                let body = if let Some(path) = body_file {
                    std::fs::read_to_string(path)?
                } else {
                    data.body
                };
                execute(
                    &cfg,
                    vec![Operation::CreateNode {
                        id: id.clone(),
                        type_key: type_key.clone(),
                        properties: data.properties,
                        body,
                    }],
                    write,
                    BTreeMap::new(),
                )
                .await
            }
            NodeCmd::Get { id, document } => {
                app::call(
                    &cfg,
                    if *document {
                        "node.document"
                    } else {
                        "node.get"
                    },
                    json!({"id":id}),
                )
                .await
            }
            NodeCmd::List {
                type_key,
                relation,
                source,
                incoming,
                include_archived,
                page,
            } => {
                let mut p = page.params();
                p["type_key"] = json!(type_key);
                p["relation"] = json!(relation);
                p["source"] = json!(source);
                p["direction"] = json!(if *incoming { "incoming" } else { "outgoing" });
                p["include_archived"] = json!(include_archived);
                app::call(&cfg, "node.list", p).await
            }
            NodeCmd::Update {
                id,
                input,
                expect_revision,
                write,
            } => {
                let data = input.value()?;
                if !data.is_object() {
                    return Err(Error::validation("Node patch must be a JSON object"));
                }
                let mut op = data.clone();
                op["op"] = json!("update_node");
                op["id"] = json!(id);
                execute(
                    &cfg,
                    vec![serde_json::from_value(op)?],
                    write,
                    BTreeMap::from([(id.clone(), *expect_revision)]),
                )
                .await
            }
            NodeCmd::Archive {
                id,
                restore,
                expect_revision,
                write,
            } => {
                execute(
                    &cfg,
                    vec![Operation::UpdateNode {
                        id: id.clone(),
                        set: BTreeMap::new(),
                        unset: vec![],
                        body: None,
                        archived: Some(!restore),
                    }],
                    write,
                    BTreeMap::from([(id.clone(), *expect_revision)]),
                )
                .await
            }
            NodeCmd::Edit { id, editor } => {
                if cli.non_interactive {
                    return Err(Error::new(
                        "usage",
                        "node edit requires an editor; use node update --file for noninteractive input",
                    ));
                }
                edit(&cfg, id, editor.as_deref()).await
            }
        },
        Command::Link { command } => match command {
            LinkCmd::List { node, page } => {
                let mut p = page.params();
                p["id"] = json!(node);
                app::call(&cfg, "link.list", p).await
            }
            LinkCmd::Suggest { source, target } => {
                app::call(
                    &cfg,
                    "link.suggest",
                    json!({"source":source,"target":target}),
                )
                .await
            }
            LinkCmd::Add {
                source,
                target,
                type_key,
                position,
                input,
                expect_graph,
                write,
            } => {
                let data: LinkInput = serde_json::from_value(input.optional()?)?;
                graph_execute(
                    &cfg,
                    vec![Operation::AddLink {
                        id: None,
                        source: source.clone(),
                        target: target.clone(),
                        type_key: type_key.clone(),
                        properties: data.properties,
                        position: *position,
                    }],
                    *expect_graph,
                    write,
                )
                .await
            }
            LinkCmd::Remove {
                id,
                expect_graph,
                write,
            } => {
                graph_execute(
                    &cfg,
                    vec![Operation::RemoveLink { id: id.clone() }],
                    *expect_graph,
                    write,
                )
                .await
            }
            LinkCmd::Reorder {
                parent,
                family,
                edges,
                expect_graph,
                write,
            } => {
                graph_execute(
                    &cfg,
                    vec![Operation::Reorder {
                        parent: parent.clone(),
                        family: family.clone(),
                        edge_ids: edges.clone(),
                    }],
                    *expect_graph,
                    write,
                )
                .await
            }
            LinkCmd::Reparent {
                id,
                parent,
                expect_graph,
                write,
            } => {
                let store = Store::connect(&cfg).await?;
                let state = store.read().await?;
                let previous = write
                    .operation_id
                    .as_ref()
                    .and_then(|op| state.receipts.get(op))
                    .and_then(|r| r.request.as_ref());
                let addition = if let Some(previous) = previous {
                    let Action::Changes { operations } = &previous.action else {
                        return Err(Error::conflict("Operation ID belongs to another action"));
                    };
                    let [
                        Operation::RemoveLink { id: removed },
                        Operation::AddLink {
                            id: Some(added),
                            target,
                            type_key,
                            properties,
                            position,
                            ..
                        },
                    ] = operations.as_slice()
                    else {
                        return Err(Error::conflict("Operation ID belongs to another action"));
                    };
                    if removed != id || added != id {
                        return Err(Error::conflict(
                            "Operation ID belongs to another relationship",
                        ));
                    }
                    Operation::AddLink {
                        id: Some(id.clone()),
                        source: parent.clone(),
                        target: target.clone(),
                        type_key: type_key.clone(),
                        properties: properties.clone(),
                        position: *position,
                    }
                } else {
                    let edge = state.edge(id)?;
                    Operation::AddLink {
                        id: Some(id.clone()),
                        source: parent.clone(),
                        target: edge.target.clone(),
                        type_key: edge.type_key.clone(),
                        properties: edge.properties.clone(),
                        position: None,
                    }
                };
                graph_execute(
                    &cfg,
                    vec![Operation::RemoveLink { id: id.clone() }, addition],
                    *expect_graph,
                    write,
                )
                .await
            }
        },
        Command::Search { query, page } => {
            let mut p = page.params();
            p["query"] = json!(query);
            app::call(&cfg, "search", p).await
        }
        Command::Outline { id, family } => {
            app::call(&cfg, "outline", json!({"id":id,"family":family})).await
        }
        Command::Backlinks { id } => app::call(&cfg, "backlinks", json!({"id":id})).await,
        Command::Export => app::call(&cfg, "export", json!({})).await,
        Command::Recovery { command } => match command {
            RecoveryCmd::List => app::call(&cfg, "recovery.list", json!({})).await,
            RecoveryCmd::Show { id } => app::call(&cfg, "recovery.show", json!({"id":id})).await,
            RecoveryCmd::Export { id } => {
                app::call(&cfg, "recovery.export", json!({"id":id})).await
            }
            RecoveryCmd::Restore { id } => {
                app::call(&cfg, "recovery.restore", json!({"id":id})).await
            }
            RecoveryCmd::Remove { id } => {
                app::call(&cfg, "recovery.remove", json!({"id":id})).await
            }
            RecoveryCmd::Save(input) => app::call(&cfg, "recovery.save", input.value()?).await,
        },
        Command::Operation { id } => app::call(&cfg, "operation.get", json!({"id":id})).await,
        Command::Change { command } => match command {
            ChangeCmd::Validate(input) => app::call(&cfg, "change.validate", input.value()?).await,
            ChangeCmd::Apply(input) => app::call(&cfg, "change.apply", input.value()?).await,
        },
        Command::Serve { .. } => unreachable!(),
    }
}
async fn edit(config: &Config, id: &str, editor: Option<&str>) -> Result<Value> {
    let editor = editor
        .map(String::from)
        .or_else(|| std::env::var("VISUAL").ok())
        .or_else(|| std::env::var("EDITOR").ok())
        .ok_or_else(|| Error::new("configuration", "Set EDITOR or use --editor"))?;
    let store = Store::connect(config).await?;
    let state = store.read().await?;
    let node = state.node(id)?;
    let ty = state
        .schema_at(node.schema_revision)?
        .node_type(&node.type_key)?;
    let mut recovery = recovery::Recovery {
        id: Uuid::new_v4().to_string(),
        workspace: config.workspace.clone(),
        node_id: id.into(),
        type_key: node.type_key.clone(),
        expected_schema: state.schema_revision,
        expected_revision: node.revision,
        text: document::format(ty, &node.properties, &node.body)?,
        staged_operations: vec![],
        expected_nodes: BTreeMap::new(),
        operation_id: None,
        pending_mutation: None,
    };
    recovery::save(config, recovery.clone())?;
    let mut file = tempfile::Builder::new()
        .prefix("projman-")
        .suffix(".md")
        .tempfile()?;
    file.write_all(recovery.text.as_bytes())?;
    let path = file
        .into_temp_path()
        .keep()
        .map_err(|e| Error::new("io", e.to_string()))?;
    let command = shlex::split(&editor)
        .filter(|parts| !parts.is_empty())
        .ok_or_else(|| {
            Error::new(
                "configuration",
                "EDITOR contains invalid quoting or no executable",
            )
        })?;
    let status = std::process::Command::new(&command[0])
        .args(&command[1..])
        .arg(&path)
        .status();
    recovery.text = std::fs::read_to_string(&path)?;
    recovery::save(config, recovery.clone())?;
    let result = match status {
        Ok(status) if status.success() => {
            recovery::call(config, "recovery.restore", json!({"id":recovery.id})).await
        }
        Ok(_) => Err(Error::new("cancelled", "Editor exited unsuccessfully")),
        Err(error) => Err(Error::from(error)),
    };
    match result {
        Ok(value) => {
            let _ = std::fs::remove_file(&path);
            Ok(value)
        }
        Err(error) => {
            let original = error.details.clone();
            Err(error.with_details(
                json!({"recovery_id":recovery.id,"recovery_file":path,"cause":original}),
            ))
        }
    }
}
async fn serve(config: Config) -> Result<()> {
    use tokio::io::{AsyncWriteExt, BufReader};
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    while let Some(frame) = transport::next_frame(&mut input).await? {
        let mut id = Value::Null;
        let result = match frame {
            Err(error) => Err(error),
            Ok(line) => match serde_json::from_str::<Value>(&line) {
                Err(e) => Err(Error::from(e)),
                Ok(request) => {
                    id = request.get("id").cloned().unwrap_or(Value::Null);
                    if request["protocol_version"] != 1 {
                        Err(Error::new("usage", "Expected protocol_version 1"))
                    } else if let Some(method) = request["method"].as_str() {
                        if method == "initialize" {
                            Ok(
                                json!({"protocol_version":1,"version":env!("CARGO_PKG_VERSION"),"workspace":config.workspace,"capabilities":["schemas","nodes","links","documents","transactions"]}),
                            )
                        } else {
                            app::call(
                                &config,
                                method,
                                request.get("params").cloned().unwrap_or(json!({})),
                            )
                            .await
                        }
                    } else {
                        Err(Error::validation("method is required"))
                    }
                }
            },
        };
        let response = match result {
            Ok(data) => json!({"protocol_version":1,"id":id,"ok":true,"data":data}),
            Err(error) => json!({"protocol_version":1,"id":id,"ok":false,"error":error}),
        };
        output
            .write_all(format!("{}\n", serde_json::to_string(&response)?).as_bytes())
            .await?;
        output.flush().await?;
    }
    Ok(())
}

async fn proposal_action(
    config: &Config,
    action: Action,
    operation_id: Option<String>,
) -> Result<Value> {
    let store = Store::connect(config).await?;
    let state = store.read().await?;
    let expected_schema = operation_id
        .as_ref()
        .and_then(|id| state.receipts.get(id))
        .and_then(|r| r.request.as_ref())
        .map_or(state.schema_revision, |m| m.expected_schema);
    let mutation = Mutation {
        operation_id: operation_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
        expected_schema,
        expected_graph: None,
        expected_nodes: BTreeMap::new(),
        action,
    };
    Ok(serde_json::to_value(store.mutate(&mutation, false).await?)?)
}
