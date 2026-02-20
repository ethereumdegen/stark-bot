//! SQLite database operations for the wallet monitor service.

use rusqlite::{Connection, Result as SqliteResult};
use std::sync::Mutex;
use wallet_monitor_types::*;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &str) -> SqliteResult<Self> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.create_tables()?;
        Ok(db)
    }

    fn create_tables(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS wallet_watchlist (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                address TEXT NOT NULL,
                label TEXT,
                chain TEXT NOT NULL DEFAULT 'mainnet',
                monitor_enabled INTEGER NOT NULL DEFAULT 1,
                large_trade_threshold_usd REAL NOT NULL DEFAULT 1000.0,
                copy_trade_enabled INTEGER NOT NULL DEFAULT 0,
                copy_trade_max_usd REAL,
                last_checked_block INTEGER,
                last_checked_at TEXT,
                notes TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(address, chain)
            )",
            [],
        )?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS wallet_activity (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                watchlist_id INTEGER NOT NULL,
                chain TEXT NOT NULL,
                tx_hash TEXT NOT NULL,
                block_number INTEGER NOT NULL,
                block_timestamp TEXT,
                from_address TEXT NOT NULL,
                to_address TEXT NOT NULL,
                activity_type TEXT NOT NULL,
                asset_symbol TEXT,
                asset_address TEXT,
                amount_raw TEXT,
                amount_formatted TEXT,
                usd_value REAL,
                is_large_trade INTEGER NOT NULL DEFAULT 0,
                swap_from_token TEXT,
                swap_from_amount TEXT,
                swap_to_token TEXT,
                swap_to_amount TEXT,
                raw_data TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (watchlist_id) REFERENCES wallet_watchlist(id) ON DELETE CASCADE,
                UNIQUE(tx_hash, watchlist_id)
            )",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_wallet_activity_watchlist ON wallet_activity(watchlist_id, block_number DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_wallet_activity_large ON wallet_activity(is_large_trade, created_at DESC)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_wallet_activity_chain ON wallet_activity(chain, block_number DESC)",
            [],
        )?;
        Ok(())
    }

    // =====================================================
    // Watchlist Operations
    // =====================================================

    pub fn add_to_watchlist(
        &self,
        address: &str,
        label: Option<&str>,
        chain: &str,
        threshold_usd: f64,
    ) -> SqliteResult<WatchlistEntry> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let addr = address.to_lowercase();

        conn.execute(
            "INSERT INTO wallet_watchlist (address, label, chain, large_trade_threshold_usd, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            rusqlite::params![addr, label, chain, threshold_usd, now],
        )?;

        let id = conn.last_insert_rowid();
        Ok(WatchlistEntry {
            id,
            address: addr,
            label: label.map(|s| s.to_string()),
            chain: chain.to_string(),
            monitor_enabled: true,
            large_trade_threshold_usd: threshold_usd,
            copy_trade_enabled: false,
            copy_trade_max_usd: None,
            last_checked_block: None,
            last_checked_at: None,
            notes: None,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn remove_from_watchlist(&self, id: i64) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows = conn.execute("DELETE FROM wallet_watchlist WHERE id = ?1", [id])?;
        Ok(rows > 0)
    }

    pub fn list_watchlist(&self) -> SqliteResult<Vec<WatchlistEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, address, label, chain, monitor_enabled, large_trade_threshold_usd,
                    copy_trade_enabled, copy_trade_max_usd, last_checked_block, last_checked_at,
                    notes, created_at, updated_at
             FROM wallet_watchlist ORDER BY created_at ASC",
        )?;
        let entries = stmt
            .query_map([], |row| row_to_watchlist_entry(row))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn list_active_watchlist(&self) -> SqliteResult<Vec<WatchlistEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, address, label, chain, monitor_enabled, large_trade_threshold_usd,
                    copy_trade_enabled, copy_trade_max_usd, last_checked_block, last_checked_at,
                    notes, created_at, updated_at
             FROM wallet_watchlist WHERE monitor_enabled = 1 ORDER BY created_at ASC",
        )?;
        let entries = stmt
            .query_map([], |row| row_to_watchlist_entry(row))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn update_watchlist_entry(
        &self,
        id: i64,
        label: Option<&str>,
        threshold_usd: Option<f64>,
        monitor_enabled: Option<bool>,
        notes: Option<&str>,
    ) -> SqliteResult<bool> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();

        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut param_idx = 2u32;
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

        if let Some(label) = label {
            updates.push(format!("label = ?{}", param_idx));
            params.push(Box::new(label.to_string()));
            param_idx += 1;
        }
        if let Some(threshold) = threshold_usd {
            updates.push(format!("large_trade_threshold_usd = ?{}", param_idx));
            params.push(Box::new(threshold));
            param_idx += 1;
        }
        if let Some(enabled) = monitor_enabled {
            updates.push(format!("monitor_enabled = ?{}", param_idx));
            params.push(Box::new(enabled));
            param_idx += 1;
        }
        if let Some(notes) = notes {
            updates.push(format!("notes = ?{}", param_idx));
            params.push(Box::new(notes.to_string()));
            param_idx += 1;
        }

        let sql = format!(
            "UPDATE wallet_watchlist SET {} WHERE id = ?{}",
            updates.join(", "),
            param_idx
        );
        params.push(Box::new(id));

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.execute(&sql, param_refs.as_slice())?;
        Ok(rows > 0)
    }

    pub fn update_watchlist_cursor(&self, id: i64, block_number: i64) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE wallet_watchlist SET last_checked_block = ?1, last_checked_at = ?2, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![block_number, now, id],
        )?;
        Ok(())
    }

    // =====================================================
    // Activity Operations
    // =====================================================

    pub fn insert_activity(
        &self,
        watchlist_id: i64,
        chain: &str,
        tx_hash: &str,
        block_number: i64,
        block_timestamp: Option<&str>,
        from_address: &str,
        to_address: &str,
        activity_type: &str,
        asset_symbol: Option<&str>,
        asset_address: Option<&str>,
        amount_raw: Option<&str>,
        amount_formatted: Option<&str>,
        usd_value: Option<f64>,
        is_large_trade: bool,
        swap_from_token: Option<&str>,
        swap_from_amount: Option<&str>,
        swap_to_token: Option<&str>,
        swap_to_amount: Option<&str>,
        raw_data: Option<&str>,
    ) -> SqliteResult<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO wallet_activity (
                watchlist_id, chain, tx_hash, block_number, block_timestamp,
                from_address, to_address, activity_type, asset_symbol, asset_address,
                amount_raw, amount_formatted, usd_value, is_large_trade,
                swap_from_token, swap_from_amount, swap_to_token, swap_to_amount, raw_data
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            rusqlite::params![
                watchlist_id, chain, tx_hash, block_number, block_timestamp,
                from_address, to_address, activity_type, asset_symbol, asset_address,
                amount_raw, amount_formatted, usd_value, is_large_trade,
                swap_from_token, swap_from_amount, swap_to_token, swap_to_amount, raw_data
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn query_activity(&self, filter: &ActivityFilter) -> SqliteResult<Vec<ActivityEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut conditions = vec!["1=1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let mut param_idx = 1u32;

        if let Some(wid) = filter.watchlist_id {
            conditions.push(format!("a.watchlist_id = ?{}", param_idx));
            params.push(Box::new(wid));
            param_idx += 1;
        }
        if let Some(ref addr) = filter.address {
            conditions.push(format!(
                "(a.from_address = ?{p} OR a.to_address = ?{p})",
                p = param_idx
            ));
            params.push(Box::new(addr.to_lowercase()));
            param_idx += 1;
        }
        if let Some(ref atype) = filter.activity_type {
            conditions.push(format!("a.activity_type = ?{}", param_idx));
            params.push(Box::new(atype.clone()));
            param_idx += 1;
        }
        if let Some(ref chain) = filter.chain {
            conditions.push(format!("a.chain = ?{}", param_idx));
            params.push(Box::new(chain.clone()));
            param_idx += 1;
        }
        if filter.large_only {
            conditions.push("a.is_large_trade = 1".to_string());
        }
        let _ = param_idx; // suppress unused warning

        let limit = filter.limit.unwrap_or(50).min(200);
        let sql = format!(
            "SELECT a.id, a.watchlist_id, a.chain, a.tx_hash, a.block_number, a.block_timestamp,
                    a.from_address, a.to_address, a.activity_type, a.asset_symbol, a.asset_address,
                    a.amount_raw, a.amount_formatted, a.usd_value, a.is_large_trade,
                    a.swap_from_token, a.swap_from_amount, a.swap_to_token, a.swap_to_amount,
                    a.raw_data, a.created_at
             FROM wallet_activity a
             WHERE {}
             ORDER BY a.block_number DESC, a.id DESC
             LIMIT {}",
            conditions.join(" AND "),
            limit
        );

        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let entries = stmt
            .query_map(param_refs.as_slice(), |row| row_to_activity_entry(row))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn get_activity_stats(&self) -> SqliteResult<ActivityStats> {
        let conn = self.conn.lock().unwrap();
        let total_transactions: i64 = conn
            .query_row("SELECT COUNT(*) FROM wallet_activity", [], |row| row.get(0))
            .unwrap_or(0);
        let large_trades: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wallet_activity WHERE is_large_trade = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let watched_wallets: i64 = conn
            .query_row("SELECT COUNT(*) FROM wallet_watchlist", [], |row| row.get(0))
            .unwrap_or(0);
        let active_wallets: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM wallet_watchlist WHERE monitor_enabled = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(ActivityStats {
            total_transactions,
            large_trades,
            watched_wallets,
            active_wallets,
        })
    }

    pub fn export_watchlist_for_backup(&self) -> SqliteResult<Vec<BackupEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT address, label, chain, monitor_enabled, large_trade_threshold_usd,
                    copy_trade_enabled, copy_trade_max_usd, notes
             FROM wallet_watchlist ORDER BY created_at ASC",
        )?;
        let entries = stmt
            .query_map([], |row| {
                Ok(BackupEntry {
                    address: row.get(0)?,
                    label: row.get(1)?,
                    chain: row.get(2)?,
                    monitor_enabled: row.get(3)?,
                    large_trade_threshold_usd: row.get(4)?,
                    copy_trade_enabled: row.get(5)?,
                    copy_trade_max_usd: row.get(6)?,
                    notes: row.get(7)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(entries)
    }

    pub fn clear_and_restore_watchlist(&self, entries: &[BackupEntry]) -> Result<usize, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM wallet_activity", [])
            .map_err(|e| format!("Failed to clear activity: {}", e))?;
        conn.execute("DELETE FROM wallet_watchlist", [])
            .map_err(|e| format!("Failed to clear watchlist: {}", e))?;

        let now = chrono::Utc::now().to_rfc3339();
        let mut count = 0;
        for entry in entries {
            conn.execute(
                "INSERT OR IGNORE INTO wallet_watchlist
                    (address, label, chain, monitor_enabled, large_trade_threshold_usd,
                     copy_trade_enabled, copy_trade_max_usd, notes, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                rusqlite::params![
                    entry.address, entry.label, entry.chain, entry.monitor_enabled,
                    entry.large_trade_threshold_usd, entry.copy_trade_enabled,
                    entry.copy_trade_max_usd, entry.notes, now
                ],
            )
            .map_err(|e| format!("Failed to insert watchlist entry: {}", e))?;
            count += 1;
        }
        Ok(count)
    }
}

fn row_to_watchlist_entry(row: &rusqlite::Row) -> rusqlite::Result<WatchlistEntry> {
    Ok(WatchlistEntry {
        id: row.get(0)?,
        address: row.get(1)?,
        label: row.get(2)?,
        chain: row.get(3)?,
        monitor_enabled: row.get(4)?,
        large_trade_threshold_usd: row.get(5)?,
        copy_trade_enabled: row.get(6)?,
        copy_trade_max_usd: row.get(7)?,
        last_checked_block: row.get(8)?,
        last_checked_at: row.get(9)?,
        notes: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn row_to_activity_entry(row: &rusqlite::Row) -> rusqlite::Result<ActivityEntry> {
    Ok(ActivityEntry {
        id: row.get(0)?,
        watchlist_id: row.get(1)?,
        chain: row.get(2)?,
        tx_hash: row.get(3)?,
        block_number: row.get(4)?,
        block_timestamp: row.get(5)?,
        from_address: row.get(6)?,
        to_address: row.get(7)?,
        activity_type: row.get(8)?,
        asset_symbol: row.get(9)?,
        asset_address: row.get(10)?,
        amount_raw: row.get(11)?,
        amount_formatted: row.get(12)?,
        usd_value: row.get(13)?,
        is_large_trade: row.get(14)?,
        swap_from_token: row.get(15)?,
        swap_from_amount: row.get(16)?,
        swap_to_token: row.get(17)?,
        swap_to_amount: row.get(18)?,
        raw_data: row.get(19)?,
        created_at: row.get(20)?,
    })
}
