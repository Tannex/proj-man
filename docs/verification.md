# Initial build verification

The initial build covers the Rust core, CLI, Neo4j persistence, optional Neovim client, and manual proposal review. Jira and Copilot integrations are deferred as requested. The tests ran against a real isolated Neo4j Community 5.26.12 container, with Rust 1.97.1 and Neovim 0.12.4.

## Recorded results

| Check | Result |
| --- | --- |
| `cargo build --workspace --locked` | Passed; executable at `target/debug/projman` |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed without warnings |
| `cargo test --workspace --locked` | Passed: 7 domain tests and 1 transport test |
| `python3 tests/cli_integration.py` with the test Neo4j URI | Passed: 14 acceptance tests |
| `python3 tests/nvim_integration.py` with the test Neo4j URI | Passed: main editor workflow, pending quit, host failure, and timeout scenarios |
| Docker Compose configuration validation | Passed with Compose 5.3.1 |
| Direct Cypher inspection | Confirmed persisted `PMNode` nodes and native `PM_LINK` relationships |

Local command output is saved in `.local/verification/`. That directory is excluded from version control. The test sources and reproduction commands are part of the project.

## Requirement evidence

| Requirement | Evidence |
| --- | --- |
| Rust core with a complete CLI | Three Cargo crates; the CLI acceptance suite runs schema/graph/recovery/proposal workflows without Neovim |
| Custom types and required properties | Domain tests and CLI cases 01, 10, and 99 use arbitrary type names, distinct property sets, defaults, supported scalar/list kinds, and schema migration |
| Stable type identity and versioned schemas | Domain test `type_identities_survive_retirement_and_return`; real schema preview, retention, and migration in CLI case 99 |
| Incomplete drafts and supplied-value validation | CLI cases 01 and 10; required facts can be missing while invalid enums, dates, ranges, patterns, lists, and malformed input are rejected |
| Free text and reliable parsing | Unicode/escaping/body round-trip domain test; real CLI and Neovim body saves |
| Required-field order and Tab navigation | Headless Neovim uses actual Tab/Shift-Tab mappings, schema enum completion, and an existing body mapping |
| Typed links, inverse navigation, and stable references | Domain graph tests; CLI cases 03 and 12; Neovim staged links, relationship rows, outline navigation, and reparenting |
| Ordered hierarchy and graph invariants | CLI case 09 checks ordering/reparenting; case 11 races competing parents; case 04 races disjoint writes that would jointly create a cycle |
| Optimistic revision handling | CLI case 05 races saves; editor integration preserves a conflicting buffer after another writer commits |
| Atomic database writes and rollback | Case 13 inserts one valid node before a second insert hits a real Neo4j uniqueness constraint, then verifies the first node, mutation receipt, and revision change were rolled back |
| Repeatable mutations | CLI cases 02 and 09 replay requests, including replay after later edge removal; editor timeout scenario confirms a lost response retry advances the node revision only once |
| Recovery during failures | CLI case 07 lists recoveries without a database and rejects stale restores; Neovim host-failure and timeout scenarios preserve text and exact pending requests |
| Nonblocking editor writes | Headless editor types while a save is pending and verifies the newer text stays dirty; the quit scenario verifies an unacknowledged save is not lost |
| Schema authoring from the editor | Headless test edits and publishes the schema through a managed JSON buffer and reads the updated custom type back |
| Explicit proposal approval | CLI case 08 checks no domain mutation on submission/rejection, digest matching, missing dependencies, partial acceptance, stale proposals, and repeated acceptance; Neovim exercises the explicit review/apply UI |
| CLI and editor use the same application rules | CLI case 06 checks protocol parity; all editor persistence uses the same core mutations and Neo4j adapter |
| Usable setup and contracts | README, CLI/data contract, Neovim guide, example schema, pinned Compose configuration, and executable command help |

## Practical boundaries

This is a personal-use implementation with a workspace transaction lock and snapshot-based validation. High-throughput collaboration, automatic offline synchronization, a graphical canvas, generalized graph undo, Jira publication, and Copilot execution are outside this initial build. Validation used the pinned local Neo4j version; remote TLS deployments and other Neo4j/Neovim releases have not been exercised.

The initial build was verified before Git was initialized. The build and tests do not depend on Git metadata. The initial build did not modify user Neovim configuration. The later LazyVim integration described below installs a small loader file.


## BFS explorer and LazyVim follow-up

The explorer adds four Rust tests (12 total across the workspace), a native Neovim UI suite, and real Neo4j explorer acceptance checks. The original 14 CLI acceptance tests and editor regression scenarios also pass.

- `crates/projman-core/tests/explorer.rs` proves shortest-parent BFS discovery, deterministic traversal, inverse directions, canonical references, cycles/self-links/parallel edges, bounded results, archived filtering, and paginated title/type/ID matching.
- `tests/nvim_explorer.lua` checks live filtering, paging, late responses, cancellation/empty states, folding, reference navigation, Space-leader compatibility, unsaved ordinary buffers, refresh failures, root changes, automatic refresh, and cleanup.
- `tests/explorer_integration.py` checks CLI/RPC/Neovim behavior on a real graph and a picker dataset larger than one page. It also confirms that exploration leaves the saved graph unchanged and a delayed node-open response does not replace the sidebar or discard unsaved edits.
- `tests/lazyvim_integration.py` uses the installed lazy.nvim and which-key packages with temporary configuration/state. It checks command/key lazy loading, complete command coverage, executable discovery, and the ProjMan shortcut group. `tests/nvim_lazyvim_build.lua` additionally exercises the actual release build hook.

The LazyVim loader was installed at `~/.config/nvim/lua/plugins/projman.lua` on the development machine; it loads the repository's `nvim/lazyvim.lua` spec. Existing configuration files were preserved. See the Neovim guide to install the loader for your own checkout. The integration still requires the normal Neo4j connection/workspace setup.

Reproduce the additional checks after building the CLI:

```sh
PROJMAN_TEST_ROOT="$PWD" nvim --headless -u NONE -l tests/nvim_explorer.lua
PROJMAN_NEO4J_URI=bolt://127.0.0.1:17688 python3 tests/explorer_integration.py
python3 tests/lazyvim_integration.py
PROJMAN_TEST_ROOT="$PWD" nvim --headless -u NONE -l tests/nvim_lazyvim_build.lua
```

The database check uses the isolated Neo4j instance from the README. The LazyVim check uses locally installed lazy.nvim and which-key; set `PROJMAN_TEST_LAZY_ROOT` if those packages live outside the usual directory. No plugin downloads are performed by the check.


## Neovim write input regression

A real `:write` reproduction with `name: Updated task` produced `name: expected value at line 1 column 1` and left the stored value unchanged. The editor parser now uses the node schema to accept plain text in text, enum, and date fields; structured fields retain JSON parsing and normal validation. Blank values remain incomplete draft facts. The formatter and storage keep canonical typed values.

Three new Rust tests in `crates/projman-core/tests/editor_input.rs` cover plain text, quoted literals, blank fields, preserved Markdown, and exact diagnostics for malformed/invalid typed values. `tests/nvim_save_integration.py` drives the actual `:write` command in standalone Neovim and lazy.nvim, then checks persistence through a separate CLI process. It also covers new nodes, incomplete drafts, recovery, and CLI editor input. The client now shows pending, confirmed, failed, and unconfirmed save outcomes without clearing newer edits.

```sh
PROJMAN_NEO4J_URI=bolt://127.0.0.1:17688 \
PROJMAN_TEST_LAZY_ROOT="$HOME/.local/share/nvim/lazy" \
python3 tests/nvim_save_integration.py
```

The complete Rust suite (15 tests), existing CLI suite (14 tests), and Neovim save/recovery/timeout regressions passed after this change.
