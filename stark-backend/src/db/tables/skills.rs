//! Skills and skill scripts database operations

use chrono::Utc;
use rusqlite::Result as SqliteResult;

use crate::skills::{DbSkill, DbSkillAbi, DbSkillPreset, DbSkillScript};
use super::super::Database;

/// Compare two semantic version strings (e.g., "1.0.0", "2.1.3")
/// Returns: Some(Ordering) if both are valid semver, None otherwise
/// Supports versions with or without patch number (e.g., "1.0" treated as "1.0.0")
fn compare_semver(v1: &str, v2: &str) -> Option<std::cmp::Ordering> {
    let parse_version = |v: &str| -> Option<(u32, u32, u32)> {
        let parts: Vec<&str> = v.trim().split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        let major = parts.first()?.parse().ok()?;
        let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        Some((major, minor, patch))
    };

    let v1_parts = parse_version(v1)?;
    let v2_parts = parse_version(v2)?;
    Some(v1_parts.cmp(&v2_parts))
}

impl Database {
    // ============================================
    // Skills CRUD methods (database-backed)
    // ============================================

    /// Create a new skill in the database, or update if version is higher
    /// Returns the skill ID (existing or new)
    ///
    /// Uses semantic version comparison: only updates if new version > existing version.
    /// If versions are equal or incoming is lower, the existing skill is preserved.
    pub fn create_skill(&self, skill: &DbSkill) -> SqliteResult<i64> {
        self.create_skill_internal(skill, false)
    }

    /// Create or update a skill, bypassing version checks
    /// Use this for source-priority loading (workspace > managed > bundled)
    pub fn create_skill_force(&self, skill: &DbSkill) -> SqliteResult<i64> {
        self.create_skill_internal(skill, true)
    }

    fn create_skill_internal(&self, skill: &DbSkill, force: bool) -> SqliteResult<i64> {
        let conn = self.conn();

        // Check if skill already exists and compare versions (unless force=true)
        if !force {
            let existing: Option<(i64, String)> = conn
                .prepare("SELECT id, version FROM skills WHERE name = ?1")?
                .query_row([&skill.name], |row| Ok((row.get(0)?, row.get(1)?)))
                .ok();

            if let Some((existing_id, existing_version)) = existing {
                // Compare versions - only update if new version is higher
                match compare_semver(&skill.version, &existing_version) {
                    Some(std::cmp::Ordering::Greater) => {
                        log::info!(
                            "Updating skill '{}': {} -> {}",
                            skill.name, existing_version, skill.version
                        );
                        // Continue to update below
                    }
                    Some(std::cmp::Ordering::Equal) => {
                        log::debug!(
                            "Skill '{}' version {} unchanged, skipping",
                            skill.name, skill.version
                        );
                        return Ok(existing_id);
                    }
                    Some(std::cmp::Ordering::Less) => {
                        log::debug!(
                            "Skill '{}' has newer version {} (incoming: {}), skipping",
                            skill.name, existing_version, skill.version
                        );
                        return Ok(existing_id);
                    }
                    None => {
                        // Invalid version format - log warning but still update
                        log::warn!(
                            "Invalid version format for skill '{}': existing='{}', new='{}'. Updating anyway.",
                            skill.name, existing_version, skill.version
                        );
                    }
                }
            }
        }

        let now = Utc::now().to_rfc3339();
        let requires_tools_json = serde_json::to_string(&skill.requires_tools).unwrap_or_default();
        let requires_binaries_json = serde_json::to_string(&skill.requires_binaries).unwrap_or_default();
        let arguments_json = serde_json::to_string(&skill.arguments).unwrap_or_default();
        let tags_json = serde_json::to_string(&skill.tags).unwrap_or_default();
        let requires_api_keys_json = serde_json::to_string(&skill.requires_api_keys).unwrap_or_default();

        conn.execute(
            "INSERT INTO skills (name, description, body, version, author, homepage, metadata, enabled, requires_tools, requires_binaries, arguments, tags, subagent_type, requires_api_keys, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
             ON CONFLICT(name) DO UPDATE SET
                description = excluded.description,
                body = excluded.body,
                version = excluded.version,
                author = excluded.author,
                homepage = excluded.homepage,
                metadata = excluded.metadata,
                requires_tools = excluded.requires_tools,
                requires_binaries = excluded.requires_binaries,
                arguments = excluded.arguments,
                tags = excluded.tags,
                subagent_type = excluded.subagent_type,
                requires_api_keys = excluded.requires_api_keys,
                updated_at = excluded.updated_at",
            rusqlite::params![
                skill.name,
                skill.description,
                skill.body,
                skill.version,
                skill.author,
                skill.homepage,
                skill.metadata,
                skill.enabled as i32,
                requires_tools_json,
                requires_binaries_json,
                arguments_json,
                tags_json,
                skill.subagent_type,
                requires_api_keys_json,
                now
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Get a skill by name
    pub fn get_skill(&self, name: &str) -> SqliteResult<Option<DbSkill>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, body, version, author, homepage, metadata, enabled, requires_tools, requires_binaries, arguments, tags, subagent_type, requires_api_keys, created_at, updated_at
             FROM skills WHERE name = ?1"
        )?;

        let skill = stmt
            .query_row([name], |row| Self::row_to_db_skill(row))
            .ok();

        Ok(skill)
    }

    /// Get a skill by ID
    pub fn get_skill_by_id(&self, id: i64) -> SqliteResult<Option<DbSkill>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, body, version, author, homepage, metadata, enabled, requires_tools, requires_binaries, arguments, tags, subagent_type, requires_api_keys, created_at, updated_at
             FROM skills WHERE id = ?1"
        )?;

        let skill = stmt
            .query_row([id], |row| Self::row_to_db_skill(row))
            .ok();

        Ok(skill)
    }

    /// Get an enabled skill by name (more efficient than loading all skills)
    pub fn get_enabled_skill_by_name(&self, name: &str) -> SqliteResult<Option<DbSkill>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, body, version, author, homepage, metadata, enabled, requires_tools, requires_binaries, arguments, tags, subagent_type, requires_api_keys, created_at, updated_at
             FROM skills WHERE name = ?1 AND enabled = 1 LIMIT 1"
        )?;

        let skill = stmt
            .query_row([name], |row| Self::row_to_db_skill(row))
            .ok();

        Ok(skill)
    }

    /// List all skills
    pub fn list_skills(&self) -> SqliteResult<Vec<DbSkill>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, body, version, author, homepage, metadata, enabled, requires_tools, requires_binaries, arguments, tags, subagent_type, requires_api_keys, created_at, updated_at
             FROM skills ORDER BY name"
        )?;

        let skills: Vec<DbSkill> = stmt
            .query_map([], |row| Self::row_to_db_skill(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(skills)
    }

    /// List enabled skills
    pub fn list_enabled_skills(&self) -> SqliteResult<Vec<DbSkill>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, body, version, author, homepage, metadata, enabled, requires_tools, requires_binaries, arguments, tags, subagent_type, requires_api_keys, created_at, updated_at
             FROM skills WHERE enabled = 1 ORDER BY name"
        )?;

        let skills: Vec<DbSkill> = stmt
            .query_map([], |row| Self::row_to_db_skill(row))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(skills)
    }

    /// Update skill enabled status
    pub fn set_skill_enabled(&self, name: &str, enabled: bool) -> SqliteResult<bool> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();
        let rows_affected = conn.execute(
            "UPDATE skills SET enabled = ?1, updated_at = ?2 WHERE name = ?3",
            rusqlite::params![enabled as i32, now, name],
        )?;
        Ok(rows_affected > 0)
    }

    /// Delete a skill (cascade deletes scripts)
    pub fn delete_skill(&self, name: &str) -> SqliteResult<bool> {
        let conn = self.conn();
        let rows_affected = conn.execute(
            "DELETE FROM skills WHERE name = ?1",
            [name],
        )?;
        Ok(rows_affected > 0)
    }

    fn row_to_db_skill(row: &rusqlite::Row) -> rusqlite::Result<DbSkill> {
        let requires_tools_str: String = row.get(9)?;
        let requires_binaries_str: String = row.get(10)?;
        let arguments_str: String = row.get(11)?;
        let tags_str: String = row.get(12)?;
        let requires_api_keys_str: String = row.get::<_, Option<String>>(14)?.unwrap_or_else(|| "{}".to_string());

        Ok(DbSkill {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            body: row.get(3)?,
            version: row.get(4)?,
            // Handle NULL values for optional fields
            author: row.get::<_, Option<String>>(5)?,
            homepage: row.get::<_, Option<String>>(6)?,
            metadata: row.get::<_, Option<String>>(7)?,
            enabled: row.get::<_, i32>(8)? != 0,
            requires_tools: serde_json::from_str(&requires_tools_str).unwrap_or_default(),
            requires_binaries: serde_json::from_str(&requires_binaries_str).unwrap_or_default(),
            arguments: serde_json::from_str(&arguments_str).unwrap_or_default(),
            tags: serde_json::from_str(&tags_str).unwrap_or_default(),
            subagent_type: row.get::<_, Option<String>>(13)?,
            requires_api_keys: serde_json::from_str(&requires_api_keys_str).unwrap_or_default(),
            created_at: row.get(15)?,
            updated_at: row.get(16)?,
        })
    }

    // ============================================
    // Skill Scripts CRUD methods
    // ============================================

    /// Create a skill script
    pub fn create_skill_script(&self, script: &DbSkillScript) -> SqliteResult<i64> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO skill_scripts (skill_id, name, code, language, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(skill_id, name) DO UPDATE SET
                code = excluded.code,
                language = excluded.language",
            rusqlite::params![
                script.skill_id,
                script.name,
                script.code,
                script.language,
                now
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Get all scripts for a skill
    pub fn get_skill_scripts(&self, skill_id: i64) -> SqliteResult<Vec<DbSkillScript>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, skill_id, name, code, language, created_at
             FROM skill_scripts WHERE skill_id = ?1 ORDER BY name"
        )?;

        let scripts: Vec<DbSkillScript> = stmt
            .query_map([skill_id], |row| {
                Ok(DbSkillScript {
                    id: row.get(0)?,
                    skill_id: row.get(1)?,
                    name: row.get(2)?,
                    code: row.get(3)?,
                    language: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(scripts)
    }

    /// Get scripts for a skill by skill name
    pub fn get_skill_scripts_by_name(&self, skill_name: &str) -> SqliteResult<Vec<DbSkillScript>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT ss.id, ss.skill_id, ss.name, ss.code, ss.language, ss.created_at
             FROM skill_scripts ss
             JOIN skills s ON s.id = ss.skill_id
             WHERE s.name = ?1 ORDER BY ss.name"
        )?;

        let scripts: Vec<DbSkillScript> = stmt
            .query_map([skill_name], |row| {
                Ok(DbSkillScript {
                    id: row.get(0)?,
                    skill_id: row.get(1)?,
                    name: row.get(2)?,
                    code: row.get(3)?,
                    language: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(scripts)
    }

    /// Delete all scripts for a skill
    pub fn delete_skill_scripts(&self, skill_id: i64) -> SqliteResult<i64> {
        let conn = self.conn();
        let rows_affected = conn.execute(
            "DELETE FROM skill_scripts WHERE skill_id = ?1",
            [skill_id],
        )?;
        Ok(rows_affected as i64)
    }

    // ============================================
    // Skill ABIs CRUD methods
    // ============================================

    /// Create or update a skill ABI
    pub fn create_skill_abi(&self, abi: &DbSkillAbi) -> SqliteResult<i64> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO skill_abis (skill_id, name, content, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(skill_id, name) DO UPDATE SET
                content = excluded.content",
            rusqlite::params![
                abi.skill_id,
                abi.name,
                abi.content,
                now
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Get all ABIs for a skill
    pub fn get_skill_abis(&self, skill_id: i64) -> SqliteResult<Vec<DbSkillAbi>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, skill_id, name, content, created_at
             FROM skill_abis WHERE skill_id = ?1 ORDER BY name"
        )?;

        let abis: Vec<DbSkillAbi> = stmt
            .query_map([skill_id], |row| {
                Ok(DbSkillAbi {
                    id: row.get(0)?,
                    skill_id: row.get(1)?,
                    name: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(abis)
    }

    /// Get all ABIs across all skills (for loading into memory at startup)
    pub fn get_all_skill_abis(&self) -> SqliteResult<Vec<DbSkillAbi>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, skill_id, name, content, created_at
             FROM skill_abis ORDER BY name"
        )?;

        let abis: Vec<DbSkillAbi> = stmt
            .query_map([], |row| {
                Ok(DbSkillAbi {
                    id: row.get(0)?,
                    skill_id: row.get(1)?,
                    name: row.get(2)?,
                    content: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(abis)
    }

    // ============================================
    // Skill Presets CRUD methods
    // ============================================

    /// Create or update a skill preset
    pub fn create_skill_preset(&self, preset: &DbSkillPreset) -> SqliteResult<i64> {
        let conn = self.conn();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO skill_presets (skill_id, content, created_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(skill_id) DO UPDATE SET
                content = excluded.content",
            rusqlite::params![
                preset.skill_id,
                preset.content,
                now
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Get preset for a skill
    pub fn get_skill_preset(&self, skill_id: i64) -> SqliteResult<Option<DbSkillPreset>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, skill_id, content, created_at
             FROM skill_presets WHERE skill_id = ?1"
        )?;

        let preset = stmt
            .query_row([skill_id], |row| {
                Ok(DbSkillPreset {
                    id: row.get(0)?,
                    skill_id: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .ok();

        Ok(preset)
    }

    /// Get all presets across all skills (for loading into memory at startup)
    pub fn get_all_skill_presets(&self) -> SqliteResult<Vec<DbSkillPreset>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, skill_id, content, created_at
             FROM skill_presets ORDER BY skill_id"
        )?;

        let presets: Vec<DbSkillPreset> = stmt
            .query_map([], |row| {
                Ok(DbSkillPreset {
                    id: row.get(0)?,
                    skill_id: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(presets)
    }
}
