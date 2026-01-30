//! Background process management for async command execution
//!
//! This module provides the ProcessManager which handles spawning and tracking
//! background processes from exec commands. It enables:
//! - Async command execution that returns immediately with a process ID
//! - Real-time output streaming via gateway events
//! - Process status checking and termination
//! - Resource limits (max concurrent processes)

use crate::gateway::events::EventBroadcaster;
use crate::gateway::protocol::GatewayEvent;
use dashmap::DashMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Semaphore};

/// Maximum number of concurrent background processes
const MAX_CONCURRENT_PROCESSES: usize = 5;

/// Maximum number of output lines to buffer per process
const MAX_OUTPUT_BUFFER: usize = 1000;

/// Status of a background process
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
    Completed { exit_code: Option<i32> },
    Failed { error: String },
    Killed,
}

impl std::fmt::Display for ProcessStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Completed { exit_code } => {
                if let Some(code) = exit_code {
                    write!(f, "completed (exit code: {})", code)
                } else {
                    write!(f, "completed")
                }
            }
            Self::Failed { error } => write!(f, "failed: {}", error),
            Self::Killed => write!(f, "killed"),
        }
    }
}

/// Handle to a background process
pub struct ProcessHandle {
    /// Unique process identifier
    pub id: String,
    /// OS process ID (if available)
    pub pid: Option<u32>,
    /// The command that was executed
    pub command: String,
    /// Working directory
    pub workdir: PathBuf,
    /// Channel ID that started this process
    pub channel_id: i64,
    /// Current status
    pub status: ProcessStatus,
    /// Start time
    pub started_at: Instant,
    /// End time (if completed)
    pub ended_at: Option<Instant>,
    /// Buffered stdout lines (ring buffer)
    pub stdout_buffer: VecDeque<String>,
    /// Buffered stderr lines (ring buffer)
    pub stderr_buffer: VecDeque<String>,
    /// Channel to send kill signal
    kill_tx: Option<mpsc::Sender<()>>,
}

impl ProcessHandle {
    /// Get duration in milliseconds
    pub fn duration_ms(&self) -> i64 {
        let end = self.ended_at.unwrap_or_else(Instant::now);
        end.duration_since(self.started_at).as_millis() as i64
    }

    /// Get recent output lines (combined stdout + stderr)
    pub fn recent_output(&self, lines: usize) -> Vec<String> {
        let mut output: Vec<String> = Vec::new();

        // Interleave stdout and stderr, taking most recent
        let stdout_iter = self.stdout_buffer.iter().rev().take(lines);
        let stderr_iter = self.stderr_buffer.iter().rev().take(lines);

        for line in stdout_iter {
            output.push(line.clone());
        }
        for line in stderr_iter {
            output.push(format!("[stderr] {}", line));
        }

        output.truncate(lines);
        output.reverse();
        output
    }
}

/// Manages background processes for async command execution
pub struct ProcessManager {
    /// Event broadcaster for sending gateway events
    broadcaster: Arc<EventBroadcaster>,
    /// Active and completed processes indexed by process ID
    processes: Arc<DashMap<String, ProcessHandle>>,
    /// Semaphore to limit concurrent processes
    semaphore: Arc<Semaphore>,
    /// Counter for generating unique process IDs
    id_counter: AtomicU64,
}

impl ProcessManager {
    /// Create a new ProcessManager
    pub fn new(broadcaster: Arc<EventBroadcaster>) -> Self {
        Self {
            broadcaster,
            processes: Arc::new(DashMap::new()),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_PROCESSES)),
            id_counter: AtomicU64::new(1),
        }
    }

    /// Generate a unique process ID
    fn next_id(&self) -> String {
        let id = self.id_counter.fetch_add(1, Ordering::SeqCst);
        format!("proc_{}", id)
    }

    /// Spawn a command in the background
    ///
    /// Returns the process ID immediately. The process runs asynchronously
    /// and its output is streamed via gateway events.
    pub async fn spawn(
        &self,
        command: &str,
        workdir: &PathBuf,
        channel_id: i64,
        env_vars: Option<&std::collections::HashMap<String, String>>,
    ) -> Result<String, String> {
        // Check if we can acquire a permit (don't block, just check)
        let permit = self
            .semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| format!("Maximum concurrent processes ({}) reached. Kill an existing process first.", MAX_CONCURRENT_PROCESSES))?;

        let process_id = self.next_id();

        // Build the command
        let shell = if cfg!(target_os = "windows") {
            "cmd"
        } else {
            "sh"
        };
        let shell_arg = if cfg!(target_os = "windows") {
            "/C"
        } else {
            "-c"
        };

        let mut cmd = Command::new(shell);
        cmd.arg(shell_arg)
            .arg(command)
            .current_dir(workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Add environment variables if provided
        if let Some(vars) = env_vars {
            for (key, value) in vars {
                cmd.env(key, value);
            }
        }

        // Spawn the process
        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn process: {}", e))?;

        let pid = child.id();

        // Create kill channel
        let (kill_tx, kill_rx) = mpsc::channel(1);

        // Create the process handle
        let handle = ProcessHandle {
            id: process_id.clone(),
            pid,
            command: command.to_string(),
            workdir: workdir.clone(),
            channel_id,
            status: ProcessStatus::Running,
            started_at: Instant::now(),
            ended_at: None,
            stdout_buffer: VecDeque::new(),
            stderr_buffer: VecDeque::new(),
            kill_tx: Some(kill_tx),
        };

        self.processes.insert(process_id.clone(), handle);

        // Broadcast process started event
        self.broadcaster.broadcast(GatewayEvent::process_started(
            channel_id,
            &process_id,
            command,
            pid.unwrap_or(0),
        ));

        log::info!(
            "[PROCESS_MANAGER] Started background process {} (pid: {:?}): {}",
            process_id,
            pid,
            command
        );

        // Spawn async task to monitor the process
        let process_id_clone = process_id.clone();
        let processes = self.processes.clone();
        let broadcaster = self.broadcaster.clone();

        tokio::spawn(async move {
            Self::monitor_process(
                child,
                process_id_clone,
                channel_id,
                processes,
                broadcaster,
                kill_rx,
                permit,
            )
            .await;
        });

        Ok(process_id)
    }

    /// Monitor a running process, streaming its output
    async fn monitor_process(
        mut child: Child,
        process_id: String,
        channel_id: i64,
        processes: DashMap<String, ProcessHandle>,
        broadcaster: Arc<EventBroadcaster>,
        mut kill_rx: mpsc::Receiver<()>,
        _permit: tokio::sync::OwnedSemaphorePermit,
    ) {
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Spawn stdout reader
        let stdout_process_id = process_id.clone();
        let stdout_processes = processes.clone();
        let stdout_broadcaster = broadcaster.clone();
        let stdout_handle = tokio::spawn(async move {
            if let Some(stdout) = stdout {
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    // Buffer the line
                    if let Some(mut handle) = stdout_processes.get_mut(&stdout_process_id) {
                        if handle.stdout_buffer.len() >= MAX_OUTPUT_BUFFER {
                            handle.stdout_buffer.pop_front();
                        }
                        handle.stdout_buffer.push_back(line.clone());
                    }
                    // Broadcast the line
                    stdout_broadcaster.broadcast(GatewayEvent::exec_output(
                        channel_id,
                        &line,
                        "stdout",
                    ));
                }
            }
        });

        // Spawn stderr reader
        let stderr_process_id = process_id.clone();
        let stderr_processes = processes.clone();
        let stderr_broadcaster = broadcaster.clone();
        let stderr_handle = tokio::spawn(async move {
            if let Some(stderr) = stderr {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    // Buffer the line
                    if let Some(mut handle) = stderr_processes.get_mut(&stderr_process_id) {
                        if handle.stderr_buffer.len() >= MAX_OUTPUT_BUFFER {
                            handle.stderr_buffer.pop_front();
                        }
                        handle.stderr_buffer.push_back(line.clone());
                    }
                    // Broadcast the line
                    stderr_broadcaster.broadcast(GatewayEvent::exec_output(
                        channel_id,
                        &line,
                        "stderr",
                    ));
                }
            }
        });

        // Wait for process to complete or be killed
        let exit_status = tokio::select! {
            status = child.wait() => status,
            _ = kill_rx.recv() => {
                // Kill signal received
                let _ = child.kill().await;
                if let Some(mut handle) = processes.get_mut(&process_id) {
                    handle.status = ProcessStatus::Killed;
                    handle.ended_at = Some(Instant::now());
                    let duration = handle.duration_ms();
                    broadcaster.broadcast(GatewayEvent::process_completed(
                        channel_id,
                        &process_id,
                        None,
                        duration,
                    ));
                }
                log::info!("[PROCESS_MANAGER] Process {} killed", process_id);
                return;
            }
        };

        // Wait for output readers to finish
        let _ = stdout_handle.await;
        let _ = stderr_handle.await;

        // Update process status
        match exit_status {
            Ok(status) => {
                let exit_code = status.code();
                if let Some(mut handle) = processes.get_mut(&process_id) {
                    handle.status = ProcessStatus::Completed { exit_code };
                    handle.ended_at = Some(Instant::now());
                    let duration = handle.duration_ms();
                    broadcaster.broadcast(GatewayEvent::process_completed(
                        channel_id,
                        &process_id,
                        exit_code,
                        duration,
                    ));
                    log::info!(
                        "[PROCESS_MANAGER] Process {} completed with exit code {:?} in {}ms",
                        process_id,
                        exit_code,
                        duration
                    );
                }
            }
            Err(e) => {
                if let Some(mut handle) = processes.get_mut(&process_id) {
                    handle.status = ProcessStatus::Failed {
                        error: e.to_string(),
                    };
                    handle.ended_at = Some(Instant::now());
                    let duration = handle.duration_ms();
                    broadcaster.broadcast(GatewayEvent::process_completed(
                        channel_id,
                        &process_id,
                        None,
                        duration,
                    ));
                    log::error!(
                        "[PROCESS_MANAGER] Process {} failed: {}",
                        process_id,
                        e
                    );
                }
            }
        }

        // Note: _permit is dropped here, releasing the semaphore slot
    }

    /// Get the status of a process
    pub fn status(&self, process_id: &str) -> Option<ProcessStatus> {
        self.processes.get(process_id).map(|h| h.status.clone())
    }

    /// Get full process info
    pub fn get(&self, process_id: &str) -> Option<ProcessInfo> {
        self.processes.get(process_id).map(|h| ProcessInfo {
            id: h.id.clone(),
            pid: h.pid,
            command: h.command.clone(),
            status: h.status.clone(),
            duration_ms: h.duration_ms(),
            channel_id: h.channel_id,
        })
    }

    /// Kill a background process
    pub async fn kill(&self, process_id: &str) -> bool {
        if let Some(handle) = self.processes.get(process_id) {
            if handle.status == ProcessStatus::Running {
                if let Some(ref tx) = handle.kill_tx {
                    let _ = tx.send(()).await;
                    return true;
                }
            }
        }
        false
    }

    /// Get recent output from a process
    pub fn output(&self, process_id: &str, lines: usize) -> Option<Vec<String>> {
        self.processes
            .get(process_id)
            .map(|h| h.recent_output(lines))
    }

    /// List all processes for a channel
    pub fn list_for_channel(&self, channel_id: i64) -> Vec<ProcessInfo> {
        self.processes
            .iter()
            .filter(|entry| entry.value().channel_id == channel_id)
            .map(|entry| {
                let h = entry.value();
                ProcessInfo {
                    id: h.id.clone(),
                    pid: h.pid,
                    command: h.command.clone(),
                    status: h.status.clone(),
                    duration_ms: h.duration_ms(),
                    channel_id: h.channel_id,
                }
            })
            .collect()
    }

    /// List all active (running) processes
    pub fn list_active(&self) -> Vec<ProcessInfo> {
        self.processes
            .iter()
            .filter(|entry| entry.value().status == ProcessStatus::Running)
            .map(|entry| {
                let h = entry.value();
                ProcessInfo {
                    id: h.id.clone(),
                    pid: h.pid,
                    command: h.command.clone(),
                    status: h.status.clone(),
                    duration_ms: h.duration_ms(),
                    channel_id: h.channel_id,
                }
            })
            .collect()
    }

    /// Clean up old completed processes (keep last N)
    pub fn cleanup(&self, keep_count: usize) {
        let mut completed: Vec<_> = self
            .processes
            .iter()
            .filter(|entry| entry.value().status != ProcessStatus::Running)
            .map(|entry| (entry.key().clone(), entry.value().ended_at))
            .collect();

        // Sort by end time (oldest first)
        completed.sort_by(|a, b| a.1.cmp(&b.1));

        // Remove oldest entries beyond keep_count
        if completed.len() > keep_count {
            for (id, _) in completed.into_iter().take(completed.len() - keep_count) {
                self.processes.remove(&id);
            }
        }
    }
}

/// Lightweight process info for listing
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub id: String,
    pub pid: Option<u32>,
    pub command: String,
    pub status: ProcessStatus,
    pub duration_ms: i64,
    pub channel_id: i64,
}

impl ProcessInfo {
    /// Convert to JSON value
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "pid": self.pid,
            "command": self.command,
            "status": self.status.to_string(),
            "duration_ms": self.duration_ms,
            "channel_id": self.channel_id
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::events::EventBroadcaster;

    fn create_test_manager() -> ProcessManager {
        let broadcaster = Arc::new(EventBroadcaster::new());
        ProcessManager::new(broadcaster)
    }

    #[tokio::test]
    async fn test_spawn_simple_command() {
        let manager = create_test_manager();
        let workdir = PathBuf::from("/tmp");

        let result = manager.spawn("echo hello", &workdir, 1, None).await;
        assert!(result.is_ok());

        let process_id = result.unwrap();
        assert!(process_id.starts_with("proc_"));

        // Give it time to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let status = manager.status(&process_id);
        assert!(status.is_some());
    }

    #[tokio::test]
    async fn test_process_output_buffering() {
        let manager = create_test_manager();
        let workdir = PathBuf::from("/tmp");

        let result = manager
            .spawn("echo line1; echo line2; echo line3", &workdir, 1, None)
            .await;
        assert!(result.is_ok());

        let process_id = result.unwrap();

        // Wait for process to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let output = manager.output(&process_id, 10);
        assert!(output.is_some());
        let lines = output.unwrap();
        assert!(!lines.is_empty());
    }

    #[tokio::test]
    async fn test_kill_process() {
        let manager = create_test_manager();
        let workdir = PathBuf::from("/tmp");

        // Start a long-running process
        let result = manager.spawn("sleep 10", &workdir, 1, None).await;
        assert!(result.is_ok());

        let process_id = result.unwrap();

        // Verify it's running
        let status = manager.status(&process_id);
        assert_eq!(status, Some(ProcessStatus::Running));

        // Kill it
        let killed = manager.kill(&process_id).await;
        assert!(killed);

        // Give it time to be killed
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let status = manager.status(&process_id);
        assert_eq!(status, Some(ProcessStatus::Killed));
    }
}
