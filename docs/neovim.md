# Neovim client

The plugin drives the same Rust application operations as the CLI. It requires an initialized workspace and a `projman` executable; use the README setup first. Jira and Copilot commands are not part of this initial version.

```lua
vim.opt.runtimepath:append('/absolute/path/to/proj-man/nvim')
require('projman').setup({
  workspace = 'personal',
  timeout = 10000,
  recovery_delay = 300,
  map_tab = true,
})
```

If you add the runtime path after startup, run `:runtime plugin/projman.lua` once to register commands. The child process inherits connection credentials from the environment. You can include normal CLI connection flags in `command`.

## Editing

| Command | Action |
| --- | --- |
| `:ProjManTypes` | Edit the workspace schema as JSON |
| `:ProjManTypeNew` | Add an editable custom node type definition |
| `:ProjManRelationTypeNew` | Add an editable relationship definition |
| `:ProjManNew [type_key]` | Create a node from the selected custom type |
| `:ProjManOpen UUID` | Open a saved node |
| `:ProjManSearch [text]` | Search titles and Markdown |
| `:write` | Validate and save the current node, including staged relationships |
| `:ProjManNextField` / `:ProjManPreviousField` | Navigate properties explicitly |
| `:ProjManBack` | Return to the previous buffer |

Node buffers are named `project://UUID`. The property header follows schema order; required fields are marked and missing values produce diagnostics. Values use JSON syntax: `"text"`, `3`, `true`, `["a", "b"]`, or `null` for an empty value. Everything after the header terminator is free Markdown. Property keys are stable identifiers; display labels and help belong to the schema.

Tab and Shift-Tab move between property values. Completion menus and native snippet navigation take precedence. Outside property values, the existing insert-mode mapping is used. Set `map_tab = false` to keep full control of these keys and use the explicit field commands. `<C-x><C-o>` offers schema enum values. `gf` follows a stable `project://` Markdown mention; elsewhere it uses ordinary file navigation.

Saving is asynchronous. If you type during a save, the acknowledged snapshot becomes the saved base and the newer text stays modified. A failed save leaves your work in the buffer and recovery files. `:wq` may refuse to quit while the asynchronous write completes; then use `:quit` after the buffer becomes unmodified. Forced quitting preserves a local recovery snapshot.

## Graph navigation and staged changes

| Command | Action |
| --- | --- |
| `:ProjManLink [relationship_key]` | Pick a destination and compatible relationship; supplying a relationship filters destinations first |
| `:ProjManUnstage` | Clear staged relationship changes while preserving node text |
| `:ProjManLinks` | Open generated relationship rows; Enter follows a node and `dd` stages edge removal |
| `:ProjManReparent EDGE_UUID PARENT_UUID` | Stage a local outline move |
| `:ProjManReorder FAMILY EDGE_UUID...` | Stage a complete sibling ordering for the current parent |
| `:ProjManOutline [family]` | Navigate the selected custom outline family |
| `:ProjManBacklinks` | View inverse typed links and plain mentions |

Relationship operations are staged with the node buffer and committed by its next `:write`. They use the same endpoint, duplicate, cardinality, and cycle checks as CLI changes. Node IDs, not displayed titles, back navigation. A relationship's inverse label represents the same edge; no duplicate inverse edge is created.

## Conflicts and recovery

`:ProjManConflict` opens the saved version beside your preserved buffer in a diff view. After comparing them, `:ProjManReconcile` can explicitly rebase your edited text on that saved node revision; the next write revalidates everything. Changed schemas and conflicting relationship revisions still require explicit reconciliation or migration. No forced overwrite is available.

`:ProjManRetry` resends an unresolved save's exact original request and operation ID. A committed receipt prevents duplicate application if the earlier acknowledgement was lost. Editing the buffer does not change the request being retried; later edits remain modified after its acknowledgement.

`:ProjManRecover` lists local recoveries and attempts restoration with revision checks. You can also inspect/export them from the CLI. The plugin writes private recovery snapshots locally even when the Rust host is unavailable. Records for unacknowledged saves are separate from the latest editing snapshot, so later typing cannot replace the exact pending request.

## Manual proposals

`:ProjManProposals` lists stored proposals, and `:ProjManReview UUID` shows rationale, questions, text diffs, and graph changes. In a review, `a` selects groups, validates that selection, displays its changes, and asks for explicit application. `r` rejects the proposal. Missing dependencies and stale proposals cannot be applied.

The engine also works entirely through the CLI. No AI is run by these commands.

## BFS explorer and root picker

`:ProjManExplore` opens a filterable root picker. Type words from a node's title, custom type name/key, or ID; all terms must match, ignoring case. Use Up/Down or Ctrl-N/P to select, Enter to choose, and Escape to cancel. Ctrl-F/B (or PageDown/PageUp) pages through all matches, including nodes beyond the initial 50 results. Ctrl-R retries a failed search. `:ProjManExplore UUID` opens a known root directly, and `:ProjManFind` uses the same picker to open a node without changing the explorer root.

The explorer stays in a left sidebar while nodes open in the editing window. It builds a breadth-first spanning tree over saved typed relationships (Markdown mentions remain available through backlinks): each node has one displayed parent at its shortest distance from the root. Additional edges, shared descendants, and cycles appear as reference rows marked `↪`. This is a navigation view; choosing a root or expanding a branch never changes stored parent relationships. Ties are resolved deterministically by relationship key, position, title, and IDs.

| Explorer key | Action |
| --- | --- |
| Enter | Open the selected node in the editing window |
| Tab / `za` | Expand or collapse a branch (Space also works when it is not your leader) |
| `l` / Right | Expand; fetch a deeper/larger bounded result when needed |
| `h` / Left | Collapse, or move to the displayed parent |
| `s` or `/` | Filter and choose another root |
| `R` | Make the selected node the root |
| `r` | Refresh from Neo4j |
| `d` | Cycle outgoing, incoming, and both directions |
| `t` | Filter by relationship type or family |
| `a` | Include or hide archived nodes |
| `+` / `-` | Increase or decrease traversal depth |
| `gp` | Jump from a reference to the node's primary occurrence |
| `gg` | Go to the root row |
| `?` | Show key help |
| `q` | Close the sidebar |

Defaults follow outgoing relationships, exclude archived nodes, and fetch at most three hops, 500 nodes, and 1,000 reference edges. The header identifies limits; `…` marks branches with unexplored neighbours. Expand a boundary to increase the applicable limit, choose a closer root, or narrow the relationship filter. Limits prevent cycles or large graphs from creating unbounded views. Successful node/schema/proposal writes in this Neovim session refresh open explorers. Press `r` to see changes made through another client.

Configure the defaults with `setup({ explorer = { width = 44, direction = "outgoing", max_depth = 3, max_nodes = 500, max_references = 1000, include_archived = false } })`. `:ProjManExplorerRoot` opens the root picker and `:ProjManExplorerRefresh` refreshes the current explorer.

## LazyVim setup

The repository includes `nvim/lazyvim.lua`, a complete local plugin spec. In `~/.config/nvim/lua/plugins/projman.lua`, use:

```lua
return dofile(vim.fn.expand('~/dev/proj-man/nvim/lazyvim.lua'))
```

Adjust only the checkout path, then restart Neovim. This follows LazyVim's `lua/plugins/*.lua` convention and uses lazy.nvim's local `dir`, command/key loading, and `opts` setup. [LazyVim plugin configuration](https://www.lazyvim.org/configuration/plugins), [lazy.nvim spec](https://lazy.folke.io/spec)

The spec discovers the most recently built local release/debug executable, falling back to `projman` on PATH. `:Lazy build projman` builds the Rust CLI in release mode. No manual runtime-path or executable-path setup is needed. The root picker is native and does not require Telescope or Snacks.

The uppercase `<leader>P` namespace avoids LazyVim's optional lowercase paste mappings. Which-key shows the ProjMan group; Edgy integration is added only if that optional plugin is already enabled.

| Shortcut | Action |
| --- | --- |
| `<leader>Pe` | Toggle explorer / open root picker |
| `<leader>Pr` | Choose BFS root |
| `<leader>Pf` | Find/open node |
| `<leader>Pn` | New node |
| `<leader>Pl` / `<leader>PL` | Add link / view relationships |
| `<leader>Pb` | Backlinks |
| `<leader>Pt` | Edit custom types |
| `<leader>Po` | Planning outline |
| `<leader>Pp` | Review proposals |
| `<leader>Pc` | Compare a save conflict |
| `<leader>PR` | Recover edits |
| `<leader>P]` / `<leader>P[` | Next/previous property |
| `<leader>P?` | Connection and executable health |

Property Tab navigation defers to an active Blink completion menu or active native/Blink/LuaSnip snippet. Existing body mappings are preserved.

Neo4j still needs to be running and the workspace initialized as described in the README. Neovim inherits the CLI's `PROJMAN_*` environment variables. To set a workspace or connection specifically for Neovim, extend the one-file spec:

```lua
local spec = dofile(vim.fn.expand('~/dev/proj-man/nvim/lazyvim.lua'))
spec[1].opts = {
  workspace = 'personal',
  -- env = { PROJMAN_NEO4J_URI = 'bolt://127.0.0.1:17687' },
  explorer = { max_depth = 4 },
}
return spec
```

`:ProjManHealth` or `:checkhealth projman` checks executable discovery and database connectivity. The plugin does not start containers or change your graph during editor startup.
