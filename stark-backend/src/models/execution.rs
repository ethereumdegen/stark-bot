use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents an execution task in the hierarchical progress tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTask {
    /// Unique task identifier (UUID)
    pub id: String,
    /// Parent task ID for hierarchy (None if root)
    pub parent_id: Option<String>,
    /// Channel ID this task belongs to
    pub channel_id: i64,
    /// Type of task
    pub task_type: TaskType,
    /// Human-readable description (e.g., "Reading Plan(~/.claude/plans/example.md)")
    pub description: String,
    /// Present continuous form for active display (e.g., "Reading plan file")
    pub active_form: Option<String>,
    /// Current status of the task
    pub status: TaskStatus,
    /// When the task was created
    pub created_at: DateTime<Utc>,
    /// When the task started executing
    pub started_at: Option<DateTime<Utc>>,
    /// When the task completed
    pub completed_at: Option<DateTime<Utc>>,
    /// Metrics for this task
    pub metrics: TaskMetrics,
}

impl ExecutionTask {
    /// Create a new execution task
    pub fn new(
        channel_id: i64,
        task_type: TaskType,
        description: impl Into<String>,
        parent_id: Option<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            parent_id,
            channel_id,
            task_type,
            description: description.into(),
            active_form: None,
            status: TaskStatus::Pending,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            metrics: TaskMetrics::default(),
        }
    }

    /// Set the active form (present continuous) for display
    pub fn with_active_form(mut self, form: impl Into<String>) -> Self {
        self.active_form = Some(form.into());
        self
    }

    /// Start the task
    pub fn start(&mut self) {
        self.status = TaskStatus::InProgress;
        self.started_at = Some(Utc::now());
    }

    /// Complete the task successfully
    pub fn complete(&mut self) {
        self.status = TaskStatus::Completed;
        self.completed_at = Some(Utc::now());
        if let (Some(start), Some(end)) = (self.started_at, self.completed_at) {
            self.metrics.duration_ms = Some((end - start).num_milliseconds() as u64);
        }
    }

    /// Complete the task with an error
    pub fn complete_with_error(&mut self, error: impl Into<String>) {
        self.status = TaskStatus::Error(error.into());
        self.completed_at = Some(Utc::now());
        if let (Some(start), Some(end)) = (self.started_at, self.completed_at) {
            self.metrics.duration_ms = Some((end - start).num_milliseconds() as u64);
        }
    }

    /// Get duration in milliseconds if completed
    pub fn duration_ms(&self) -> Option<u64> {
        self.metrics.duration_ms
    }
}

/// Type of execution task
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    /// AI processing/reasoning phase
    Thinking,
    /// Single tool execution
    ToolExecution,
    /// Sub-agent spawned for a subtask
    AgentSpawn,
    /// Planning phase (exploring and designing)
    PlanMode,
    /// Main execution phase
    Execution,
    /// Planning tool calls (before execution)
    Planning,
    /// Analyzing/processing results
    Analyzing,
    /// Validating inputs or outputs
    Validation,
    /// Loading context or data
    Loading,
    /// Formatting response
    Formatting,
}

impl std::fmt::Display for TaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskType::Thinking => write!(f, "thinking"),
            TaskType::ToolExecution => write!(f, "tool"),
            TaskType::AgentSpawn => write!(f, "agent"),
            TaskType::PlanMode => write!(f, "plan"),
            TaskType::Execution => write!(f, "execution"),
            TaskType::Planning => write!(f, "planning"),
            TaskType::Analyzing => write!(f, "analyzing"),
            TaskType::Validation => write!(f, "validation"),
            TaskType::Loading => write!(f, "loading"),
            TaskType::Formatting => write!(f, "formatting"),
        }
    }
}

/// Status of an execution task
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "message")]
pub enum TaskStatus {
    /// Waiting to start
    Pending,
    /// Currently executing
    InProgress,
    /// Successfully completed
    Completed,
    /// Failed with an error
    Error(String),
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::InProgress => write!(f, "in_progress"),
            TaskStatus::Completed => write!(f, "completed"),
            TaskStatus::Error(msg) => write!(f, "error: {}", msg),
        }
    }
}

/// Metrics associated with an execution task
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskMetrics {
    /// Number of tool uses within this task
    pub tool_uses: u32,
    /// Tokens used (input + output)
    pub tokens_used: u32,
    /// Lines read from files
    pub lines_read: u32,
    /// Duration in milliseconds
    pub duration_ms: Option<u64>,
    /// Number of child tasks
    pub child_count: u32,
}

impl TaskMetrics {
    /// Create new empty metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Add tool use count
    pub fn with_tool_uses(mut self, count: u32) -> Self {
        self.tool_uses = count;
        self
    }

    /// Add token count
    pub fn with_tokens(mut self, count: u32) -> Self {
        self.tokens_used = count;
        self
    }

    /// Add lines read
    pub fn with_lines_read(mut self, count: u32) -> Self {
        self.lines_read = count;
        self
    }

    /// Increment tool uses
    pub fn add_tool_use(&mut self) {
        self.tool_uses += 1;
    }

    /// Add tokens
    pub fn add_tokens(&mut self, count: u32) {
        self.tokens_used += count;
    }

    /// Add lines read
    pub fn add_lines(&mut self, count: u32) {
        self.lines_read += count;
    }

    /// Format for display (e.g., "14 tool uses · 33.9k tokens")
    pub fn format_display(&self) -> String {
        let mut parts = Vec::new();

        if self.tool_uses > 0 {
            parts.push(format!("{} tool use{}", self.tool_uses, if self.tool_uses == 1 { "" } else { "s" }));
        }

        if self.tokens_used > 0 {
            let formatted = if self.tokens_used >= 1000 {
                format!("{:.1}k tokens", self.tokens_used as f64 / 1000.0)
            } else {
                format!("{} tokens", self.tokens_used)
            };
            parts.push(formatted);
        }

        if let Some(ms) = self.duration_ms {
            if ms >= 1000 {
                parts.push(format!("{:.1}s", ms as f64 / 1000.0));
            } else {
                parts.push(format!("{}ms", ms));
            }
        }

        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_lifecycle() {
        let mut task = ExecutionTask::new(
            1,
            TaskType::ToolExecution,
            "Reading file",
            None,
        );

        assert!(matches!(task.status, TaskStatus::Pending));
        assert!(task.started_at.is_none());

        task.start();
        assert!(matches!(task.status, TaskStatus::InProgress));
        assert!(task.started_at.is_some());

        task.complete();
        assert!(matches!(task.status, TaskStatus::Completed));
        assert!(task.completed_at.is_some());
        assert!(task.duration_ms().is_some());
    }

    #[test]
    fn test_metrics_display() {
        let metrics = TaskMetrics {
            tool_uses: 14,
            tokens_used: 33900,
            lines_read: 0,
            duration_ms: Some(1500),
            child_count: 0,
        };

        let display = metrics.format_display();
        assert!(display.contains("14 tool uses"));
        assert!(display.contains("33.9k tokens"));
        assert!(display.contains("1.5s"));
    }
}
