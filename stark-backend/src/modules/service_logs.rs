//! Per-module service log capture.
//!
//! Stores the last N lines of stdout/stderr from each module's child process
//! in a global, thread-safe ring buffer so they can be served via the API.

use std::collections::{HashMap, VecDeque};
use std::io::BufRead;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_LINES: usize = 500;

type LogBuffer = Arc<Mutex<VecDeque<String>>>;
type LogStore = Mutex<HashMap<String, LogBuffer>>;

static STORE: OnceLock<LogStore> = OnceLock::new();

fn store() -> &'static LogStore {
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Get or create the ring buffer for a module.
pub fn get_or_create(name: &str) -> LogBuffer {
    let mut map = store().lock().unwrap();
    map.entry(name.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LINES))))
        .clone()
}

/// Push a line into a buffer, evicting the oldest if over capacity.
pub fn push_line(buf: &LogBuffer, line: String) {
    let mut q = buf.lock().unwrap();
    if q.len() >= MAX_LINES {
        q.pop_front();
    }
    q.push_back(line);
}

/// Read all buffered lines for a module (returns empty vec if unknown).
pub fn read_lines(name: &str) -> Vec<String> {
    let map = store().lock().unwrap();
    match map.get(name) {
        Some(buf) => buf.lock().unwrap().iter().cloned().collect(),
        None => Vec::new(),
    }
}

/// Spawn threads that read stdout and stderr from a child process, storing
/// each line in the module's ring buffer and forwarding to the parent's stderr
/// with a `[module_name]` prefix so container logs stay visible.
pub fn spawn_log_capture_threads(
    name: &str,
    stdout: Option<std::process::ChildStdout>,
    stderr: Option<std::process::ChildStderr>,
) {
    let buf = get_or_create(name);

    if let Some(out) = stdout {
        let buf = buf.clone();
        let tag = format!("[{}]", name);
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(out);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        eprintln!("{} {}", tag, l);
                        push_line(&buf, l);
                    }
                    Err(_) => break,
                }
            }
        });
    }

    if let Some(err) = stderr {
        let buf = buf.clone();
        let tag = format!("[{}]", name);
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(err);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        eprintln!("{} {}", tag, l);
                        push_line(&buf, l);
                    }
                    Err(_) => break,
                }
            }
        });
    }
}
