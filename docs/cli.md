# CLI and data contract

Run `projman --help` or a command's `--help` for all arguments. Commands operate on the workspace selected by `--workspace` or `PROJMAN_WORKSPACE` (default `personal`). There is no built-in task taxonomy.

## Connection and output

| Setting | Environment | Default |
| --- | --- | --- |
| `--uri` | `PROJMAN_NEO4J_URI` | `bolt://127.0.0.1:17687` |
| `--user` | `PROJMAN_NEO4J_USER` | `neo4j` |
| Password | `PROJMAN_NEO4J_PASSWORD` | Empty |
| `--database` | `PROJMAN_NEO4J_DATABASE` | `neo4j` |
| `--workspace` | `PROJMAN_WORKSPACE` | `personal` |

`projman config` shows effective non-secret configuration. `doctor` reports database version and edition. `workspace init` creates supported uniqueness constraints and initializes the selected workspace. `workspace show` reports current schema/graph revisions and counts.

`--json` emits one object on stdout: `{"protocol_version":1,"ok":true,"data":...}` or `{"protocol_version":1,"ok":false,"error":{"code":"...","message":"...","details":...}}`. Diagnostics use stderr in human mode. `--non-interactive` prevents invoking an editor; other commands already require explicit arguments rather than prompts. Lists accept `--offset` and `--limit` and return `items`, `total`, and `next_offset` where applicable.

Exit codes: 0 success, 2 usage, 3 validation, 4 revision/approval conflict, 5 unavailable database, 6 missing entity, 7 configuration, 130 cancellation, and 1 other IO/internal errors. An incomplete draft is a successful save with a nonempty `missing` field.

## Define schemas

A schema document has `node_types` and `relationship_types` arrays. See `schemas/examples/research.json` for a complete editable example. IDs are assigned on publication and preserved for stable keys. Keys use lowercase ASCII letters, digits, and underscores, begin with a letter, and contain at most 64 characters.

Node types define `key`, `name`, `display_property`, and an ordered `properties` array. Each property has a `key` and `type`; optional fields are `label`, `required`, `default`, `help`, `editor`, `choices`, `items`, `min`, `max`, `min_length`, `max_length`, and `pattern`. The display property must be a string property.

Property types are `string`, `integer`, `number`, `boolean`, `date`, `enum`, and `list`. Lists specify a scalar `items` type; enums specify string `choices`. Dates use `YYYY-MM-DD`. Integers are limited to the exact JSON/Lua range of ±9,007,199,254,740,991. Null or absent required values, blank required strings, and empty required lists are incomplete; supplied invalid values are errors. Defaults are applied on creation/migration, not silently reapplied when editing an existing field.

Relationships define `key`, `name`, `inverse_name`, `sources`, and `targets`, plus optional property definitions and graph rules. `family` groups relationship types for cardinality, cycles, and ordering. `max_incoming`, `max_outgoing`, `allow_duplicates`, `allow_self`, `acyclic`, and `ordered` control behavior. An ordered family requires acyclicity and at most one incoming parent. Members of a family must agree on its graph rules.

```sh
projman schema validate --file schema.json
projman schema preview --file schema.json
projman schema publish --file schema.json --expect-schema SCHEMA_REV --expect-graph GRAPH_REV
projman schema export
projman type list
projman type show investigation
```

A breaking publication needs `--retain-legacy` or prior migration of affected data. Preview identifies affected IDs. Compatible records move to the new schema revision; incompatible records retain their recorded definitions and must be explicitly migrated before editing. `schema migrate NODE_ID --file migration.json --expect-graph GRAPH_REV` accepts `{"properties":{...},"type_key":"optional_new_type"}`. Migrations preserve the body and validate incident edges; incompatible links must be handled through an explicit atomic batch.

## Nodes and Markdown

```sh
projman node create --type investigation --file node.json
projman node get NODE_ID
projman node get NODE_ID --document
projman node update NODE_ID --expect-revision NODE_REV --file patch.json
projman node edit NODE_ID --editor nvim
projman node archive NODE_ID --expect-revision NODE_REV
projman node archive NODE_ID --restore --expect-revision NODE_REV
```

Creation input is `{"properties":{"summary":"Investigate imports"},"body":"## Notes\n"}`. `--body-file notes.md` supplies Markdown separately. Patch input uses explicit operations: `{"set":{"priority":"high"},"unset":["estimate"],"body":"Replacement body"}`. Omitted fields are preserved; `unset` removes properties. Archive retains identity, history, and existing links, and excludes the node from default listings. New links to archived nodes are rejected.

The editor representation begins with `--- projman`, then one `key: value` line per property, followed by `---` and arbitrary Markdown. The schema determines how values are read: text, enum, and date fields accept unquoted text or JSON strings; numbers, booleans, and lists require JSON values. A blank value or `null` clears a field. Quote the literal word `"null"` and use JSON escaping for embedded newlines. The formatter emits canonical JSON values when opening a node; duplicate/unknown keys and invalid values are diagnosed at the property line. The Markdown body is preserved. `[Title](project://UUID)` adds a plain mention/backlink, with identity independent of the displayed title.

`node list`, `search TEXT`, and `backlinks NODE_ID` inspect planning content. `node list --relation TYPE --source NODE_ID` lists compatible destinations; add `--incoming` for the inverse direction. `node list --include-archived` includes archived records.

## Relationships and atomic changes

```sh
projman link suggest --source SOURCE_ID --target TARGET_ID
projman link add --source SOURCE_ID --target TARGET_ID --type contains --expect-graph GRAPH_REV
projman link list --node NODE_ID
projman link remove EDGE_ID --expect-graph GRAPH_REV
projman link reparent EDGE_ID --parent NEW_PARENT_ID --expect-graph GRAPH_REV
projman link reorder PARENT_ID --family breakdown --edges EDGE_3,EDGE_1,EDGE_2 --expect-graph GRAPH_REV
projman outline ROOT_ID --family breakdown
```

An add can include `--position` and `--file` containing `{"properties":{...}}`. Relationship properties must be complete. Reordering must name every sibling edge in that family exactly once. Reparenting preserves the edge ID and commits removal/addition together. Incoming views use inverse labels for the same edge.

All mutations recheck constraints inside a locked Neo4j transaction. For advanced workflows, `change validate --file request.json` previews a batch without persistence, and `change apply --file request.json` commits it atomically. A request is:

```json
{
  "operation_id": "5edb23ee-3947-47c4-bb5f-27463104848c",
  "expected_schema": 1,
  "expected_graph": 5,
  "expected_nodes": {"EXISTING_NODE_UUID": 2},
  "action": {
    "kind": "changes",
    "operations": [
      {"op": "update_node", "id": "EXISTING_NODE_UUID", "set": {"priority": "high"}}
    ]
  }
}
```

Replace the example operation ID for each new intent. Reuse the same ID and exact request after an unknown outcome. `operation OPERATION_UUID` returns the stored receipt, including revisions and previous values. A reused ID with a different request is a conflict. Standalone mutations also accept `--operation-id`.

Batch operations are `create_node`, `update_node`, `migrate_node`, `add_link`, `remove_link`, and `reorder`. Supply expected revisions for all existing affected nodes, including both endpoints and siblings whose ordering changes. Optional `expected_graph` guards the complete snapshot. `create_node`/`add_link` can include a UUID `id`; otherwise one is assigned. Use explicit IDs when later operations reference newly created nodes.

## Recovery

```sh
projman recovery list
projman recovery show RECOVERY_UUID
projman recovery export RECOVERY_UUID
projman recovery restore RECOVERY_UUID
projman recovery remove RECOVERY_UUID
```

Recovery files are private JSON files in `$XDG_STATE_HOME/projman/recovery`, or `~/.local/state/projman/recovery`. Listing and export work without a database. They retain text, base revisions, staged operations, and any exact unacknowledged request. Restore validates normally, never forces a revision overwrite, and removes the recovery only after an acknowledged success. A conflict leaves it available for explicit reconciliation.

`node edit` preserves its editing file and recovery ID on failure. Neovim also writes local snapshots if its host process is unavailable. `recovery save --file record.json` exposes the same facility for other clients. The exported format can be edited into a new recovery record after reviewing a conflict; clear `pending_mutation` and `operation_id` when intentionally changing the request.

## Manual proposals

The proposal engine is usable without an AI integration. A draft contains `id`, `base_schema`, `base_graph`, `expected_nodes`, ordered `groups`, and optional `questions`. Each group has `id`, `title`, `rationale`, `operations`, and optional `depends_on` naming preceding groups. Operations use the same format as atomic changes.

```sh
projman proposal submit --file proposal.json
projman proposal list
projman proposal show PROPOSAL_UUID
projman proposal validate PROPOSAL_UUID --groups create_output,attach_output
projman proposal apply PROPOSAL_UUID --digest REVIEWED_DIGEST --groups create_output,attach_output
projman proposal reject PROPOSAL_UUID --digest REVIEWED_DIGEST
```

Submission validates the complete proposal and assigns any missing creation IDs before computing its immutable digest. It stores the read context and schema. Review exposes text diffs and explicit graph changes. Application requires the reviewed digest and exact selected groups, including dependencies. Missing dependencies, stale schemas/graphs, and repeated acceptance under a new operation ID are rejected. A repeated identical operation returns its prior receipt. Partial acceptance closes that proposal; remaining suggestions need a new capture and review. Submission/rejection change proposal metadata only, not the domain graph revision.

## Editor host protocol

`projman serve --stdio` accepts newline-framed JSON messages:

```json
{"protocol_version":1,"id":1,"method":"node.get","params":{"id":"NODE_UUID"}}
```

Responses contain the same `id`, `protocol_version`, `ok`, and `data`/`error` fields as CLI output. `initialize` reports capabilities and workspace identity. Methods include `schema.validate`, `schema.preview`, `schema.export`, `type.list`, `type.show`, `node.new`, `node.get`, `node.document`, `node.list`, `document.parse`, `link.list`, `link.suggest`, `search`, `outline`, `backlinks`, `export`, `change.validate`, `change.apply`, `operation.get`, `proposal.list`, `proposal.show`, `proposal.validate`, and the `recovery.*` operations. Mutations use `change.apply` with the same request contract. Stdout is reserved for protocol messages; malformed/oversized input returns a structured error and does not terminate the host.

## Breadth-first explorer

The same traversal and root filtering used by Neovim are available through the CLI:

```sh
projman node pick 'title type or-id-fragment' --limit 50 --offset 0
projman explore ROOT_UUID
projman explore ROOT_UUID --direction both --relation relates --max-depth 4
projman explore ROOT_UUID --family references --include-archived
```

`node pick` matches every whitespace-separated query term against title, type name/key, and UUID, ignoring case, and returns a stable title/type/ID ordering with pagination. It excludes archived nodes by default. The existing `search` command continues to search Markdown bodies as well.

`explore` builds a read-only BFS spanning tree. JSON output contains nodes in discovery order, their depth and chosen parent, incoming traversal-edge metadata, and non-tree reference edges. Incoming traversal uses inverse relationship labels. Both-direction traversal emits each canonical edge once. Shared nodes and cycles do not duplicate subtrees or mutate the planning hierarchy.

Optional controls are `--direction outgoing|incoming|both`, `--relation KEY`, `--family FAMILY`, `--max-depth` (0–32; default 3), `--max-nodes` (1–5000; default 500), `--max-references` (0–5000; default 1000), and `--include-archived`. Truncation flags and each node's `more` field identify omitted neighbours. The stdio methods are `node.pick` and `explorer.tree`, using the same option names with underscores and `root` for the selected node UUID.
