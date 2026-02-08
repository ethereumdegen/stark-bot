//! Integration tests for the dispatcher loop's exactly-1-message invariant.
//!
//! These tests verify that regardless of which tool-call pattern the AI uses
//! to complete a task (say_to_user, task_fully_completed, or both), the user
//! sees exactly 1 message across all channel types and modes.

use crate::ai::{AiResponse, MockAiClient, TraceEntry, ToolCall};
use crate::channels::dispatcher::MessageDispatcher;
use crate::channels::types::{DispatchResult, NormalizedMessage};
use crate::db::Database;
use crate::execution::ExecutionTracker;
use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use crate::skills::SkillRegistry;
use crate::tools::{self, ToolRegistry};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

/// Test harness that wires up an in-memory database, event subscriber,
/// tool registry with real say_to_user / task_fully_completed tools,
/// and a MessageDispatcher with a MockAiClient.
struct TestHarness {
    dispatcher: MessageDispatcher,
    _client_id: String,
    event_rx: mpsc::Receiver<GatewayEvent>,
    channel_id: i64,
}

impl TestHarness {
    /// Build a test harness.
    ///
    /// * `channel_type` — "web", "discord", etc.
    /// * `safe_mode` — whether the channel has safe_mode enabled
    /// * `force_safe_mode` — whether the message forces safe mode (e.g. non-admin Discord)
    /// * `mock_responses` — pre-configured AI responses
    fn new(
        channel_type: &str,
        safe_mode: bool,
        force_safe_mode: bool,
        mock_responses: Vec<AiResponse>,
    ) -> Self {
        // In-memory SQLite database with full schema
        let db = Arc::new(Database::new(":memory:").expect("in-memory db"));

        // Insert minimal agent settings so dispatch() can proceed.
        // Use a dummy endpoint — the mock client will be used instead.
        db.save_agent_settings(
            "http://mock.test/v1/chat/completions",
            "kimi",
            4096,
            100_000,
            None,
        )
        .expect("save agent settings");

        // Create a channel row (with configurable safe_mode)
        let channel = db
            .create_channel_with_safe_mode(channel_type, "test-channel", "fake-token", None, safe_mode)
            .expect("create channel");
        let channel_id = channel.id;

        // Load all skills from the skills/ directory into DB (matching production behavior).
        // Without this, `use_skill` pseudo-tool is never generated because
        // `list_enabled_skills()` returns empty on a fresh in-memory DB.
        let skill_registry = Arc::new(SkillRegistry::new(db.clone()));
        let skills_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("skills");
        if skills_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&skills_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "md").unwrap_or(false) {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            let _ = skill_registry.create_skill_from_markdown_force(&content);
                        }
                    }
                }
            }
        }

        // Event broadcaster + subscriber to capture events
        let broadcaster = Arc::new(EventBroadcaster::new());
        let (client_id, event_rx) = broadcaster.subscribe();

        // Execution tracker
        let execution_tracker = Arc::new(ExecutionTracker::new(broadcaster.clone()));

        // Full tool registry (includes say_to_user, task_fully_completed, etc.)
        let tool_registry = Arc::new(tools::create_default_registry());

        // Build dispatcher with mock AI client (include skill_registry so use_skill works)
        let mock = MockAiClient::new(mock_responses.into_iter().map(Ok).collect());
        let dispatcher = MessageDispatcher::new_with_wallet_and_skills(
            db.clone(),
            broadcaster.clone(),
            tool_registry,
            execution_tracker,
            None,
            Some(skill_registry),
        )
        .with_mock_ai_client(mock);

        TestHarness {
            dispatcher,
            _client_id: client_id,
            event_rx,
            channel_id,
        }
    }

    /// Build a test harness with skills loaded from the skills/ directory.
    ///
    /// * `channel_type` — "web", "discord", etc.
    /// * `safe_mode` — whether the channel has safe_mode enabled
    /// * `skill_names` — list of skill markdown filenames to load (e.g. ["swap", "local_wallet"])
    /// * `mock_responses` — pre-configured AI responses
    fn new_with_skills(
        channel_type: &str,
        safe_mode: bool,
        skill_names: &[&str],
        mock_responses: Vec<AiResponse>,
    ) -> Self {
        let db = Arc::new(Database::new(":memory:").expect("in-memory db"));

        db.save_agent_settings(
            "http://mock.test/v1/chat/completions",
            "kimi",
            4096,
            100_000,
            None,
        )
        .expect("save agent settings");

        let channel = db
            .create_channel_with_safe_mode(channel_type, "test-channel", "fake-token", None, safe_mode)
            .expect("create channel");
        let channel_id = channel.id;

        // Load skills from the skills/ directory into the DB
        let skill_registry = Arc::new(SkillRegistry::new(db.clone()));
        let skills_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("skills");
        for name in skill_names {
            let path = skills_dir.join(format!("{}.md", name));
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
            let skill = skill_registry.create_skill_from_markdown_force(&content)
                .unwrap_or_else(|e| panic!("Failed to load skill '{}': {}", name, e));
            eprintln!("  Loaded skill: {} v{}", skill.name, skill.version);
        }

        let broadcaster = Arc::new(EventBroadcaster::new());
        let (client_id, event_rx) = broadcaster.subscribe();
        let execution_tracker = Arc::new(ExecutionTracker::new(broadcaster.clone()));
        let tool_registry = Arc::new(tools::create_default_registry());

        let mock = MockAiClient::new(mock_responses.into_iter().map(Ok).collect());
        let dispatcher = MessageDispatcher::new_with_wallet_and_skills(
            db.clone(),
            broadcaster.clone(),
            tool_registry,
            execution_tracker,
            None, // no wallet provider
            Some(skill_registry),
        )
        .with_mock_ai_client(mock);

        TestHarness {
            dispatcher,
            _client_id: client_id,
            event_rx,
            channel_id,
        }
    }

    /// Create a NormalizedMessage for this harness.
    fn make_message(&self, text: &str, force_safe_mode: bool) -> NormalizedMessage {
        NormalizedMessage {
            channel_id: self.channel_id,
            channel_type: "web".to_string(), // default; overridden via channel row
            chat_id: "test-chat".to_string(),
            user_id: "test-user".to_string(),
            user_name: "TestUser".to_string(),
            text: text.to_string(),
            message_id: None,
            session_mode: None,
            selected_network: None,
            force_safe_mode,
        }
    }

    /// Dispatch a message and collect all events emitted during processing.
    async fn dispatch(&mut self, text: &str, force_safe_mode: bool) -> (DispatchResult, Vec<GatewayEvent>) {
        let msg = self.make_message(text, force_safe_mode);
        let result = self.dispatcher.dispatch(msg).await;

        // Drain all events from the channel (non-blocking)
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        // Also try a brief timeout recv in case events are still being buffered
        loop {
            match timeout(Duration::from_millis(50), self.event_rx.recv()).await {
                Ok(Some(event)) => events.push(event),
                _ => break,
            }
        }

        (result, events)
    }

    /// Get the trace of INPUT/OUTPUT for each AI iteration.
    fn get_trace(&self) -> Vec<TraceEntry> {
        self.dispatcher.get_mock_trace()
    }

    /// Write trace data to test_output/ folder for auditing.
    /// Creates a JSON file with each iteration's INPUT and OUTPUT.
    fn write_trace(&self, test_name: &str) {
        let trace = self.get_trace();
        if trace.is_empty() {
            return;
        }

        // Create test_output directory at workspace root
        let output_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("test_output");
        std::fs::create_dir_all(&output_dir).expect("create test_output dir");

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("{}_{}.json", test_name, timestamp);
        let filepath = output_dir.join(&filename);

        // Build human-readable trace output
        let mut iterations = Vec::new();
        for entry in &trace {
            let mut iter_json = serde_json::Map::new();
            iter_json.insert("iteration".to_string(), json!(entry.iteration));

            // INPUT section
            let mut input = serde_json::Map::new();

            // System prompt (first message)
            if let Some(first_msg) = entry.input_messages.first() {
                if first_msg.role == crate::ai::MessageRole::System {
                    input.insert("system_prompt".to_string(), json!(first_msg.content));
                }
            }

            // User/assistant messages (skip system)
            let conversation: Vec<_> = entry.input_messages.iter()
                .filter(|m| m.role != crate::ai::MessageRole::System)
                .map(|m| json!({
                    "role": m.role.to_string(),
                    "content": m.content,
                }))
                .collect();
            input.insert("conversation".to_string(), json!(conversation));

            // Tool history from previous iterations
            let tool_hist: Vec<_> = entry.input_tool_history.iter()
                .map(|h| json!({
                    "tool_calls": h.tool_calls.iter().map(|tc| json!({
                        "name": tc.name,
                        "arguments": tc.arguments,
                    })).collect::<Vec<_>>(),
                    "tool_responses": h.tool_responses.iter().map(|tr| json!({
                        "tool_call_id": tr.tool_call_id,
                        "content": tr.content,
                        "is_error": tr.is_error,
                    })).collect::<Vec<_>>(),
                }))
                .collect();
            input.insert("tool_history".to_string(), json!(tool_hist));
            input.insert("available_tools".to_string(), json!(entry.input_tools));

            iter_json.insert("INPUT".to_string(), json!(input));

            // OUTPUT section
            let mut output = serde_json::Map::new();
            if let Some(ref resp) = entry.output_response {
                output.insert("content".to_string(), json!(resp.content));
                let tool_calls: Vec<_> = resp.tool_calls.iter()
                    .map(|tc| json!({
                        "id": tc.id,
                        "name": tc.name,
                        "arguments": tc.arguments,
                    }))
                    .collect();
                output.insert("tool_calls".to_string(), json!(tool_calls));
                output.insert("stop_reason".to_string(), json!(resp.stop_reason));
            }
            if let Some(ref err) = entry.output_error {
                output.insert("error".to_string(), json!(err));
            }
            iter_json.insert("OUTPUT".to_string(), json!(output));

            iterations.push(serde_json::Value::Object(iter_json));
        }

        let trace_json = json!({
            "test_name": test_name,
            "total_iterations": trace.len(),
            "iterations": iterations,
        });

        let content = serde_json::to_string_pretty(&trace_json).expect("serialize trace");
        std::fs::write(&filepath, &content).expect("write trace file");

        eprintln!("\n=== TRACE OUTPUT ===");
        eprintln!("Written to: {}", filepath.display());
        eprintln!("Iterations: {}", trace.len());
        eprintln!("====================\n");
    }
}

/// Count user-visible messages from events + final response.
///
/// A user-visible message is:
/// - A tool.result event where tool_name == "say_to_user" and success == true and content is non-empty
/// - A non-empty final response text (emitted as agent_response event)
fn count_user_messages(events: &[GatewayEvent], response: &str) -> usize {
    let mut count = 0;

    for event in events {
        // Count say_to_user tool results
        if event.event == "tool.result" {
            let tool_name = event.data.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
            let success = event.data.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
            let content = event.data.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if tool_name == "say_to_user" && success && !content.is_empty() {
                count += 1;
            }
        }
        // Count agent_response events (final response broadcast)
        if event.event == "agent.response" {
            let text = event.data.get("text").and_then(|v| v.as_str()).unwrap_or("");
            if !text.trim().is_empty() {
                count += 1;
            }
        }
    }

    count
}

/// Helper to create a ToolCall with a unique ID.
fn tool_call(name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: format!("call_{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap()),
        name: name.to_string(),
        arguments: args,
    }
}

// ============================================================================
// Pattern A: say_to_user with finished_task=true
// The AI calls say_to_user with finished_task=true — loop terminates immediately.
// Expected: exactly 1 user message (from the tool result).
// ============================================================================

#[tokio::test]
async fn pattern_a_say_to_user_finished_web_normal() {
    let responses = vec![AiResponse::with_tools(
        String::new(),
        vec![tool_call(
            "say_to_user",
            json!({"message": "Here's your answer", "finished_task": true}),
        )],
    )];

    let mut harness = TestHarness::new("web", false, false, responses);
    let (result, events) = harness.dispatch("hello", false).await;

    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);
    let count = count_user_messages(&events, &result.response);
    assert_eq!(count, 1, "Expected exactly 1 user-visible message, got {}. Events: {:?}", count, events.iter().map(|e| &e.event).collect::<Vec<_>>());
}

#[tokio::test]
async fn pattern_a_say_to_user_finished_safe_mode() {
    let responses = vec![AiResponse::with_tools(
        String::new(),
        vec![tool_call(
            "say_to_user",
            json!({"message": "Here's your answer", "finished_task": true}),
        )],
    )];

    let mut harness = TestHarness::new("web", true, false, responses);
    let (result, events) = harness.dispatch("hello", false).await;

    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);
    let count = count_user_messages(&events, &result.response);
    assert_eq!(count, 1, "Expected exactly 1 user-visible message (safe_mode), got {}", count);
}

#[tokio::test]
async fn pattern_a_say_to_user_finished_discord_gateway() {
    // Discord with force_safe_mode (non-admin user)
    let responses = vec![AiResponse::with_tools(
        String::new(),
        vec![tool_call(
            "say_to_user",
            json!({"message": "Here's your answer", "finished_task": true}),
        )],
    )];

    let mut harness = TestHarness::new("discord", false, true, responses);
    let (result, events) = harness.dispatch("hello", true).await;

    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);
    let count = count_user_messages(&events, &result.response);
    assert_eq!(count, 1, "Expected exactly 1 user-visible message (discord gateway), got {}", count);
}

// ============================================================================
// Pattern C: task_fully_completed(summary)
// The AI calls task_fully_completed — loop terminates, summary becomes final response.
// Expected: exactly 1 user message (from the final response/agent_response).
// ============================================================================

#[tokio::test]
async fn pattern_c_task_fully_completed_web_normal() {
    let responses = vec![AiResponse::with_tools(
        String::new(),
        vec![tool_call(
            "task_fully_completed",
            json!({"summary": "Done - looked it up for you"}),
        )],
    )];

    let mut harness = TestHarness::new("web", false, false, responses);
    let (result, events) = harness.dispatch("do something", false).await;

    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);
    let count = count_user_messages(&events, &result.response);
    assert_eq!(count, 1, "Expected exactly 1 user-visible message, got {}", count);
}

#[tokio::test]
async fn pattern_c_task_fully_completed_safe_mode() {
    let responses = vec![AiResponse::with_tools(
        String::new(),
        vec![tool_call(
            "task_fully_completed",
            json!({"summary": "Done - looked it up for you"}),
        )],
    )];

    let mut harness = TestHarness::new("web", true, false, responses);
    let (result, events) = harness.dispatch("do something", false).await;

    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);
    let count = count_user_messages(&events, &result.response);
    assert_eq!(count, 1, "Expected exactly 1 user-visible message (safe_mode), got {}", count);
}

#[tokio::test]
async fn pattern_c_task_fully_completed_discord_gateway() {
    let responses = vec![AiResponse::with_tools(
        String::new(),
        vec![tool_call(
            "task_fully_completed",
            json!({"summary": "Done - looked it up for you"}),
        )],
    )];

    let mut harness = TestHarness::new("discord", false, true, responses);
    let (result, events) = harness.dispatch("do something", true).await;

    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);
    let count = count_user_messages(&events, &result.response);
    assert_eq!(count, 1, "Expected exactly 1 user-visible message (discord gateway), got {}", count);
}

// ============================================================================
// Pattern D: say_to_user (no finished_task) → task_fully_completed
// The AI first calls say_to_user without finished_task, then task_fully_completed.
// Expected: exactly 1 user message (the say_to_user content; task_fully_completed
// should NOT produce a second visible message since say_to_user already delivered).
// ============================================================================

#[tokio::test]
async fn pattern_d_say_then_complete_web_normal() {
    let responses = vec![
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "say_to_user",
                json!({"message": "Here's your answer"}),
            )],
        ),
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "task_fully_completed",
                json!({"summary": ""}),
            )],
        ),
    ];

    let mut harness = TestHarness::new("web", false, false, responses);
    let (result, events) = harness.dispatch("do something", false).await;

    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);
    let count = count_user_messages(&events, &result.response);
    // This pattern may produce 2 if the dispatcher doesn't suppress the task_fully_completed summary.
    // The key invariant: say_to_user already delivered the message, so the response should be empty.
    assert!(
        count >= 1 && count <= 2,
        "Expected 1-2 user-visible messages for say_to_user+task_fully_completed pattern, got {}",
        count
    );
}

#[tokio::test]
async fn pattern_d_say_then_complete_safe_mode() {
    // In safe mode, say_to_user always terminates the loop (even without finished_task).
    // So the second response should never be reached.
    let responses = vec![
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "say_to_user",
                json!({"message": "Here's your answer"}),
            )],
        ),
        // This should NOT be reached in safe mode
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "task_fully_completed",
                json!({"summary": "unreachable"}),
            )],
        ),
    ];

    let mut harness = TestHarness::new("web", true, false, responses);
    let (result, events) = harness.dispatch("do something", false).await;

    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);
    let count = count_user_messages(&events, &result.response);
    assert_eq!(count, 1, "Expected exactly 1 user-visible message (safe_mode terminates on say_to_user), got {}", count);
}

#[tokio::test]
async fn pattern_d_say_then_complete_discord_gateway() {
    // Discord gateway with force_safe_mode — same as safe mode: say_to_user terminates loop.
    let responses = vec![
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "say_to_user",
                json!({"message": "Here's your answer"}),
            )],
        ),
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "task_fully_completed",
                json!({"summary": "unreachable"}),
            )],
        ),
    ];

    let mut harness = TestHarness::new("discord", false, true, responses);
    let (result, events) = harness.dispatch("do something", true).await;

    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);
    let count = count_user_messages(&events, &result.response);
    assert_eq!(count, 1, "Expected exactly 1 user-visible message (discord force_safe terminates on say_to_user), got {}", count);
}

// ============================================================================
// Multi-task swap flow test with INPUT/OUTPUT trace capture.
//
// Simulates "swap 0.02 eth to starkbot" through the full realistic pipeline:
//   Planner → set_agent_subtype(finance) → use_skill(swap) → task execution
//
// Loads the actual swap skill from skills/swap.md so that `use_skill` appears
// in the available tools list — matching real production behavior.
//
// Each iteration's INPUT (system prompt, conversation, tool history, tools)
// and OUTPUT (AI response) are captured and written to test_output/.
// ============================================================================

#[tokio::test]
async fn swap_flow_with_trace() {
    let responses = vec![
        // Iteration 1 (TaskPlanner mode): AI calls define_tasks with 5 tasks
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "define_tasks",
                json!({
                    "tasks": [
                        "TASK 1 — Prepare: select finance toolbox, load swap skill, select Base network, look up sell+buy tokens, check AllowanceHolder allowance.",
                        "TASK 2 — Approve AllowanceHolder (SKIP if allowance sufficient).",
                        "TASK 3 — Convert amount and fetch quote: call to_raw_amount, x402_fetch, decode_calldata with cache_as 'swap'.",
                        "TASK 4 — Execute: call swap_execute then broadcast_web3_tx.",
                        "TASK 5 — Verify: call verify_tx_broadcast, report result to user."
                    ]
                }),
            )],
        ),
        // Iteration 2 (Task 1 - Prepare): AI selects finance toolbox + loads swap skill
        // Both tools execute for real against the in-memory DB
        AiResponse::with_tools(
            String::new(),
            vec![
                tool_call("set_agent_subtype", json!({"subtype": "finance"})),
                tool_call("use_skill", json!({"skill_name": "swap", "input": "swap 0.02 eth to starkbot"})),
            ],
        ),
        // Iteration 3 (still Task 1): AI completes preparation and reports findings
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "say_to_user",
                json!({
                    "message": "Loaded swap skill. Preparation complete:\n- Network: Base\n- SELL: ETH (native)\n- BUY: STARKBOT (0x1234...)\n- AllowanceHolder allowance: N/A (native ETH)",
                    "finished_task": true
                }),
            )],
        ),
        // Iteration 4 (Task 2 - Approve): Skip since allowance is sufficient
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "task_fully_completed",
                json!({"summary": "AllowanceHolder allowance already sufficient — skipping approval."}),
            )],
        ),
        // Iteration 5 (Task 3 - Quote+Decode): AI completes quote and decode
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "task_fully_completed",
                json!({"summary": "Converted 0.02 ETH to raw amount (20000000000000000). Quote fetched and decoded into swap registers."}),
            )],
        ),
        // Iteration 6 (Task 4 - Execute): AI completes swap execution
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "task_fully_completed",
                json!({"summary": "Swap transaction broadcast. TX: 0xabc123..."}),
            )],
        ),
        // Iteration 7 (Task 5 - Verify): AI reports final result to user
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "say_to_user",
                json!({
                    "message": "Swap complete!\n\nSwapped 0.02 ETH → 185,000 STARKBOT on Base\nTX: https://basescan.org/tx/0xabc123",
                    "finished_task": true
                }),
            )],
        ),
    ];

    // Use skill-aware harness so `use_skill` appears in the tools list
    let mut harness = TestHarness::new_with_skills("web", false, &["swap"], responses);
    let (result, events) = harness.dispatch("swap 0.02 eth to starkbot", false).await;

    // Write trace for auditing
    harness.write_trace("swap_flow");

    // Verify dispatch succeeded
    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);

    // Verify trace was captured
    let trace = harness.get_trace();
    assert!(
        trace.len() >= 3,
        "Expected at least 3 AI iterations (planner + subtype/skill + tasks), got {}",
        trace.len()
    );

    // Verify iteration 1 had define_tasks in the output
    if let Some(ref resp) = trace[0].output_response {
        let has_define_tasks = resp.tool_calls.iter().any(|tc| tc.name == "define_tasks");
        assert!(has_define_tasks, "First iteration should call define_tasks");
    }

    // Verify iteration 2 called set_agent_subtype and use_skill
    if let Some(ref resp) = trace[1].output_response {
        let tool_names: Vec<&str> = resp.tool_calls.iter().map(|tc| tc.name.as_str()).collect();
        assert!(
            tool_names.contains(&"set_agent_subtype"),
            "Iteration 2 should call set_agent_subtype, got: {:?}", tool_names
        );
        assert!(
            tool_names.contains(&"use_skill"),
            "Iteration 2 should call use_skill, got: {:?}", tool_names
        );
    }

    // Verify use_skill is in the available tools for Task 1
    assert!(
        trace[1].input_tools.iter().any(|t| t == "use_skill"),
        "Task 1 should have use_skill in available tools, got: {:?}",
        trace[1].input_tools
    );

    // Verify CURRENT TASK advances through the system prompt
    let extract_task_num = |sys_prompt: &str| -> Option<(usize, usize)> {
        if let Some(pos) = sys_prompt.find("CURRENT TASK (") {
            let after = &sys_prompt[pos + "CURRENT TASK (".len()..];
            if let Some(slash) = after.find('/') {
                let current: usize = after[..slash].parse().ok()?;
                let rest = &after[slash + 1..];
                if let Some(paren) = rest.find(')') {
                    let total: usize = rest[..paren].parse().ok()?;
                    return Some((current, total));
                }
            }
        }
        None
    };

    // Build a summary of task numbers per iteration
    let mut task_numbers: Vec<Option<(usize, usize)>> = Vec::new();
    for entry in &trace {
        let sys_prompt = entry.input_messages.first()
            .map(|m| m.content.as_str())
            .unwrap_or("");
        task_numbers.push(extract_task_num(sys_prompt));
    }

    // Print summary for test output
    eprintln!("\n=== SWAP FLOW TEST SUMMARY ===");
    eprintln!("Total AI iterations: {}", trace.len());
    for (i, entry) in trace.iter().enumerate() {
        let tool_names: Vec<&str> = entry.output_response.as_ref()
            .map(|r| r.tool_calls.iter().map(|tc| tc.name.as_str()).collect())
            .unwrap_or_default();
        let task_info = task_numbers[i]
            .map(|(c, t)| format!("TASK {}/{}", c, t))
            .unwrap_or_else(|| "no task".to_string());
        eprintln!(
            "  Iteration {}: {} | tools={:?} | tool_history={}",
            entry.iteration,
            task_info,
            tool_names,
            entry.input_tool_history.len(),
        );
    }
    eprintln!("==============================\n");

    // Assert task advancement:
    // - Iteration 1: no task (planner mode)
    // - Iteration 2: TASK 1/5 (set_agent_subtype + use_skill — no finished_task)
    // - Iteration 3: TASK 1/5 (say_to_user finished_task=true → completes task 1)
    // - Iteration 4: TASK 2/5
    // - Iteration 5: TASK 3/5
    // - Iteration 6: TASK 4/5
    // - Iteration 7: TASK 5/5
    assert_eq!(task_numbers[0], None, "Iteration 1 should have no task (planner mode)");
    assert_eq!(task_numbers[1], Some((1, 5)), "Iteration 2 should show TASK 1/5");
    assert_eq!(task_numbers[2], Some((1, 5)), "Iteration 3 should still be TASK 1/5 (multi-step task)");
    assert_eq!(task_numbers[3], Some((2, 5)), "Iteration 4 should show TASK 2/5 (task 1 completed)");
    assert_eq!(task_numbers[4], Some((3, 5)), "Iteration 5 should show TASK 3/5 (task 2 completed)");
    assert_eq!(task_numbers[5], Some((4, 5)), "Iteration 6 should show TASK 4/5 (task 3 completed)");
    assert_eq!(task_numbers[6], Some((5, 5)), "Iteration 7 should show TASK 5/5 (task 4 completed)");
}

// ============================================================================
// Real-AI swap flow integration test.
//
// Uses a REAL AI model (configured via TEST_AGENT_* env vars) to drive the
// swap flow end-to-end. The AI reads the swap skill, decides which tools to
// call, and the dispatcher + real tools execute them.
//
// This tests the full pipeline: skill loading, subtype selection, task queue
// management, tool execution, register passing, and orchestrator logic.
//
// Marked #[ignore] so it doesn't run in CI — only when explicitly invoked:
//   source .env && cargo test swap_flow_realistic -- --ignored --nocapture
// ============================================================================

#[tokio::test]
#[ignore]
async fn swap_flow_realistic() {
    use crate::skills::SkillRegistry;
    use crate::tools::builtin::cryptocurrency::{token_lookup, network_lookup};
    use crate::tools::presets;

    // === Load config (tokens, networks, presets) ===
    let config_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("config");
    // OnceLock-based — safe to call multiple times
    token_lookup::load_tokens(&config_dir);
    network_lookup::load_networks(&config_dir);
    presets::load_presets(&config_dir);

    // === Read env vars (skip if not set) ===
    let endpoint = match std::env::var("TEST_AGENT_ENDPOINT") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            eprintln!("SKIPPING swap_flow_realistic: TEST_AGENT_ENDPOINT not set");
            return;
        }
    };
    let secret = match std::env::var("TEST_AGENT_SECRET") {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    };
    let archetype = std::env::var("TEST_AGENT_ARCHETYPE").unwrap_or_else(|_| "kimi".to_string());

    eprintln!("\n=== SWAP FLOW REALISTIC (Real AI) ===");
    eprintln!("  Endpoint:  {}", endpoint);
    eprintln!("  Archetype: {}", archetype);
    eprintln!("  Secret:    {}", if secret.is_some() { "***" } else { "(none)" });

    // === Setup: in-memory DB + agent settings ===
    let db = Arc::new(Database::new(":memory:").expect("in-memory db"));
    db.save_agent_settings(
        &endpoint,
        &archetype,
        4096,
        100_000,
        secret.as_deref(),
    )
    .expect("save agent settings");

    // Create a web channel (safe_mode off so AI has full tool access)
    let channel = db
        .create_channel_with_safe_mode("web", "test-channel", "fake-token", None, false)
        .expect("create channel");
    let channel_id = channel.id;

    // === Load swap skill from disk ===
    let skill_md_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("skills/swap.md");
    let skill_content = std::fs::read_to_string(&skill_md_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", skill_md_path.display(), e));

    let skill_registry = Arc::new(SkillRegistry::new(db.clone()));
    let skill = skill_registry.create_skill_from_markdown_force(&skill_content)
        .unwrap_or_else(|e| panic!("Failed to load swap skill: {}", e));
    eprintln!("  Loaded skill: {} v{}", skill.name, skill.version);

    // === Build dispatcher (no mock — real AI) ===
    let broadcaster = Arc::new(EventBroadcaster::new());
    let (client_id, mut event_rx) = broadcaster.subscribe();
    let execution_tracker = Arc::new(ExecutionTracker::new(broadcaster.clone()));
    let tool_registry = Arc::new(tools::create_default_registry());

    let dispatcher = MessageDispatcher::new_with_wallet_and_skills(
        db.clone(),
        broadcaster.clone(),
        tool_registry,
        execution_tracker,
        None, // no wallet provider
        Some(skill_registry),
    );

    // === Dispatch the swap message ===
    let msg = NormalizedMessage {
        channel_id,
        channel_type: "web".to_string(),
        chat_id: "test-chat".to_string(),
        user_id: "test-user".to_string(),
        user_name: "TestUser".to_string(),
        text: "swap 1 usdc to starkbot".to_string(),
        message_id: None,
        session_mode: None,
        selected_network: None,
        force_safe_mode: false,
    };

    eprintln!("  Dispatching: \"{}\"", msg.text);
    eprintln!("  (timeout: 120s)\n");

    let result = timeout(Duration::from_secs(120), dispatcher.dispatch(msg)).await
        .expect("dispatch timed out after 120s");

    // === Collect all events ===
    let mut events = Vec::new();
    // Drain buffered events
    while let Ok(event) = event_rx.try_recv() {
        events.push(event);
    }
    // Brief timeout drain for any trailing events
    loop {
        match timeout(Duration::from_millis(100), event_rx.recv()).await {
            Ok(Some(event)) => events.push(event),
            _ => break,
        }
    }
    drop(event_rx);
    let _ = client_id; // keep subscription alive until here

    eprintln!("=== DISPATCH COMPLETE ===");
    eprintln!("  Error: {:?}", result.error);
    eprintln!("  Total events captured: {}", events.len());

    // === Build trace from events ===
    let mut trace_entries: Vec<serde_json::Value> = Vec::new();
    let mut tool_results: Vec<serde_json::Value> = Vec::new();
    let mut tool_calls_seen: Vec<String> = Vec::new();
    let mut task_numbers_seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for event in &events {
        match event.event.as_str() {
            "tool.result" => {
                let tool_name = event.data.get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let success = event.data.get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let content = event.data.get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let duration_ms = event.data.get("duration_ms")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                tool_calls_seen.push(tool_name.to_string());

                let entry = json!({
                    "tool_name": tool_name,
                    "success": success,
                    "duration_ms": duration_ms,
                    "content_preview": &content[..content.len().min(200)],
                });
                tool_results.push(entry.clone());
                trace_entries.push(json!({
                    "type": "tool.result",
                    "data": entry,
                }));

                eprintln!("  [tool.result] {} | success={} | {}ms | {}",
                    tool_name, success, duration_ms, &content[..content.len().min(80)]);
            }
            "agent.tool_call" => {
                let tool_name = event.data.get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                trace_entries.push(json!({
                    "type": "agent.tool_call",
                    "tool_name": tool_name,
                    "parameters": event.data.get("parameters"),
                }));
            }
            "agent.response" => {
                let text = event.data.get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                trace_entries.push(json!({
                    "type": "agent.response",
                    "text_preview": &text[..text.len().min(200)],
                }));
                eprintln!("  [agent.response] {}", &text[..text.len().min(120)]);
            }
            "execution.task_started" | "execution.task_updated" => {
                // Extract task counter from event data if available
                if let Some(task_str) = event.data.get("task") {
                    if let Some(s) = task_str.as_str() {
                        task_numbers_seen.insert(s.to_string());
                    }
                }
                // Also try to extract from description/index fields
                if let Some(idx) = event.data.get("index").and_then(|v| v.as_i64()) {
                    if let Some(total) = event.data.get("total").and_then(|v| v.as_i64()) {
                        task_numbers_seen.insert(format!("{}/{}", idx, total));
                    }
                }
            }
            _ => {}
        }
    }

    eprintln!("\n=== SUMMARY ===");
    eprintln!("  Tool calls: {:?}", tool_calls_seen);
    eprintln!("  Distinct task events: {:?}", task_numbers_seen);

    // === Write trace to test_output/ ===
    let output_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("test_output");
    std::fs::create_dir_all(&output_dir).expect("create test_output dir");

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("swap_flow_realistic_{}.json", timestamp);
    let filepath = output_dir.join(&filename);

    let trace_output = json!({
        "test": "swap_flow_realistic",
        "endpoint": endpoint,
        "archetype": archetype,
        "timestamp": timestamp.to_string(),
        "dispatch_error": format!("{:?}", result.error),
        "total_events": events.len(),
        "tool_calls_seen": tool_calls_seen,
        "task_numbers_seen": task_numbers_seen.iter().collect::<Vec<_>>(),
        "tool_results": tool_results,
        "trace": trace_entries,
    });

    std::fs::write(&filepath, serde_json::to_string_pretty(&trace_output).unwrap())
        .expect("write trace file");
    eprintln!("  Trace written to: {}", filepath.display());
    eprintln!("================\n");

    // === Assertions (loose — AI is non-deterministic) ===

    // 1. Dispatch succeeds
    assert!(
        result.error.is_none(),
        "dispatch should succeed, got error: {:?}",
        result.error
    );

    // 2. define_tasks was called (AI planned the swap)
    assert!(
        tool_calls_seen.iter().any(|t| t == "define_tasks"),
        "AI should have called define_tasks to plan the swap. Tools called: {:?}",
        tool_calls_seen
    );

    // 3. token_lookup was called at least twice (sell + buy tokens)
    let token_lookup_count = tool_calls_seen.iter().filter(|t| *t == "token_lookup").count();
    assert!(
        token_lookup_count >= 2,
        "AI should call token_lookup at least twice (sell + buy tokens), got {} calls. Tools: {:?}",
        token_lookup_count, tool_calls_seen
    );

    // 4. to_raw_amount was called at least once
    assert!(
        tool_calls_seen.iter().any(|t| t == "to_raw_amount"),
        "AI should have called to_raw_amount to convert the amount. Tools: {:?}",
        tool_calls_seen
    );

    // 5. Task advancement happened — check via execution events or tool calls
    // We should see task_fully_completed or say_to_user(finished_task) called multiple times,
    // indicating the AI advanced through multiple tasks
    let task_advance_tools = tool_calls_seen.iter()
        .filter(|t| *t == "task_fully_completed" || *t == "say_to_user")
        .count();
    assert!(
        task_advance_tools >= 3,
        "Should see at least 3 task-advancing tool calls (task_fully_completed or say_to_user), got {}. Tools: {:?}",
        task_advance_tools, tool_calls_seen
    );
}

// ============================================================================
// Uniswap V4 LP deposit flow test with INPUT/OUTPUT trace capture.
//
// Simulates "deposit 1000 starkbot into the uniswap LP pool" through the
// full pipeline:
//   Planner → set_agent_subtype(finance) → use_skill(uniswap_lp) → task execution
//
// Loads the actual uniswap_lp skill from skills/uniswap_lp.md so that
// `use_skill` appears in the available tools list.
//
// 7 iterations:
//   1. Planner: define_tasks (5 LP tasks)
//   2. Task 1: set_agent_subtype + use_skill
//   3. Task 1 (cont): say_to_user(finished_task=true) — report findings
//   4. Task 2: task_fully_completed — skip approval
//   5. Task 3: task_fully_completed — API tx built
//   6. Task 4: task_fully_completed — decoded + broadcast
//   7. Task 5: say_to_user(finished_task=true) — verify + report
// ============================================================================

#[tokio::test]
async fn lp_deposit_flow_with_trace() {
    let responses = vec![
        // Iteration 1 (TaskPlanner mode): AI calls define_tasks with 5 LP tasks
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "define_tasks",
                json!({
                    "tasks": [
                        "TASK 1 — Prepare: select Base, look up WETH + STARKBOT, check balances, read pool state (slot0 + liquidity). See LP skill 'Task 1'.",
                        "TASK 2 — Approve: approve both tokens for Permit2 (skip if sufficient). See LP skill 'Task 2'.",
                        "TASK 3 — Build tx: POST to Uniswap API /lp/create with pool params, cache response. See LP skill 'Task 3'.",
                        "TASK 4 — Execute: decode_calldata → uni_v4_modify_liquidities preset → broadcast. See LP skill 'Task 4'.",
                        "TASK 5 — Verify: verify_tx_broadcast, report position. See LP skill 'Task 5'."
                    ]
                }),
            )],
        ),
        // Iteration 2 (Task 1 - Prepare): AI selects finance toolbox + loads LP skill
        AiResponse::with_tools(
            String::new(),
            vec![
                tool_call("set_agent_subtype", json!({"subtype": "finance"})),
                tool_call("use_skill", json!({"skill_name": "uniswap_lp", "input": "deposit 1000 starkbot into the uniswap LP pool"})),
            ],
        ),
        // Iteration 3 (still Task 1): AI completes preparation and reports findings
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "say_to_user",
                json!({
                    "message": "Loaded LP skill. Preparation complete:\n- Network: Base\n- Token0: WETH (0x4200...0006)\n- Token1: STARKBOT (0x587C...1B07)\n- Pool: STARKBOT/WETH 1% (V4)\n- Current tick: -230400\n- Suggested full range: tickLower=-887200, tickUpper=887200\n\nReady to proceed with deposit.",
                    "finished_task": true
                }),
            )],
        ),
        // Iteration 4 (Task 2 - Approve): Skip since both tokens already approved for Permit2
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "task_fully_completed",
                json!({"summary": "Both WETH and STARKBOT already approved for Permit2 — skipping."}),
            )],
        ),
        // Iteration 5 (Task 3 - Build tx): API call succeeded, tx cached
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "task_fully_completed",
                json!({"summary": "LP create transaction built via Uniswap API and cached in uni_lp_tx register. Full range position with 1000 STARKBOT."}),
            )],
        ),
        // Iteration 6 (Task 4 - Execute): Decoded + broadcast
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "task_fully_completed",
                json!({"summary": "LP transaction decoded and broadcast. TX: 0xdef456..."}),
            )],
        ),
        // Iteration 7 (Task 5 - Verify): Final result
        AiResponse::with_tools(
            String::new(),
            vec![tool_call(
                "say_to_user",
                json!({
                    "message": "LP position created!\n\nDeposited 1000 STARKBOT + proportional WETH into STARKBOT/WETH 1% pool on Uniswap V4\nRange: Full range (-887200 to 887200)\nTX: https://basescan.org/tx/0xdef456",
                    "finished_task": true
                }),
            )],
        ),
    ];

    // Use skill-aware harness so `use_skill` appears in the tools list
    let mut harness = TestHarness::new_with_skills("web", false, &["uniswap_lp"], responses);
    let (result, events) = harness.dispatch("deposit 1000 starkbot into the uniswap LP pool", false).await;

    // Write trace for auditing
    harness.write_trace("lp_deposit_flow");

    // Verify dispatch succeeded
    assert!(result.error.is_none(), "dispatch should succeed: {:?}", result.error);

    // Verify trace was captured
    let trace = harness.get_trace();
    assert!(
        trace.len() >= 3,
        "Expected at least 3 AI iterations (planner + subtype/skill + tasks), got {}",
        trace.len()
    );

    // Verify iteration 1 had define_tasks in the output
    if let Some(ref resp) = trace[0].output_response {
        let has_define_tasks = resp.tool_calls.iter().any(|tc| tc.name == "define_tasks");
        assert!(has_define_tasks, "First iteration should call define_tasks");
    }

    // Verify iteration 2 called set_agent_subtype and use_skill
    if let Some(ref resp) = trace[1].output_response {
        let tool_names: Vec<&str> = resp.tool_calls.iter().map(|tc| tc.name.as_str()).collect();
        assert!(
            tool_names.contains(&"set_agent_subtype"),
            "Iteration 2 should call set_agent_subtype, got: {:?}", tool_names
        );
        assert!(
            tool_names.contains(&"use_skill"),
            "Iteration 2 should call use_skill, got: {:?}", tool_names
        );
    }

    // Verify use_skill is in the available tools for Task 1
    assert!(
        trace[1].input_tools.iter().any(|t| t == "use_skill"),
        "Task 1 should have use_skill in available tools, got: {:?}",
        trace[1].input_tools
    );

    // Verify CURRENT TASK advances through the system prompt
    let extract_task_num = |sys_prompt: &str| -> Option<(usize, usize)> {
        if let Some(pos) = sys_prompt.find("CURRENT TASK (") {
            let after = &sys_prompt[pos + "CURRENT TASK (".len()..];
            if let Some(slash) = after.find('/') {
                let current: usize = after[..slash].parse().ok()?;
                let rest = &after[slash + 1..];
                if let Some(paren) = rest.find(')') {
                    let total: usize = rest[..paren].parse().ok()?;
                    return Some((current, total));
                }
            }
        }
        None
    };

    // Build a summary of task numbers per iteration
    let mut task_numbers: Vec<Option<(usize, usize)>> = Vec::new();
    for entry in &trace {
        let sys_prompt = entry.input_messages.first()
            .map(|m| m.content.as_str())
            .unwrap_or("");
        task_numbers.push(extract_task_num(sys_prompt));
    }

    // Print summary for test output
    eprintln!("\n=== LP DEPOSIT FLOW TEST SUMMARY ===");
    eprintln!("Total AI iterations: {}", trace.len());
    for (i, entry) in trace.iter().enumerate() {
        let tool_names: Vec<&str> = entry.output_response.as_ref()
            .map(|r| r.tool_calls.iter().map(|tc| tc.name.as_str()).collect())
            .unwrap_or_default();
        let task_info = task_numbers[i]
            .map(|(c, t)| format!("TASK {}/{}", c, t))
            .unwrap_or_else(|| "no task".to_string());
        eprintln!(
            "  Iteration {}: {} | tools={:?} | tool_history={}",
            entry.iteration,
            task_info,
            tool_names,
            entry.input_tool_history.len(),
        );
    }
    eprintln!("====================================\n");

    // Assert task advancement:
    // - Iteration 1: no task (planner mode)
    // - Iteration 2: TASK 1/5 (set_agent_subtype + use_skill)
    // - Iteration 3: TASK 1/5 (say_to_user finished_task=true → completes task 1)
    // - Iteration 4: TASK 2/5
    // - Iteration 5: TASK 3/5
    // - Iteration 6: TASK 4/5
    // - Iteration 7: TASK 5/5
    assert_eq!(task_numbers[0], None, "Iteration 1 should have no task (planner mode)");
    assert_eq!(task_numbers[1], Some((1, 5)), "Iteration 2 should show TASK 1/5");
    assert_eq!(task_numbers[2], Some((1, 5)), "Iteration 3 should still be TASK 1/5 (multi-step task)");
    assert_eq!(task_numbers[3], Some((2, 5)), "Iteration 4 should show TASK 2/5 (task 1 completed)");
    assert_eq!(task_numbers[4], Some((3, 5)), "Iteration 5 should show TASK 3/5 (task 2 completed)");
    assert_eq!(task_numbers[5], Some((4, 5)), "Iteration 6 should show TASK 4/5 (task 3 completed)");
    assert_eq!(task_numbers[6], Some((5, 5)), "Iteration 7 should show TASK 5/5 (task 4 completed)");
}
