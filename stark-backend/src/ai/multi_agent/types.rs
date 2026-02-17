//! Agent types

use serde::{Deserialize, Serialize};

use crate::tools::types::ToolGroup;

// =====================================================
// Agent Subtype Config (dynamic, config-driven subtypes)
// =====================================================

/// A fully-configurable agent subtype definition.
/// Stored in DB, loaded from `config/defaultagents.ron` on first boot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSubtypeConfig {
    pub key: String,
    pub label: String,
    pub emoji: String,
    pub description: String,
    /// Tool group keys (e.g. ["finance"] or ["development", "exec"])
    pub tool_groups: Vec<String>,
    /// Skill tags this subtype can access
    pub skill_tags: Vec<String>,
    /// Additional tools added on top of group-based tools.
    /// These are included regardless of which tool groups are active.
    #[serde(default)]
    pub additional_tools: Vec<String>,
    /// The prompt text returned when this subtype is activated
    pub prompt: String,
    pub sort_order: i32,
    pub enabled: bool,
    /// Maximum iterations for agents using this subtype (default 90)
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    /// If true, skip TaskPlanner mode and go straight to Assistant mode
    #[serde(default)]
    pub skip_task_planner: bool,
    /// Alternative names that resolve to this subtype key (e.g. "crypto" → "finance")
    #[serde(default)]
    pub aliases: Vec<String>,
}

fn default_max_iterations() -> u32 {
    90
}

/// Global registry of agent subtype configs, loaded at startup from DB.
static SUBTYPE_REGISTRY: std::sync::OnceLock<parking_lot::RwLock<Vec<AgentSubtypeConfig>>> =
    std::sync::OnceLock::new();

fn registry() -> &'static parking_lot::RwLock<Vec<AgentSubtypeConfig>> {
    SUBTYPE_REGISTRY.get_or_init(|| parking_lot::RwLock::new(Vec::new()))
}

/// Load agent subtype configs into the global registry (called at startup).
pub fn load_subtype_registry(configs: Vec<AgentSubtypeConfig>) {
    let mut reg = registry().write();
    *reg = configs;
    log::info!("[SUBTYPE_REGISTRY] Loaded {} agent subtypes", reg.len());
}

/// Get a single subtype config by key.
pub fn get_subtype_config(key: &str) -> Option<AgentSubtypeConfig> {
    let reg = registry().read();
    reg.iter().find(|c| c.key == key).cloned()
}

/// Get all subtype configs (enabled only, sorted by sort_order).
pub fn all_subtype_configs() -> Vec<AgentSubtypeConfig> {
    let reg = registry().read();
    let mut configs: Vec<_> = reg.iter().filter(|c| c.enabled).cloned().collect();
    configs.sort_by_key(|c| c.sort_order);
    configs
}

/// Get all subtype configs including disabled ones (for admin UI).
pub fn all_subtype_configs_unfiltered() -> Vec<AgentSubtypeConfig> {
    let reg = registry().read();
    let mut configs: Vec<_> = reg.iter().cloned().collect();
    configs.sort_by_key(|c| c.sort_order);
    configs
}

/// Get the default subtype key (lowest sort_order among enabled subtypes).
/// Returns empty string if the registry is empty or not loaded yet.
pub fn default_subtype_key() -> String {
    all_subtype_configs()
        .first()
        .map(|c| c.key.clone())
        .unwrap_or_default()
}

/// Check if the registry has been populated.
pub fn subtype_registry_loaded() -> bool {
    SUBTYPE_REGISTRY.get().map(|r| !r.read().is_empty()).unwrap_or(false)
}

/// Load agent subtypes from `config/defaultagents.ron`.
/// Panics if the file is missing or malformed — the RON config is required.
pub fn load_default_agent_subtypes_from_file(config_dir: &std::path::Path) -> Vec<AgentSubtypeConfig> {
    let path = config_dir.join("defaultagents.ron");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("[SUBTYPE] config/defaultagents.ron is required but could not be read: {}", e));
    let configs: Vec<AgentSubtypeConfig> = ron::from_str(&content)
        .unwrap_or_else(|e| panic!("[SUBTYPE] Failed to parse {}: {}", path.display(), e));
    log::info!("[SUBTYPE] Loaded {} subtypes from {}", configs.len(), path.display());
    configs
}

/// Load subtypes from the repo's config/defaultagents.ron (for tests).
/// Walks up from CARGO_MANIFEST_DIR to find the config directory.
#[cfg(test)]
pub fn load_test_subtypes() -> Vec<AgentSubtypeConfig> {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // config/ is at the repo root, one level above stark-backend/
    let config_dir = manifest_dir.parent().unwrap().join("config");
    load_default_agent_subtypes_from_file(&config_dir)
}

// =====================================================
// Subtype free functions (replace former AgentSubtype enum methods)
// =====================================================

/// Resolve a user-provided string to a canonical subtype key.
/// Checks exact key match first, then scans aliases across the registry.
/// Returns `None` if no match is found.
pub fn resolve_subtype_key(input: &str) -> Option<String> {
    let lower = input.to_lowercase();

    // 1. Exact key match
    if get_subtype_config(&lower).is_some() {
        return Some(lower);
    }

    // 2. Alias match — scan all configs
    let reg = registry().read();
    for config in reg.iter() {
        if config.aliases.iter().any(|a| a.to_lowercase() == lower) {
            return Some(config.key.clone());
        }
    }

    None
}

/// Human-readable label for a subtype key. Falls back to the key itself.
pub fn subtype_label(key: &str) -> String {
    get_subtype_config(key)
        .map(|c| c.label)
        .unwrap_or_else(|| key.to_string())
}

/// Emoji for a subtype key. Falls back to "❓".
pub fn subtype_emoji(key: &str) -> String {
    get_subtype_config(key)
        .map(|c| c.emoji)
        .unwrap_or_else(|| "❓".to_string())
}

/// Tool groups allowed for a subtype key. Always includes System.
pub fn allowed_tool_groups_for_key(key: &str) -> Vec<ToolGroup> {
    let mut groups = Vec::new();
    if let Some(config) = get_subtype_config(key) {
        for g in &config.tool_groups {
            if let Some(tg) = ToolGroup::from_str(g) {
                if !groups.contains(&tg) {
                    groups.push(tg);
                }
            }
        }
    }
    groups
}

/// Skill tags allowed for a subtype key.
pub fn allowed_skill_tags_for_key(key: &str) -> Vec<String> {
    get_subtype_config(key)
        .map(|c| c.skill_tags)
        .unwrap_or_default()
}

/// All enabled subtype keys from the registry.
pub fn all_subtype_keys() -> Vec<String> {
    all_subtype_configs().iter().map(|c| c.key.clone()).collect()
}

// =====================================================
// Task Planner Types
// =====================================================

/// Status of a planner task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

impl Default for TaskStatus {
    fn default() -> Self {
        TaskStatus::Pending
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::InProgress => write!(f, "in_progress"),
            TaskStatus::Completed => write!(f, "completed"),
        }
    }
}

/// A task created by the task planner
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerTask {
    pub id: u32,
    pub description: String,
    pub status: TaskStatus,
    /// If set, this task auto-completes when the named tool succeeds
    #[serde(default)]
    pub auto_complete_tool: Option<String>,
}

impl PlannerTask {
    pub fn new(id: u32, description: String) -> Self {
        Self {
            id,
            description,
            status: TaskStatus::Pending,
            auto_complete_tool: None,
        }
    }
}

/// Queue of tasks to be executed
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskQueue {
    pub tasks: Vec<PlannerTask>,
    pub current_task_idx: Option<usize>,
}

impl TaskQueue {
    /// Create a new task queue from a list of task descriptions
    pub fn from_descriptions(descriptions: Vec<String>) -> Self {
        let tasks = descriptions
            .into_iter()
            .enumerate()
            .map(|(i, desc)| PlannerTask::new((i + 1) as u32, desc))
            .collect();
        Self {
            tasks,
            current_task_idx: None,
        }
    }

    /// Get the current task being worked on
    pub fn current_task(&self) -> Option<&PlannerTask> {
        self.current_task_idx.and_then(|idx| self.tasks.get(idx))
    }

    /// Get the current task mutably
    pub fn current_task_mut(&mut self) -> Option<&mut PlannerTask> {
        self.current_task_idx.and_then(|idx| self.tasks.get_mut(idx))
    }

    /// Pop the next pending task and mark it as in progress
    pub fn pop_next(&mut self) -> Option<&PlannerTask> {
        // Find the first pending task
        let next_idx = self.tasks.iter().position(|t| t.status == TaskStatus::Pending)?;
        self.tasks[next_idx].status = TaskStatus::InProgress;
        self.current_task_idx = Some(next_idx);
        self.tasks.get(next_idx)
    }

    /// Mark the current task as completed
    pub fn complete_current(&mut self) -> Option<u32> {
        if let Some(idx) = self.current_task_idx {
            if let Some(task) = self.tasks.get_mut(idx) {
                task.status = TaskStatus::Completed;
                let task_id = task.id;
                self.current_task_idx = None;
                return Some(task_id);
            }
        }
        None
    }

    /// Check if all tasks are complete
    pub fn all_complete(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.iter().all(|t| t.status == TaskStatus::Completed)
    }

    /// Get the total number of tasks
    pub fn total(&self) -> usize {
        self.tasks.len()
    }

    /// Get the number of completed tasks
    pub fn completed_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.status == TaskStatus::Completed).count()
    }

    /// Check if the queue is empty (no tasks defined)
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Delete a task by ID. Returns true if the task was found and deleted.
    /// If the deleted task was the current one (in_progress), clears current_task_idx.
    pub fn delete_task(&mut self, task_id: u32) -> bool {
        if let Some(idx) = self.tasks.iter().position(|t| t.id == task_id) {
            // If deleting the current task, clear the index
            if self.current_task_idx == Some(idx) {
                self.current_task_idx = None;
            } else if let Some(curr_idx) = self.current_task_idx {
                // If deleting a task before the current one, adjust the index
                if idx < curr_idx {
                    self.current_task_idx = Some(curr_idx - 1);
                }
            }
            self.tasks.remove(idx);
            true
        } else {
            false
        }
    }

    /// Get a task by ID
    pub fn get_task(&self, task_id: u32) -> Option<&PlannerTask> {
        self.tasks.iter().find(|t| t.id == task_id)
    }

    /// Insert new tasks right after the current task (before remaining pending tasks).
    /// Returns the IDs of the newly created tasks.
    pub fn insert_after_current(&mut self, descriptions: Vec<String>) -> Vec<u32> {
        let max_id = self.tasks.iter().map(|t| t.id).max().unwrap_or(0);
        let insert_idx = match self.current_task_idx {
            Some(idx) => idx + 1,
            None => 0,
        };

        let mut new_ids = Vec::new();
        for (i, desc) in descriptions.iter().enumerate() {
            let new_id = max_id + (i as u32) + 1;
            self.tasks.insert(insert_idx + i, PlannerTask::new(new_id, desc.clone()));
            new_ids.push(new_id);
        }

        // Adjust current_task_idx: since we inserted AFTER it, it doesn't change.
        // But if there was no current task, nothing to adjust.
        new_ids
    }

    /// System/meta tool names excluded from auto-complete matching
    const AUTO_COMPLETE_EXCLUDED_TOOLS: &'static [&'static str] = &[
        "say_to_user",
        "task_fully_completed",
        "define_tasks",
        "set_agent_subtype",
        "add_task",
        "ask_user",
        "spawn_subagent",
        "spawn_subagents",
        "subagent_status",
        "use_skill",
        "manage_skills",
    ];

    /// Create a task queue with auto-complete tool matching.
    /// For each task description, scans for tool names (case-insensitive substring).
    /// If multiple match, picks the longest (most specific).
    /// System/meta tools are excluded from matching.
    pub fn from_descriptions_with_tool_matching(
        descriptions: Vec<String>,
        tool_names: &[String],
    ) -> Self {
        let tasks = descriptions
            .into_iter()
            .enumerate()
            .map(|(i, desc)| {
                let desc_lower = desc.to_lowercase();
                let mut best_match: Option<&String> = None;
                let mut best_len = 0;
                for name in tool_names {
                    // Skip system/meta tools
                    if Self::AUTO_COMPLETE_EXCLUDED_TOOLS.contains(&name.as_str()) {
                        continue;
                    }
                    let name_lower = name.to_lowercase();
                    if desc_lower.contains(&name_lower) && name.len() > best_len {
                        best_match = Some(name);
                        best_len = name.len();
                    }
                }
                let mut task = PlannerTask::new((i + 1) as u32, desc);
                task.auto_complete_tool = best_match.cloned();
                if let Some(ref tool) = task.auto_complete_tool {
                    log::info!(
                        "[TASK_QUEUE] Task {}: auto_complete_tool = '{}'",
                        task.id,
                        tool
                    );
                }
                task
            })
            .collect();
        Self {
            tasks,
            current_task_idx: None,
        }
    }

    /// Append new tasks at the end of the queue.
    /// Returns the IDs of the newly created tasks.
    pub fn append_tasks(&mut self, descriptions: Vec<String>) -> Vec<u32> {
        let max_id = self.tasks.iter().map(|t| t.id).max().unwrap_or(0);
        let mut new_ids = Vec::new();
        for (i, desc) in descriptions.iter().enumerate() {
            let new_id = max_id + (i as u32) + 1;
            self.tasks.push(PlannerTask::new(new_id, desc.clone()));
            new_ids.push(new_id);
        }
        new_ids
    }
}

#[cfg(test)]
mod task_queue_tests {
    use super::*;

    #[test]
    fn test_insert_after_current() {
        let mut queue = TaskQueue::from_descriptions(vec![
            "Task A".to_string(),
            "Task B".to_string(),
            "Task C".to_string(),
        ]);

        // Pop first task (Task A becomes current at idx 0)
        queue.pop_next();
        assert_eq!(queue.current_task().unwrap().description, "Task A");

        // Insert two tasks after current
        let ids = queue.insert_after_current(vec![
            "Inserted 1".to_string(),
            "Inserted 2".to_string(),
        ]);
        assert_eq!(ids.len(), 2);

        // Queue should be: [Task A (current), Inserted 1, Inserted 2, Task B, Task C]
        assert_eq!(queue.tasks.len(), 5);
        assert_eq!(queue.tasks[0].description, "Task A");
        assert_eq!(queue.tasks[1].description, "Inserted 1");
        assert_eq!(queue.tasks[2].description, "Inserted 2");
        assert_eq!(queue.tasks[3].description, "Task B");
        assert_eq!(queue.tasks[4].description, "Task C");

        // Current task should still be Task A
        assert_eq!(queue.current_task().unwrap().description, "Task A");
    }

    #[test]
    fn test_insert_front_ordering_for_swap() {
        // Simulates the swap skill: add swap task first, then approval task at front
        let mut queue = TaskQueue::from_descriptions(vec!["Prepare tokens".to_string()]);
        queue.pop_next(); // "Prepare tokens" is current

        // Add "Execute swap" at front (after current)
        queue.insert_after_current(vec!["Execute swap".to_string()]);
        // Queue: [Prepare tokens (current), Execute swap, ...]

        // Add "Approve tokens" at front (after current, pushing Execute swap further)
        queue.insert_after_current(vec!["Approve tokens".to_string()]);
        // Queue: [Prepare tokens (current), Approve tokens, Execute swap]

        assert_eq!(queue.tasks[0].description, "Prepare tokens");
        assert_eq!(queue.tasks[1].description, "Approve tokens");
        assert_eq!(queue.tasks[2].description, "Execute swap");

        // Complete current task, pop next — should be "Approve tokens"
        queue.complete_current();
        let next = queue.pop_next().unwrap();
        assert_eq!(next.description, "Approve tokens");

        // Complete approval, pop next — should be "Execute swap"
        queue.complete_current();
        let next = queue.pop_next().unwrap();
        assert_eq!(next.description, "Execute swap");
    }

    #[test]
    fn test_append_tasks() {
        let mut queue = TaskQueue::from_descriptions(vec!["Task A".to_string()]);
        queue.pop_next();

        let ids = queue.append_tasks(vec!["Task B".to_string(), "Task C".to_string()]);
        assert_eq!(ids.len(), 2);

        assert_eq!(queue.tasks[0].description, "Task A");
        assert_eq!(queue.tasks[1].description, "Task B");
        assert_eq!(queue.tasks[2].description, "Task C");
    }

    #[test]
    fn test_insert_with_no_current_task() {
        let mut queue = TaskQueue::default();
        assert!(queue.is_empty());

        // Insert when no current task — inserts at position 0
        let ids = queue.insert_after_current(vec!["New task".to_string()]);
        assert_eq!(ids.len(), 1);
        assert_eq!(queue.tasks[0].description, "New task");
    }

    // =========================================================================
    // Auto-complete tool matching tests
    // =========================================================================

    #[test]
    fn test_auto_complete_basic_match() {
        let tools = vec!["token_lookup".to_string(), "web_fetch".to_string()];
        let queue = TaskQueue::from_descriptions_with_tool_matching(
            vec!["Look up the token price using token_lookup".to_string()],
            &tools,
        );
        assert_eq!(queue.tasks[0].auto_complete_tool, Some("token_lookup".to_string()));
    }

    #[test]
    fn test_auto_complete_no_match() {
        let tools = vec!["token_lookup".to_string(), "web_fetch".to_string()];
        let queue = TaskQueue::from_descriptions_with_tool_matching(
            vec!["Tell the user the final answer".to_string()],
            &tools,
        );
        assert_eq!(queue.tasks[0].auto_complete_tool, None);
    }

    #[test]
    fn test_auto_complete_longest_wins() {
        let tools = vec![
            "web3".to_string(),
            "web3_preset_function_call".to_string(),
        ];
        let queue = TaskQueue::from_descriptions_with_tool_matching(
            vec!["Call web3_preset_function_call to get balance".to_string()],
            &tools,
        );
        assert_eq!(
            queue.tasks[0].auto_complete_tool,
            Some("web3_preset_function_call".to_string())
        );
    }

    #[test]
    fn test_auto_complete_case_insensitive() {
        let tools = vec!["token_lookup".to_string()];
        let queue = TaskQueue::from_descriptions_with_tool_matching(
            vec!["Use TOKEN_LOOKUP to find the price".to_string()],
            &tools,
        );
        assert_eq!(queue.tasks[0].auto_complete_tool, Some("token_lookup".to_string()));
    }

    #[test]
    fn test_auto_complete_excludes_system_tools() {
        let tools = vec![
            "say_to_user".to_string(),
            "task_fully_completed".to_string(),
            "define_tasks".to_string(),
            "set_agent_subtype".to_string(),
            "token_lookup".to_string(),
        ];
        let queue = TaskQueue::from_descriptions_with_tool_matching(
            vec![
                "Use say_to_user to respond".to_string(),
                "Call define_tasks to plan".to_string(),
                "Look up token_lookup".to_string(),
            ],
            &tools,
        );
        assert_eq!(queue.tasks[0].auto_complete_tool, None);
        assert_eq!(queue.tasks[1].auto_complete_tool, None);
        assert_eq!(queue.tasks[2].auto_complete_tool, Some("token_lookup".to_string()));
    }

    #[test]
    fn test_auto_complete_serde_backward_compat() {
        // Old JSON without auto_complete_tool field should deserialize fine
        let json = r#"{"id": 1, "description": "test", "status": "pending"}"#;
        let task: PlannerTask = serde_json::from_str(json).unwrap();
        assert_eq!(task.auto_complete_tool, None);
    }
}

/// The current mode of the agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    /// Task planner mode - first iteration only, breaks down request into tasks
    TaskPlanner,
    /// Active assistant mode - handles tasks one at a time
    Assistant,
}

impl Default for AgentMode {
    fn default() -> Self {
        AgentMode::TaskPlanner
    }
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentMode::TaskPlanner => write!(f, "task_planner"),
            AgentMode::Assistant => write!(f, "assistant"),
        }
    }
}

impl AgentMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "task_planner" | "taskplanner" | "planner" => Some(AgentMode::TaskPlanner),
            "assistant" | "explore" | "plan" | "perform" | "execute" => Some(AgentMode::Assistant),
            _ => None,
        }
    }

    /// Check if skills are available in this mode
    pub fn allows_skills(&self) -> bool {
        matches!(self, AgentMode::Assistant)
    }

    /// Check if action tools (swap, transfer, etc.) are available in this mode
    pub fn allows_action_tools(&self) -> bool {
        matches!(self, AgentMode::Assistant)
    }

    /// Human-readable label for UI display
    pub fn label(&self) -> &'static str {
        match self {
            AgentMode::TaskPlanner => "Planning",
            AgentMode::Assistant => "Assistant",
        }
    }
}

/// Context accumulated during the agent session
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentContext {
    /// Original user request
    pub original_request: String,

    /// Notes gathered during the session
    pub exploration_notes: Vec<String>,

    /// Current mode (TaskPlanner for first iteration, then Assistant)
    pub mode: AgentMode,

    /// Current agent subtype/specialization key (None = not selected, Some("finance") = selected)
    #[serde(default)]
    pub subtype: Option<String>,

    /// Number of iterations in current session
    pub mode_iterations: u32,

    /// Total iterations
    pub total_iterations: u32,

    /// Scratchpad for agent notes
    pub scratchpad: String,

    /// Currently active skill context
    #[serde(default)]
    pub active_skill: Option<ActiveSkill>,

    /// Total actual tool calls made (excludes orchestrator tools)
    #[serde(default)]
    pub actual_tool_calls: u32,

    /// Number of times the agent tried to respond without calling tools
    #[serde(default)]
    pub no_tool_warnings: u32,

    /// Context saved when waiting for user response (e.g., from ask_user tool).
    /// Contains a summary of what tool calls were made before asking the user,
    /// so the AI can continue where it left off when the user responds.
    #[serde(default)]
    pub waiting_for_user_context: Option<String>,

    /// Task queue for the current session (populated after planner runs)
    #[serde(default)]
    pub task_queue: TaskQueue,

    /// Whether the planner phase has completed
    #[serde(default)]
    pub planner_completed: bool,

    /// Currently selected network from UI (e.g., "base", "polygon", "mainnet")
    /// Used as default for web3 operations unless user explicitly specifies otherwise
    #[serde(default)]
    pub selected_network: Option<String>,
}

/// Active skill context that persists across turns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSkill {
    /// Name of the skill
    pub name: String,
    /// Skill instructions/body
    pub instructions: String,
    /// When the skill was activated
    pub activated_at: String,
    /// Number of actual tool calls made since this skill was activated
    #[serde(default)]
    pub tool_calls_made: u32,
    /// Tools required by this skill - these are force-included in the toolset
    /// regardless of tool profile/config restrictions
    #[serde(default)]
    pub requires_tools: Vec<String>,
}

/// Mode transition (kept for API compatibility)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeTransition {
    pub from: AgentMode,
    pub to: AgentMode,
    pub reason: String,
}

// =====================================================
// Sub-Agent System Types
// =====================================================

/// Status of a sub-agent execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentStatus {
    /// Waiting to be started
    Pending,
    /// Currently executing
    Running,
    /// Successfully completed
    Completed,
    /// Failed with an error
    Failed,
    /// Timed out during execution
    TimedOut,
    /// Cancelled by user or system
    Cancelled,
}

impl Default for SubAgentStatus {
    fn default() -> Self {
        SubAgentStatus::Pending
    }
}

impl std::fmt::Display for SubAgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubAgentStatus::Pending => write!(f, "pending"),
            SubAgentStatus::Running => write!(f, "running"),
            SubAgentStatus::Completed => write!(f, "completed"),
            SubAgentStatus::Failed => write!(f, "failed"),
            SubAgentStatus::TimedOut => write!(f, "timed_out"),
            SubAgentStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl SubAgentStatus {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pending" => Some(SubAgentStatus::Pending),
            "running" => Some(SubAgentStatus::Running),
            "completed" => Some(SubAgentStatus::Completed),
            "failed" => Some(SubAgentStatus::Failed),
            "timed_out" | "timedout" => Some(SubAgentStatus::TimedOut),
            "cancelled" | "canceled" => Some(SubAgentStatus::Cancelled),
            _ => None,
        }
    }

    /// Check if this status represents a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(self, SubAgentStatus::Completed | SubAgentStatus::Failed | SubAgentStatus::TimedOut | SubAgentStatus::Cancelled)
    }
}

/// Context for a sub-agent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentContext {
    /// Unique identifier for this sub-agent
    pub id: String,
    /// Parent session ID that spawned this sub-agent
    pub parent_session_id: i64,
    /// Channel ID where the sub-agent was spawned
    pub parent_channel_id: i64,
    /// Session ID for this sub-agent's conversation (created when execution starts)
    pub session_id: Option<i64>,
    /// Human-readable label for this sub-agent
    pub label: String,
    /// The task/prompt for the sub-agent to work on
    pub task: String,
    /// Current execution status
    pub status: SubAgentStatus,
    /// Optional model override (uses parent's model if None)
    pub model_override: Option<String>,
    /// Timeout in seconds for this sub-agent
    pub timeout_secs: u64,
    /// Result of the execution (set on completion)
    pub result: Option<String>,
    /// Error message (set on failure)
    pub error: Option<String>,
    /// When the sub-agent was created
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// When the sub-agent completed (success, failure, or timeout)
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Additional context passed from the parent
    pub context: Option<String>,
    /// Thinking level for Claude models
    pub thinking_level: Option<String>,
    /// If true, restrict to read-only tools (for safe parallel research)
    #[serde(default)]
    pub read_only: bool,
    /// ID of the parent sub-agent that spawned this one (None for top-level)
    #[serde(default)]
    pub parent_subagent_id: Option<String>,
    /// Depth in the sub-agent tree (0 for top-level, 1 for child of top-level, etc.)
    #[serde(default)]
    pub depth: u32,
    /// Agent subtype key (e.g. "superouter", "finance") — determines which tools/skills are available
    #[serde(default)]
    pub agent_subtype: Option<String>,
}

impl SubAgentContext {
    /// Create a new sub-agent context
    pub fn new(
        id: String,
        parent_session_id: i64,
        parent_channel_id: i64,
        label: String,
        task: String,
        timeout_secs: u64,
    ) -> Self {
        Self {
            id,
            parent_session_id,
            parent_channel_id,
            session_id: None,
            label,
            task,
            status: SubAgentStatus::Pending,
            model_override: None,
            timeout_secs,
            result: None,
            error: None,
            started_at: chrono::Utc::now(),
            completed_at: None,
            context: None,
            thinking_level: None,
            read_only: false,
            parent_subagent_id: None,
            depth: 0,
            agent_subtype: None,
        }
    }

    /// Set the model override
    pub fn with_model(mut self, model: Option<String>) -> Self {
        self.model_override = model;
        self
    }

    /// Set additional context
    pub fn with_context(mut self, context: Option<String>) -> Self {
        self.context = context;
        self
    }

    /// Set thinking level
    pub fn with_thinking(mut self, level: Option<String>) -> Self {
        self.thinking_level = level;
        self
    }

    /// Set read-only mode (restricts to read-only tools for safe research)
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Set agent subtype (determines which tools/skills are available)
    pub fn with_agent_subtype(mut self, subtype: Option<String>) -> Self {
        self.agent_subtype = subtype;
        self
    }

    /// Set parent sub-agent identity (for recursive sub-agent tracking)
    /// depth is set to parent_depth + 1
    pub fn with_parent_subagent(mut self, parent_id: String, parent_depth: u32) -> Self {
        self.parent_subagent_id = Some(parent_id);
        self.depth = parent_depth + 1;
        self
    }

    /// Mark the sub-agent as running
    pub fn mark_running(&mut self, session_id: i64) {
        self.status = SubAgentStatus::Running;
        self.session_id = Some(session_id);
    }

    /// Mark the sub-agent as completed
    pub fn mark_completed(&mut self, result: String) {
        self.status = SubAgentStatus::Completed;
        self.result = Some(result);
        self.completed_at = Some(chrono::Utc::now());
    }

    /// Mark the sub-agent as failed
    pub fn mark_failed(&mut self, error: String) {
        self.status = SubAgentStatus::Failed;
        self.error = Some(error);
        self.completed_at = Some(chrono::Utc::now());
    }

    /// Mark the sub-agent as timed out
    pub fn mark_timed_out(&mut self) {
        self.status = SubAgentStatus::TimedOut;
        self.error = Some(format!("Execution timed out after {} seconds", self.timeout_secs));
        self.completed_at = Some(chrono::Utc::now());
    }

    /// Mark the sub-agent as cancelled
    pub fn mark_cancelled(&mut self) {
        self.status = SubAgentStatus::Cancelled;
        self.error = Some("Cancelled by user or system".to_string());
        self.completed_at = Some(chrono::Utc::now());
    }

    /// Get the duration of the execution
    pub fn duration(&self) -> chrono::Duration {
        match self.completed_at {
            Some(completed) => completed - self.started_at,
            None => chrono::Utc::now() - self.started_at,
        }
    }
}

/// Configuration for the sub-agent system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentConfig {
    /// Maximum concurrent sub-agents per channel
    pub max_concurrent_per_channel: usize,
    /// Maximum total concurrent sub-agents system-wide
    pub max_total_concurrent: usize,
    /// Default timeout in seconds for sub-agents
    pub default_timeout_secs: u64,
    /// Maximum timeout allowed (cannot exceed this)
    pub max_timeout_secs: u64,
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        Self {
            max_concurrent_per_channel: 3,
            max_total_concurrent: 10,
            default_timeout_secs: 300,
            max_timeout_secs: 3600,
        }
    }
}
