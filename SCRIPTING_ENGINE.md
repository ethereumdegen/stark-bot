# Plugin SDK with Lifecycle Hooks — Implementation Plan

## Context

StarkBot already has 80% of the hook infrastructure built (`Hook` trait, `HookManager`, 16 `HookEvent` variants, `HookContext`, `HookResult`, `HookPriority`, `HookStats`, plus two built-in hooks: `LoggingHook` and `RateLimitHook`). But only **one** hook event (`AfterToolCall`) is actually wired in the dispatcher, and there's no way to bundle tools + hooks + validators into a single extension unit. This plan turns that foundation into a real plugin system.

**Goal**: A `Plugin` trait that bundles hooks, tools, and validators — loadable as compiled Rust or runtime Rhai scripts from a `plugins/` directory.

---

## Phase A — Core Types & DB Schema

### New files

**`src/plugins/mod.rs`** — Module root
```rust
pub mod types;
pub mod traits;
pub mod registry;
pub mod loader;
pub mod builtin;
pub mod script;
```

**`src/plugins/types.rs`** — `PluginManifest`, `PluginSource`, `DbPlugin`, `PluginStats`
- `PluginManifest` { id, name, version, description, author, tags, hook_events, provides_tools, provides_validators, default_config, source }
- `PluginSource` enum: `Builtin`, `Script`, `Managed`
- `DbPlugin` struct for SQLite persistence
- `PluginStats` { hook_executions, tool_executions, errors, avg_latency_ms }

**`src/plugins/traits.rs`** — The core `Plugin` trait
```rust
#[async_trait]
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;
    async fn on_load(&self, config: &Value) -> Result<(), String> { Ok(()) }
    async fn on_unload(&self) -> Result<(), String> { Ok(()) }
    async fn on_config_change(&self, config: &Value) -> Result<(), String> { Ok(()) }
    fn hooks(&self) -> Vec<Arc<dyn Hook>> { vec![] }
    fn tools(&self) -> Vec<Arc<dyn Tool>> { vec![] }
    fn validators(&self) -> Vec<Arc<dyn ToolValidator>> { vec![] }
}
```

Design: Plugin is a **factory** — it returns hooks/tools/validators, not implements them. Each concern stays dyn-safe.

**`src/db/tables/plugins.rs`** — CRUD for `plugins` table

### Modified files

- **`src/db/sqlite.rs`** — Add `plugins` table to `init()`:
  ```sql
  CREATE TABLE IF NOT EXISTS plugins (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      plugin_id TEXT UNIQUE NOT NULL,
      name TEXT NOT NULL,
      version TEXT NOT NULL DEFAULT '1.0.0',
      description TEXT NOT NULL DEFAULT '',
      source TEXT NOT NULL DEFAULT 'builtin',
      enabled INTEGER NOT NULL DEFAULT 1,
      config TEXT NOT NULL DEFAULT '{}',
      manifest TEXT NOT NULL DEFAULT '{}',
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
  )
  ```
- **`src/db/tables/mod.rs`** — Add `mod plugins;`
- **`src/main.rs`** — Add `mod plugins;`

---

## Phase B — Plugin Registry & Loader

### New files

**`src/plugins/registry.rs`** — `PluginRegistry`

Follows `SkillRegistry` pattern (DB-backed) + `HookManager` pattern (DashMap):
- `new(db)` — constructor
- `load_plugin(plugin, hook_manager, tool_registry, validator_registry)` — register everything a plugin provides, call `on_load()`, persist to DB
- `unload_plugin(id, hook_manager)` — unregister hooks, call `on_unload()`
- `set_enabled(id, enabled)` / `set_config(id, config)` — DB persistence
- `list()` / `get(id)` / `get_stats(id)` / `len()`

Hook ID convention: `"plugin.{plugin_id}.{hook_id}"` — makes unregistration clean.

**`src/plugins/loader.rs`** — TOML manifest parsing + directory scanner
- `parse_manifest(content: &str) -> Result<PluginManifest>`
- `discover_plugins(dir: &Path) -> Vec<(PluginManifest, PathBuf)>`
- `load_all_plugins(dir, registry, hook_manager, tool_registry, validator_registry)`

Each plugin lives in a subdirectory with `PLUGIN.toml`:
```toml
[plugin]
id = "cost-tracker"
name = "Cost Tracker"
version = "1.0.0"
description = "Track AI API costs per session"
author = "starkbot"
tags = ["analytics"]

[hooks]
events = ["after_tool_call", "after_agent_end"]

[config]
max_budget_usd = 10.0
```

### Modified files

- **`Cargo.toml`** — Add `toml = "0.8"`
- **`src/config.rs`** — Add `PLUGINS_DIR` env var constant + `plugins_dir()` helper

---

## Phase C — Wire All Hook Events into Dispatcher

**This is the highest-value change.** Currently only `AfterToolCall` fires (dispatcher.rs line 2108). Wire the remaining 6 critical events:

### `src/channels/dispatcher.rs` modifications

**1. `BeforeToolCall`** — Insert in `process_tool_call_result()` at ~line 1736, BEFORE the validator check:
```rust
if let Some(hook_manager) = &self.hook_manager {
    let mut ctx = HookContext::new(HookEvent::BeforeToolCall)
        .with_channel(original_message.channel_id, Some(session_id))
        .with_tool(tool_name.to_string(), tool_arguments.clone());
    match hook_manager.execute(HookEvent::BeforeToolCall, &mut ctx).await {
        HookResult::Skip => return ToolCallProcessed {
            result_content: "Skipped by hook".into(), success: false, ..Default::default()
        },
        HookResult::Cancel(msg) => return ToolCallProcessed {
            result_content: msg, success: false, ..Default::default()
        },
        _ => {}
    }
}
```

**2. `BeforeAgentStart`** — Insert in `dispatch()` after execution tracking starts, before the AI call. If Cancel, return early with error DispatchResult.

**3. `AfterAgentEnd`** — Insert at end of `finalize_tool_loop()` (~line 2280), before the return.

**4. `BeforeResponse`** — Insert in `finalize_tool_loop()` before the response text is finalized. If `Replace(value)`, use the replacement text.

**5. `OnError`** — Create helper method and call at key error paths:
```rust
async fn fire_on_error(&self, channel_id: i64, session_id: Option<i64>, error: &str) {
    if let Some(hook_manager) = &self.hook_manager {
        let mut ctx = HookContext::new(HookEvent::OnError)
            .with_channel(channel_id, session_id.unwrap_or(0))
            .with_error(error.to_string());
        let _ = hook_manager.execute(HookEvent::OnError, &mut ctx).await;
    }
}
```

**6. `SessionStart` / `SessionEnd`** — Fire when session is created (in `dispatch()`) and when `task_fully_completed` is set (in `finalize_tool_loop()`).

---

## Phase D — Bootstrap Integration

### `src/main.rs` modifications

**Key refactor**: Delay `Arc::new()` wrapping of registries so plugins can register during bootstrap:

```rust
// BEFORE (current, lines 650-753):
let tool_registry = Arc::new(tools::create_default_registry());           // line 650
let validator_registry = Arc::new(tool_validators::create_default_registry()); // line 752

// AFTER:
let mut tool_registry = tools::create_default_registry();
let mut validator_registry = tool_validators::create_default_registry();

// Load plugins (while registries are still mutable)
let plugin_registry = plugins::PluginRegistry::new(db.clone());
plugins::loader::load_all_plugins(
    &config::plugins_dir().into(),
    &plugin_registry, &hook_manager, &mut tool_registry, &mut validator_registry,
).await;
log::info!("Loaded {} plugins", plugin_registry.len());
let plugin_registry = Arc::new(plugin_registry);

// NOW wrap in Arc
let tool_registry = Arc::new(tool_registry);
let validator_registry = Arc::new(validator_registry);
```

Add `plugin_registry: Arc<PluginRegistry>` to `AppState`.

---

## Phase E — REST API

### New file: `src/controllers/plugins.rs`

Following `controllers/skills.rs` pattern:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/plugins` | GET | List all plugins with status |
| `/api/plugins/{id}` | GET | Plugin detail (manifest + config + stats) |
| `/api/plugins/{id}/enabled` | PUT | Enable/disable (`{ "enabled": true }`) |
| `/api/plugins/{id}/config` | GET | Current config |
| `/api/plugins/{id}/config` | PUT | Update config (fires `on_config_change`) |
| `/api/plugins/{id}/stats` | GET | Execution statistics |
| `/api/plugins/reload` | POST | Reload all from disk |

### Modified files
- **`src/controllers/mod.rs`** — Add `pub mod plugins;`
- **`src/main.rs`** — Add `.configure(controllers::plugins::config)` to route setup

---

## Phase F — Rhai Script Engine

### New files

**`src/plugins/script/mod.rs`** — module root

**`src/plugins/script/engine.rs`** — Rhai engine with sandboxing:
- `PluginEngine::new()` — create engine with limits (max_operations=100K, max_expr_depth=64, max_string_size=1MB)
- Register host functions: `log_info()`, `log_warn()`, `config()`, `state_get()`, `state_set()`
- Register `HookContextWrapper` type with getters: `event`, `tool_name`, `tool_args`, `channel_id`, `session_id`

**`src/plugins/script/script_hook.rs`** — `ScriptHook` implements `Hook` trait:
- Wraps a compiled Rhai AST
- `execute()` calls the script's `on_hook(ctx)` function
- Maps Rhai return values to `HookResult` (string `"continue"` → Continue, `"skip"` → Skip, object with `"cancel"` key → Cancel)

**`src/plugins/script/script_plugin.rs`** — `ScriptPlugin` implements `Plugin` trait:
- `from_directory(path)` — parse PLUGIN.toml + compile `*.rhai` files
- Returns `ScriptHook` instances from `hooks()`

Example script plugin (`plugins/example-script/hook.rhai`):
```rhai
fn on_hook(ctx) {
    if ctx.event == "after_tool_call" {
        log_info("[Script] Tool " + ctx.tool_name + " completed");
    }
    "continue"
}
```

### Modified files
- **`Cargo.toml`** — Add `rhai = { version = "1.17", features = ["sync"] }`

The `sync` feature makes Rhai's `Dynamic` type `Send + Sync`, required for our async context.

### Why Rhai?

- Pure Rust — no C dependencies, builds cleanly with cargo
- Sandboxed by default — no filesystem/network access unless explicitly provided
- Supports `Send + Sync` (via `sync` feature) — safe for our tokio + DashMap architecture
- Fast for a scripting language — compiled to AST, not interpreted line-by-line
- Small footprint — ~500KB added to binary

---

## Phase G — Built-in Example Plugins

### `src/plugins/builtin/mod.rs` + `analytics.rs`

A practical built-in plugin that:
- Subscribes to `AfterToolCall` and `AfterAgentEnd`
- Tracks tool call counts, success rates, and latency per session
- Provides a `plugin_analytics_report` tool that returns usage stats
- Demonstrates the full Plugin pattern: hooks + tools + config

### `plugins/example-script/PLUGIN.toml` + `hook.rhai`

A minimal Rhai script plugin for reference.

---

## File Summary

### New files (14)
| Path | Purpose |
|------|---------|
| `src/plugins/mod.rs` | Module root |
| `src/plugins/types.rs` | PluginManifest, DbPlugin, PluginSource, PluginStats |
| `src/plugins/traits.rs` | Plugin trait |
| `src/plugins/registry.rs` | PluginRegistry |
| `src/plugins/loader.rs` | TOML manifest parsing, directory loader |
| `src/plugins/builtin/mod.rs` | Built-in plugin re-exports |
| `src/plugins/builtin/analytics.rs` | Example analytics plugin |
| `src/plugins/script/mod.rs` | Script engine module root |
| `src/plugins/script/engine.rs` | Rhai engine wrapper |
| `src/plugins/script/script_hook.rs` | ScriptHook (Rhai to Hook bridge) |
| `src/plugins/script/script_plugin.rs` | ScriptPlugin (directory to Plugin) |
| `src/controllers/plugins.rs` | REST API endpoints |
| `src/db/tables/plugins.rs` | DB CRUD |
| `plugins/example-script/` | PLUGIN.toml + hook.rhai |

### Modified files (8)
| Path | Change |
|------|--------|
| `Cargo.toml` | Add `toml`, `rhai` deps |
| `src/main.rs` | `mod plugins`, bootstrap, AppState, route |
| `src/channels/dispatcher.rs` | Wire 6 missing hook events |
| `src/db/sqlite.rs` | `plugins` table in `init()` |
| `src/db/tables/mod.rs` | `mod plugins` |
| `src/controllers/mod.rs` | `pub mod plugins` |
| `src/config.rs` | `PLUGINS_DIR` + `plugins_dir()` |
| `src/tools/registry.rs` | (Optional) `HashMap` to `DashMap` for runtime registration |

---

## Build Order

Phases A + C are independent and can be done in parallel.

```
A (types, DB)  ───> B (registry, loader) ───> D (main.rs) ───> E (API) ───> G (examples)
                                                ↑
C (wire hooks) ─────────────────────────────────┘
                                                ↓
                                           F (Rhai scripts)
```

**Recommended order**: C → A → B → D → G → E → F

Start with **C** (hook wiring) because it delivers value immediately with zero new dependencies — the existing `LoggingHook` and `RateLimitHook` will start actually firing for all events.

---

## Verification

1. `cargo build` compiles clean after each phase
2. `cargo test` — existing dispatcher tests still pass
3. New unit tests in each `plugins/*.rs` module
4. Integration test: load HelloWorldPlugin, verify hooks fire during MockAiClient dispatch
5. API test: `curl /api/plugins` returns loaded plugins
6. Script test: place `plugins/test/PLUGIN.toml` + `hook.rhai`, verify it loads and executes
