# ProjMan Neo

A personal graph planning tool with a Rust core, a complete CLI, Neo4j persistence, and an optional Neovim client. Define your own node types, properties, and relationships. Project, Epic, Story, and Task are possible definitions, not built-in categories.

The initial implementation includes schema validation and migration, incomplete drafts, free Markdown, typed links, ordered outlines, backlinks, revision conflicts, recovery, and manually supplied proposals with explicit approval. Jira and Copilot integrations are deferred.

## Run locally

The implementation has been exercised with Rust 1.97.1, Neo4j Community 5.26.12, and Neovim 0.12.4. The pinned container binds only to localhost. Docker Compose is optional if you already have a compatible Neo4j instance.

```sh
git clone https://github.com/Tannex/proj-man.git
cd proj-man
cargo build --workspace --locked
export PATH="$PWD/target/debug:$PATH"
export PROJMAN_NEO4J_PASSWORD='choose-a-local-password'
docker compose -f infra/compose.yaml up -d
export PROJMAN_NEO4J_URI='bolt://127.0.0.1:17687'
export PROJMAN_WORKSPACE='personal'
projman doctor
projman workspace init
projman schema validate --file schemas/examples/research.json
projman schema publish --file schemas/examples/research.json --expect-schema 0 --expect-graph 0
```

Wait for Neo4j to become healthy before running the initialization commands. `workspace init` is repeatable. Schema publication uses revisions, so the final command above is for a new workspace.

Create a node from an example custom type:

```sh
projman node create --type investigation
projman node list
projman node edit NODE_ID --editor nvim
```

A missing required property is saved as an incomplete draft. Supplied invalid values reject the entire change. Every node also has a free Markdown body.

Use `--json` for a versioned response envelope and `--file` or `--stdin` for structured input. The CLI operates directly against the Rust core; no editor or application daemon is required.

## Neovim

For LazyVim, add a single plugin spec file containing `return dofile(vim.fn.expand("~/dev/proj-man/nvim/lazyvim.lua"))`. Restart Neovim, then use **`<leader>Pe`** for the BFS explorer and **`<leader>Pr`** for its filterable root picker. The spec discovers the built executable and registers which-key shortcuts. See [LazyVim setup](docs/neovim.md#lazyvim-setup) for connection options.

Add the `nvim` directory to your runtime path, or configure your plugin manager to load that subdirectory. With a local checkout:

```lua
vim.opt.runtimepath:append('/absolute/path/to/proj-man/nvim')
require('projman').setup({}) -- Finds the built executable automatically
```

The child process inherits the same `PROJMAN_*` environment variables as the CLI. Commands include `:ProjManExplore`, `:ProjManFind`, `:ProjManTypes`, `:ProjManNew`, `:ProjManSearch`, `:ProjManLink`, `:ProjManLinks`, `:ProjManOutline`, and `:ProjManBacklinks`. `<leader>Pn` guides node creation and `<leader>Pv` edits properties through typed controls. Tab and Shift-Tab also navigate raw property values; ordinary editor mappings work in the Markdown body. `:write` is asynchronous and preserves edits typed while a save is pending.

See [the CLI and data contract](docs/cli.md), [the Neovim guide](docs/neovim.md), and [the implementation plan](docs/implementation-plan.md), and [verification evidence](docs/verification.md).

## Test

Run compiler, style, and domain checks:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The acceptance suites require a real Neo4j instance. They create unique test workspaces and do not alter existing workspaces. Start an isolated disposable instance:

```sh
docker run --detach --rm --name projman-neo4j-test \
  --publish 127.0.0.1:17688:7687 --publish 127.0.0.1:17475:7474 \
  --env NEO4J_AUTH=none \
  --env NEO4J_server_memory_heap_initial__size=256m \
  --env NEO4J_server_memory_heap_max__size=512m \
  --env NEO4J_server_memory_pagecache_size=256m \
  neo4j:5.26.12-community@sha256:9f75e8df4325a24f00fdd7a8c0bcce650a58375049b1058e496e8b43d6c36b37
```

Once ready:

```sh
export PROJMAN_NEO4J_URI='bolt://127.0.0.1:17688'
cargo build --workspace --locked
python3 tests/cli_integration.py
python3 tests/nvim_integration.py
docker stop projman-neo4j-test
```

The test container has no authentication and exposes only localhost. It is removed when stopped. The development Compose configuration instead uses a password and a persistent named volume.

## Initial implementation boundaries

Neo4j is authoritative. Local recovery files are editing snapshots, not an offline synchronization database. The adapter loads a workspace snapshot and serializes mutations with a workspace lock to enforce invariants across independent CLI/editor processes. This favors correctness and simplicity for personal graphs; it is not designed as a shared high-throughput service.

Application UUIDs survive renaming. Node/edge records and immutable schema revisions are stored in Neo4j, with native graph relationships and derived scalar property projections. Edit through the application: direct Cypher changes bypass its invariants.
