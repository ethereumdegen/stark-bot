# StarkBot Improvements — Inspired by OpenClaw Audit

> Audit date: 2026-02-11
> Compared: **starkbot-monorepo** (Rust/Actix, ~25K LOC) vs **openclaw** (TypeScript/Node, ~336K LOC)

---

## 1. Plugin SDK with Lifecycle Hooks

**What OpenClaw does:** Ships a formal `plugin-sdk` with typed interfaces for channel plugins, provider plugins, tool definitions, and gateway request handlers. Plugins are loaded at runtime via `jiti` (TypeScript transpiler) and can intercept core behavior through a hook system (`beforeAgentRun`, `beforeToolCall`, `afterToolCall`, `onAgentEvent`, `onChannelEvent`). This means third-party code can modify tool execution, inject pre/post-processing, and add entirely new channels — without touching core.

**What StarkBot has today:** Tools are registered via `create_default_registry()` and channels are hardcoded in `ChannelManager`. The `hooks` table exists in SQLite but is limited to lifecycle events, not true extension points. Adding a new channel or tool requires modifying core source files and recompiling.

**Proposed upgrade:**

- Define a `Plugin` trait in Rust with methods: `on_load()`, `before_tool_call()`, `after_tool_call()`, `on_message_received()`, `on_response_sent()`.
- Load plugins from a `plugins/` directory as dynamic libraries (`.so`/`.dylib` via `libloading`) or as WASM modules (via `wasmtime` — safer sandboxing).
- Each plugin declares a manifest (TOML) with name, version, hooks it subscribes to, and config schema.
- The dispatcher calls hook points at the appropriate stages, passing a mutable context struct. Plugins can modify tool parameters, inject additional context, block execution, or log telemetry.
- **Quick win:** Start with just `before_tool_call` and `after_tool_call` hooks — these cover 80% of extension use cases (audit logging, parameter rewriting, rate limiting, cost tracking).

**Impact:** Unlocks community contributions, custom integrations, and per-deployment customization without maintaining forks.

---

## 2. Model Failover Chain with Auth Profile Rotation

**What OpenClaw does:** Each agent can define a `model.fallbacks` list. When the primary model fails (auth error, rate limit, billing issue, context overflow), the system automatically rotates to the next model in the chain. Additionally, OpenClaw supports multiple API keys per provider via "auth profiles" — if one key hits a rate limit, it cools down that key and rotates to the next, with ranking and exponential backoff. Context overflow triggers automatic compaction then retry.

**What StarkBot has today:** A single `ModelArchetype` per agent selected at configuration time. If the Claude API returns a 429 or 500, the request fails and the user sees an error. There's no automatic fallback to OpenAI or Kimi, and no support for multiple API keys per provider.

**Proposed upgrade:**

- Add a `fallback_models` field to `AgentSettings` — an ordered list of `(provider, model_id, api_key_ref)` tuples.
- In the dispatcher's `generate_with_native_tools_orchestrated()` and `generate_with_text_tools_orchestrated()`, wrap the API call in a retry loop that catches:
  - **429 Rate Limit** → rotate to next model/key, mark current as cooling down.
  - **401/403 Auth Error** → skip to next key for same provider, or next provider.
  - **Context Overflow** → trigger compaction, retry with same model.
  - **5xx Server Error** → retry with backoff, then rotate.
- Store API key health in an `ApiKeyHealth` struct: `{ last_failure: Instant, cooldown_until: Instant, success_count: u64, failure_count: u64 }`.
- Add a `starkbot_api_keys` table to support multiple keys per provider with priority ordering.
- Surface failover events through the WebSocket gateway so the frontend can show "Switched to GPT-4o (Claude rate-limited)".

**Impact:** Dramatically improves uptime. Users stop seeing errors for transient API issues. Multi-key support enables higher throughput for heavy usage.

---

## 3. Structured Observability: Metrics, Diagnostics, and Tracing

**What OpenClaw does:** Provides structured logging with automatic secret redaction, a `openclaw doctor` CLI command that checks system health (API keys valid, channels connected, disk space, dependencies installed), heartbeat tracking with ACK windows for session warmth monitoring, and usage metrics per agent/channel/model.

**What StarkBot has today:** `env_logger` with string-prefixed log lines (`[Keystore]`, `[Gateway]`, `[x402]`). The `ExecutionTracker` exists for task progress, and events are broadcast via WebSocket, but there are no aggregated metrics, no health-check CLI, no secret redaction in logs, and no Prometheus/StatsD export.

**Proposed upgrade:**

**A. `starkbot doctor` CLI command:**
- Check all configured API keys are valid (make a lightweight test call).
- Verify database schema is up-to-date.
- Test channel connectivity (Discord bot token valid, Telegram webhook reachable, etc.).
- Check disk space for workspace/memory/journal directories.
- Verify required external tools are installed (git, python3, etc.).
- Print a color-coded summary: green/yellow/red per check.

**B. Prometheus metrics endpoint (`/metrics`):**
- `starkbot_requests_total{channel, model, status}` — counter per request.
- `starkbot_tool_calls_total{tool_name, success}` — tool usage tracking.
- `starkbot_token_usage{model, direction}` — input/output token counts.
- `starkbot_response_latency_seconds{model}` — histogram of AI response times.
- `starkbot_active_sessions` — gauge of open sessions.
- `starkbot_context_compactions_total` — compaction frequency.
- Use the `prometheus` crate — lightweight, well-maintained.

**C. Structured logging with `tracing`:**
- Replace `env_logger` + `log` with `tracing` + `tracing-subscriber`.
- Add span context: `request_id`, `session_id`, `channel_id`, `model`.
- Enable JSON output mode for log aggregation (ELK, Loki, etc.).
- Automatic secret redaction: filter fields named `*_key`, `*_secret`, `*_token` in log output.

**Impact:** Makes production debugging feasible, enables alerting on error spikes, provides usage analytics for cost management, and gives users a one-command health check.

---

## 4. Progressive Streaming with Block Chunking

**What OpenClaw does:** Implements "soft chunking" — AI output is streamed to clients progressively in semantic blocks (paragraphs, code fences, list items) rather than token-by-token or all-at-once. The `pi-embedded-block-chunker` detects block boundaries and flushes complete units. Tool execution results are also streamed inline with summaries. This means users see coherent paragraphs appear one at a time during long responses.

**What StarkBot has today:** The WebSocket gateway broadcasts `agent_response` events, and the frontend receives them, but responses are sent as complete messages after the full tool loop finishes. During multi-tool execution, the user sees "thinking" status but no incremental output. Long responses (especially multi-step tasks) create a poor UX with extended waiting.

**Proposed upgrade:**

- **Streaming response accumulator:** As the AI streams tokens, buffer them and detect block boundaries:
  - Double newline → paragraph break → flush.
  - Code fence close (` ``` `) → flush code block.
  - Tool call detected → flush preceding text, show "Executing {tool_name}..." status.
  - Tool result received → flush tool summary inline.
- **New gateway event types:**
  - `agent_block(channel_id, block_type, content)` — semantic block (paragraph, code, list).
  - `agent_tool_status(channel_id, tool_name, phase)` — "started", "completed", "failed".
  - `agent_stream_end(channel_id)` — final signal.
- **Frontend rendering:** Update `AgentChat` component to append blocks incrementally using a streaming state machine. Show a typing indicator between blocks.
- **Channel adapters:** For Discord/Slack/Telegram, batch blocks into messages respecting platform limits (2000 chars for Discord, 4096 for Telegram). Edit the last message to append new blocks rather than sending multiple messages.
- **Implementation path:** Start with the Claude client (`ai/claude.rs`) which already returns streaming chunks — add a `BlockAccumulator` that sits between the stream and the dispatcher, emitting `GatewayEvent::AgentBlock` events.

**Impact:** Transforms the perceived responsiveness of the agent. Users get feedback within 1-2 seconds instead of waiting 10-30 seconds for complex multi-tool responses. Critical for chat channels where long silences feel broken.

---

## 5. Unified YAML Configuration with Hot-Reload

**What OpenClaw does:** Uses a single `~/.openclaw/config.yaml` file with hierarchical structure: global agent defaults (`agents.defaults`), per-agent overrides (`agents.list[].{model, skills, workspace, ...}`), channel-specific settings (`channels.{slack, discord, ...}`), and plugin configs. Supports `${ENV_VAR}` expansion in values. Config reloads on `SIGHUP` signal and via filesystem watch (`chokidar`), meaning changes take effect without restarting the process.

**What StarkBot has today:** Configuration is split across multiple sources: environment variables (`.env`), database tables (`agent_settings`, `bot_settings`, `channel_settings`, `external_api_keys`), hardcoded defaults in Rust, and the web UI settings pages. There's no single source of truth, no config file for version control, and changes to most settings require an API call or DB update + restart.

**Proposed upgrade:**

- **Single config file** (`starkbot.yaml` or `~/.starkbot/config.yaml`):
  ```yaml
  server:
    http_port: 8080
    ws_port: 8081
    database_url: ./.db/stark.db

  agent:
    default_model: claude-sonnet-4-5-20250929
    thinking_level: low
    fallback_models:
      - gpt-4o
      - kimi-latest
    context_window: 100000
    reserve_tokens: 20000

  channels:
    discord:
      enabled: true
      token: ${DISCORD_BOT_TOKEN}
      safe_mode: false
    telegram:
      enabled: true
      token: ${TELEGRAM_BOT_TOKEN}
    slack:
      enabled: true
      bot_token: ${SLACK_BOT_TOKEN}
      app_token: ${SLACK_APP_TOKEN}

  tools:
    disabled: [exec, delete_file]
    validators:
      enabled: true

  skills:
    directory: ./skills
    auto_load: true

  memory:
    directory: ./memory
    fts_reindex_interval: 3600

  x402:
    enabled: true
    default_limit_usd: 1.00

  wallet:
    mode: standard  # or "flash"
    # private_key loaded from env only, never in config
  ```
- **Environment variable expansion:** Parse `${VAR}` and `${VAR:-default}` patterns before applying config.
- **Hot-reload:** Use `notify` crate to watch the config file. On change, diff the new config against the running config and apply safe changes (model settings, channel enable/disable, tool allowlists, thinking level) without restart. Unsafe changes (ports, database URL) log a warning that restart is required.
- **Priority order:** CLI flags > environment variables > config file > database defaults.
- **Migration:** On first run, generate `starkbot.yaml` from current DB settings + env vars. Keep DB settings as override layer (UI changes write to DB, which takes priority over file).
- **Version control friendly:** Users can commit `starkbot.yaml` (minus secrets) to track configuration changes.

**Impact:** Simplifies deployment (one file to configure), enables GitOps workflows, makes configuration reproducible and auditable, and hot-reload reduces downtime for config changes.

---

## Summary

| # | Improvement | Effort | Impact | Priority |
|---|------------|--------|--------|----------|
| 1 | Plugin SDK with Lifecycle Hooks | High | High | Medium-term |
| 2 | Model Failover Chain + Auth Rotation | Medium | High | **Immediate** |
| 3 | Structured Observability (metrics, doctor, tracing) | Medium | High | **Immediate** |
| 4 | Progressive Streaming with Block Chunking | Medium | Medium-High | Short-term |
| 5 | Unified YAML Config with Hot-Reload | Medium | Medium | Short-term |

**Recommended order:** Start with **#2** (failover) and **#3** (observability) — they're the highest-ROI improvements with moderate effort. Then **#5** (config) to reduce operational friction, **#4** (streaming) for UX, and **#1** (plugins) as a longer-term investment in extensibility.
