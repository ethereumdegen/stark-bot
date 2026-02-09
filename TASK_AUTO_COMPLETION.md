# Task Auto-Completion

When the AI plans a multi-step session via `define_tasks`, each task normally requires an extra API round-trip for the AI to signal completion (`task_fully_completed` or `say_to_user(finished_task=true)`). Auto-completion eliminates that overhead for tasks that map 1:1 to a specific tool call.

## How It Works

1. **At plan time** — when `define_tasks` builds the task queue, each task description is scanned (case-insensitive substring match) against the available tool names. If a tool name appears in the description, it's stored in `auto_complete_tool` on that task.

2. **At execution time** — after every successful tool call, the dispatcher checks whether the tool name matches the current task's `auto_complete_tool`. If it does, the task is immediately marked completed and the queue advances to the next task — no extra AI call needed.

3. **In the system prompt** — when a task has an `auto_complete_tool`, the AI sees a hint:
   ```
   Note: This task will auto-complete when `token_lookup` succeeds.
   You do NOT need to call `task_fully_completed` for this task.
   ```
   This prevents the AI from wasting a turn on a redundant completion call.

## Example

User asks: *"What's the price of ETH and then send 1 ETH to alice.eth"*

The planner creates:
| Task | Description | auto_complete_tool |
|------|-------------|--------------------|
| 1 | Look up ETH price using `token_lookup` | `token_lookup` |
| 2 | Send 1 ETH to alice.eth using `web3_tx` | `web3_tx` |
| 3 | Report results to the user | `None` |

- Task 1 auto-completes the moment `token_lookup` returns successfully.
- Task 2 auto-completes the moment `web3_tx` returns successfully.
- Task 3 has no matching tool, so the AI completes it manually via `say_to_user(finished_task=true)`.

Result: 2 fewer API round-trips compared to the old behavior.

## Matching Rules

- **Case-insensitive substring**: `"TOKEN_LOOKUP"` in a description matches tool `token_lookup`.
- **Longest wins**: If a description mentions both `web3` and `web3_preset_function_call`, the longer name is picked (more specific).
- **System tools excluded**: `say_to_user`, `task_fully_completed`, `define_tasks`, `set_agent_subtype`, `add_task`, `ask_user`, `subagent`, `subagent_status`, `use_skill`, and `manage_skills` are never matched. These are orchestration tools, not action tools.
- **Dynamic tasks unaffected**: Tasks added later via `add_task` get `auto_complete_tool: None` by default.

## Safety Guards

| Scenario | Guard |
|----------|-------|
| Tool call fails | `result.success` must be true |
| `define_tasks` in same batch (native path) | `!define_tasks_replaced_queue` prevents auto-completing the wrong task |
| AI calls `task_fully_completed` after auto-complete | `auto_completed_task` flag suppresses duplicate advancement in same batch |
| AI calls `say_to_user(finished_task)` after auto-complete | Same `auto_completed_task` flag |
| Already completed by explicit call | `complete_current_task()` returns `None` when no current task — idempotent |
| Multiple tool calls in one native batch | Each processed sequentially; task 1 auto-completes → advances to task 2 → task 2's tool in same batch can also auto-complete |

## Code Locations

- **`PlannerTask.auto_complete_tool`** — `src/ai/multi_agent/types.rs`
- **`TaskQueue::from_descriptions_with_tool_matching()`** — `src/ai/multi_agent/types.rs`
- **Auto-complete check (native path)** — `src/channels/dispatcher.rs`, search for `[AUTO_COMPLETE]` in the native tool loop
- **Auto-complete check (text path)** — `src/channels/dispatcher.rs`, search for `[AUTO_COMPLETE]` in the text tool loop
- **System prompt hint** — `src/ai/multi_agent/orchestrator.rs`, in `get_system_prompt()`
- **Broadcast JSON** — `src/gateway/protocol.rs`, `task_queue_update()` includes the field

## Unit Tests

In `types.rs` under `task_queue_tests`:

- `test_auto_complete_basic_match` — tool name in description → stored
- `test_auto_complete_no_match` — no tool names → `None`
- `test_auto_complete_longest_wins` — `web3` vs `web3_preset_function_call` → picks longest
- `test_auto_complete_case_insensitive` — `TOKEN_LOOKUP` matches `token_lookup`
- `test_auto_complete_excludes_system_tools` — `say_to_user` in description → `None`
- `test_auto_complete_serde_backward_compat` — old JSON without field deserializes with `None`
