//! Disk Quota Manager — application-level enforcement of disk usage limits.
//!
//! Scans tracked directories on startup, re-scans periodically, and provides
//! a fast lock-free `check_quota()` via AtomicU64 for use before every write.

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use walkdir::WalkDir;

/// Default disk quota in megabytes (256 MB)
const DEFAULT_QUOTA_MB: u64 = 256;

/// Per-write size cap (5 MB)
pub const MAX_WRITE_BYTES: usize = 5 * 1024 * 1024;

/// Per-memory-append size cap (100 KB)
pub const MAX_MEMORY_APPEND_BYTES: usize = 100 * 1024;

/// Max skill ZIP upload size (10 MB)
pub const MAX_SKILL_ZIP_BYTES: usize = 10 * 1024 * 1024;

/// Error returned when a disk quota would be exceeded.
#[derive(Debug)]
pub struct QuotaError {
    pub requested_bytes: u64,
    pub remaining_bytes: u64,
    pub quota_bytes: u64,
    pub used_bytes: u64,
}

impl fmt::Display for QuotaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Disk quota exceeded: cannot write {} — only {} remaining out of {} total ({} used). \
             Visit the System page to view your storage breakdown and clean up files.",
            format_bytes(self.requested_bytes),
            format_bytes(self.remaining_bytes),
            format_bytes(self.quota_bytes),
            format_bytes(self.used_bytes),
        )
    }
}

impl std::error::Error for QuotaError {}

/// Manages disk usage tracking and quota enforcement for a set of directories.
pub struct DiskQuotaManager {
    quota_bytes: u64,
    tracked_dirs: Vec<PathBuf>,
    cached_usage: AtomicU64,
}

impl DiskQuotaManager {
    /// Create a new DiskQuotaManager.
    ///
    /// `quota_mb` — quota in megabytes (0 = disabled, uses env default if None)
    /// `tracked_dirs` — directories to scan for usage
    pub fn new(quota_mb: Option<u64>, tracked_dirs: Vec<PathBuf>) -> Self {
        let quota_mb = quota_mb.unwrap_or_else(|| {
            std::env::var("STARK_DISK_QUOTA_MB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_QUOTA_MB)
        });

        let quota_bytes = quota_mb * 1024 * 1024;

        let manager = Self {
            quota_bytes,
            tracked_dirs,
            cached_usage: AtomicU64::new(0),
        };

        // Initial scan
        let usage = manager.scan_usage();
        manager.cached_usage.store(usage, Ordering::Relaxed);

        manager
    }

    /// Whether the quota is enabled (quota_bytes > 0).
    pub fn is_enabled(&self) -> bool {
        self.quota_bytes > 0
    }

    /// Check if writing `additional_bytes` would exceed the quota.
    /// Returns Ok(()) if allowed, Err(QuotaError) if not.
    pub fn check_quota(&self, additional_bytes: u64) -> Result<(), QuotaError> {
        if !self.is_enabled() {
            return Ok(());
        }

        let current = self.cached_usage.load(Ordering::Relaxed);
        let after_write = current.saturating_add(additional_bytes);

        if after_write > self.quota_bytes {
            Err(QuotaError {
                requested_bytes: additional_bytes,
                remaining_bytes: self.quota_bytes.saturating_sub(current),
                quota_bytes: self.quota_bytes,
                used_bytes: current,
            })
        } else {
            Ok(())
        }
    }

    /// Optimistically bump cached usage after a successful write.
    pub fn record_write(&self, bytes_written: u64) {
        if self.is_enabled() {
            self.cached_usage.fetch_add(bytes_written, Ordering::Relaxed);
        }
    }

    /// Walk all tracked directories and compute total disk usage in bytes.
    pub fn scan_usage(&self) -> u64 {
        let mut total: u64 = 0;
        for dir in &self.tracked_dirs {
            if !dir.exists() {
                continue;
            }
            for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    if let Ok(meta) = entry.metadata() {
                        total += meta.len();
                    }
                }
            }
        }
        total
    }

    /// Re-scan and update the cached usage. Returns new usage.
    pub fn refresh(&self) -> u64 {
        let usage = self.scan_usage();
        self.cached_usage.store(usage, Ordering::Relaxed);
        usage
    }

    /// Current cached usage in bytes.
    pub fn usage_bytes(&self) -> u64 {
        self.cached_usage.load(Ordering::Relaxed)
    }

    /// Remaining bytes before hitting the quota.
    pub fn remaining_bytes(&self) -> u64 {
        if !self.is_enabled() {
            return u64::MAX;
        }
        self.quota_bytes
            .saturating_sub(self.cached_usage.load(Ordering::Relaxed))
    }

    /// Usage as a percentage (0–100). Returns 0 if quota is disabled.
    pub fn usage_percentage(&self) -> u64 {
        if !self.is_enabled() || self.quota_bytes == 0 {
            return 0;
        }
        let used = self.cached_usage.load(Ordering::Relaxed);
        (used * 100) / self.quota_bytes
    }

    /// Quota limit in bytes.
    pub fn quota_bytes(&self) -> u64 {
        self.quota_bytes
    }

    /// Format a human-readable status line, e.g. "Disk quota: 12.3MB / 256MB (5%)"
    pub fn status_line(&self) -> String {
        if !self.is_enabled() {
            return "Disk quota: disabled".to_string();
        }
        let used = self.usage_bytes();
        let pct = self.usage_percentage();
        format!(
            "Disk quota: {} / {} ({}%)",
            format_bytes(used),
            format_bytes(self.quota_bytes),
            pct,
        )
    }
}

/// Format bytes into a human-readable string (e.g. "12.3MB").
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    if bytes >= GB {
        format!("{:.1}GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1}MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1}KB", bytes as f64 / KB as f64)
    } else {
        format!("{}B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_quota_disabled_when_zero() {
        let manager = DiskQuotaManager::new(Some(0), vec![]);
        assert!(!manager.is_enabled());
        assert!(manager.check_quota(u64::MAX).is_ok());
    }

    #[test]
    fn test_quota_allows_within_limit() {
        let dir = tempdir().unwrap();
        let manager = DiskQuotaManager::new(Some(1), vec![dir.path().to_path_buf()]);
        assert!(manager.is_enabled());
        // 1MB quota, no files yet — should allow 500KB
        assert!(manager.check_quota(500 * 1024).is_ok());
    }

    #[test]
    fn test_quota_rejects_over_limit() {
        let dir = tempdir().unwrap();
        let manager = DiskQuotaManager::new(Some(1), vec![dir.path().to_path_buf()]);
        // 1MB quota — should reject 2MB
        let result = manager.check_quota(2 * 1024 * 1024);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.quota_bytes, 1024 * 1024);
        assert!(err.to_string().contains("Disk quota exceeded"));
    }

    #[test]
    fn test_record_write_bumps_usage() {
        let dir = tempdir().unwrap();
        let manager = DiskQuotaManager::new(Some(1), vec![dir.path().to_path_buf()]);
        assert_eq!(manager.usage_bytes(), 0);

        manager.record_write(100_000);
        assert_eq!(manager.usage_bytes(), 100_000);

        // Now check that we can't exceed the remaining
        let remaining = manager.remaining_bytes();
        assert!(manager.check_quota(remaining + 1).is_err());
        assert!(manager.check_quota(remaining).is_ok());
    }

    #[test]
    fn test_scan_usage_counts_files() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap(); // 11 bytes

        let manager = DiskQuotaManager::new(Some(1), vec![dir.path().to_path_buf()]);
        assert_eq!(manager.usage_bytes(), 11);
    }

    #[test]
    fn test_refresh_updates_cached_usage() {
        let dir = tempdir().unwrap();
        let manager = DiskQuotaManager::new(Some(1), vec![dir.path().to_path_buf()]);
        assert_eq!(manager.usage_bytes(), 0);

        // Write a file after creation
        fs::write(dir.path().join("data.bin"), vec![0u8; 1000]).unwrap();

        // Cached usage is still 0
        assert_eq!(manager.usage_bytes(), 0);

        // After refresh, it should reflect the file
        let new_usage = manager.refresh();
        assert_eq!(new_usage, 1000);
        assert_eq!(manager.usage_bytes(), 1000);
    }

    #[test]
    fn test_usage_percentage() {
        let dir = tempdir().unwrap();
        let manager = DiskQuotaManager::new(Some(1), vec![dir.path().to_path_buf()]);
        assert_eq!(manager.usage_percentage(), 0);

        // Simulate 50% usage
        manager.record_write(512 * 1024);
        assert_eq!(manager.usage_percentage(), 50);
    }

    #[test]
    fn test_status_line() {
        let dir = tempdir().unwrap();
        let manager = DiskQuotaManager::new(Some(256), vec![dir.path().to_path_buf()]);
        let status = manager.status_line();
        assert!(status.contains("256.0MB"));
        assert!(status.contains("0%"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1024), "1.0KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0GB");
        assert_eq!(format_bytes(1536 * 1024), "1.5MB");
    }

    #[test]
    fn test_quota_error_display() {
        let err = QuotaError {
            requested_bytes: 10 * 1024 * 1024,
            remaining_bytes: 5 * 1024 * 1024,
            quota_bytes: 256 * 1024 * 1024,
            used_bytes: 251 * 1024 * 1024,
        };
        let msg = err.to_string();
        assert!(msg.contains("10.0MB"));
        assert!(msg.contains("5.0MB"));
        assert!(msg.contains("256.0MB"));
    }
}
