use crate::db::Database;
use crate::skills::types::{DbSkill, DbSkillScript, Skill, SkillSource};
use crate::skills::zip_parser::{parse_skill_md, parse_skill_zip, ParsedSkill};
use std::path::PathBuf;
use std::sync::Arc;

/// Registry that provides access to skills stored in the database
/// Also maintains backward compatibility with file-based skills
pub struct SkillRegistry {
    db: Arc<Database>,
    /// Optional paths for file-based skill loading (for backward compatibility)
    bundled_path: Option<PathBuf>,
    managed_path: Option<PathBuf>,
    workspace_path: Option<PathBuf>,
}

impl SkillRegistry {
    pub fn new(db: Arc<Database>) -> Self {
        SkillRegistry {
            db,
            bundled_path: None,
            managed_path: None,
            workspace_path: None,
        }
    }

    /// Create a registry with configured paths (for backward compatibility with file-based skills)
    pub fn with_paths(
        db: Arc<Database>,
        bundled_path: Option<PathBuf>,
        managed_path: Option<PathBuf>,
        workspace_path: Option<PathBuf>,
    ) -> Self {
        SkillRegistry {
            db,
            bundled_path,
            managed_path,
            workspace_path,
        }
    }

    /// Get a skill by name
    pub fn get(&self, name: &str) -> Option<Skill> {
        match self.db.get_skill(name) {
            Ok(Some(db_skill)) => Some(db_skill.into_skill()),
            _ => None,
        }
    }

    /// List all registered skills
    pub fn list(&self) -> Vec<Skill> {
        match self.db.list_skills() {
            Ok(skills) => skills.into_iter().map(|s| s.into_skill()).collect(),
            Err(e) => {
                log::error!("Failed to list skills: {}", e);
                Vec::new()
            }
        }
    }

    /// List enabled skills
    pub fn list_enabled(&self) -> Vec<Skill> {
        match self.db.list_enabled_skills() {
            Ok(skills) => skills.into_iter().map(|s| s.into_skill()).collect(),
            Err(e) => {
                log::error!("Failed to list enabled skills: {}", e);
                Vec::new()
            }
        }
    }

    /// Enable or disable a skill
    pub fn set_enabled(&self, name: &str, enabled: bool) -> bool {
        match self.db.set_skill_enabled(name, enabled) {
            Ok(success) => success,
            Err(e) => {
                log::error!("Failed to set skill enabled status: {}", e);
                false
            }
        }
    }

    /// Check if a skill exists
    pub fn has_skill(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Get count of registered skills
    pub fn len(&self) -> usize {
        self.list().len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Create a skill from a parsed ZIP file
    pub fn create_skill_from_zip(&self, data: &[u8]) -> Result<DbSkill, String> {
        let parsed = parse_skill_zip(data)?;
        self.create_skill_from_parsed(parsed)
    }

    /// Create a skill from markdown content, bypassing version checks (force update)
    pub fn create_skill_from_markdown_force(&self, content: &str) -> Result<DbSkill, String> {
        let (metadata, body) = parse_skill_md(content)?;

        let parsed = ParsedSkill {
            name: metadata.name,
            description: metadata.description,
            body,
            version: metadata.version,
            author: metadata.author,
            homepage: metadata.homepage,
            metadata: metadata.metadata,
            requires_tools: metadata.requires_tools,
            requires_binaries: metadata.requires_binaries,
            arguments: metadata.arguments,
            tags: metadata.tags,
            subagent_type: metadata.subagent_type,
            scripts: Vec::new(),
        };

        self.create_skill_from_parsed_force(parsed)
    }

    /// Create a skill from markdown content (SKILL.md format)
    pub fn create_skill_from_markdown(&self, content: &str) -> Result<DbSkill, String> {
        let (metadata, body) = parse_skill_md(content)?;

        let parsed = ParsedSkill {
            name: metadata.name,
            description: metadata.description,
            body,
            version: metadata.version,
            author: metadata.author,
            homepage: metadata.homepage,
            metadata: metadata.metadata,
            requires_tools: metadata.requires_tools,
            requires_binaries: metadata.requires_binaries,
            arguments: metadata.arguments,
            tags: metadata.tags,
            subagent_type: metadata.subagent_type,
            scripts: Vec::new(), // No scripts for plain markdown
        };

        self.create_skill_from_parsed(parsed)
    }

    /// Create a skill from parsed skill data
    pub fn create_skill_from_parsed(&self, parsed: ParsedSkill) -> Result<DbSkill, String> {
        let now = chrono::Utc::now().to_rfc3339();

        let db_skill = DbSkill {
            id: None,
            name: parsed.name.clone(),
            description: parsed.description,
            body: parsed.body,
            version: parsed.version,
            author: parsed.author,
            homepage: parsed.homepage,
            metadata: parsed.metadata,
            enabled: true,
            requires_tools: parsed.requires_tools,
            requires_binaries: parsed.requires_binaries,
            arguments: parsed.arguments,
            tags: parsed.tags,
            subagent_type: parsed.subagent_type,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        // Insert skill into database
        let skill_id = self.db.create_skill(&db_skill)
            .map_err(|e| format!("Failed to create skill: {}", e))?;

        // Insert scripts
        for script in parsed.scripts {
            let db_script = DbSkillScript {
                id: None,
                skill_id,
                name: script.name,
                code: script.code,
                language: script.language,
                created_at: now.clone(),
            };
            self.db.create_skill_script(&db_script)
                .map_err(|e| format!("Failed to create skill script: {}", e))?;
        }

        // Return the created skill
        self.db.get_skill(&parsed.name)
            .map_err(|e| format!("Failed to retrieve created skill: {}", e))?
            .ok_or_else(|| "Skill not found after creation".to_string())
    }

    /// Create a skill from parsed skill data, bypassing version checks (force update)
    pub fn create_skill_from_parsed_force(&self, parsed: ParsedSkill) -> Result<DbSkill, String> {
        let now = chrono::Utc::now().to_rfc3339();

        let db_skill = DbSkill {
            id: None,
            name: parsed.name.clone(),
            description: parsed.description,
            body: parsed.body,
            version: parsed.version,
            author: parsed.author,
            homepage: parsed.homepage,
            metadata: parsed.metadata,
            enabled: true,
            requires_tools: parsed.requires_tools,
            requires_binaries: parsed.requires_binaries,
            arguments: parsed.arguments,
            tags: parsed.tags,
            subagent_type: parsed.subagent_type,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        // Insert skill into database (force - bypass version check)
        let skill_id = self.db.create_skill_force(&db_skill)
            .map_err(|e| format!("Failed to create skill: {}", e))?;

        // Insert scripts
        for script in parsed.scripts {
            let db_script = DbSkillScript {
                id: None,
                skill_id,
                name: script.name,
                code: script.code,
                language: script.language,
                created_at: now.clone(),
            };
            self.db.create_skill_script(&db_script)
                .map_err(|e| format!("Failed to create skill script: {}", e))?;
        }

        // Return the created skill
        self.db.get_skill(&parsed.name)
            .map_err(|e| format!("Failed to retrieve created skill: {}", e))?
            .ok_or_else(|| "Skill not found after creation".to_string())
    }

    /// Delete a skill and its scripts
    pub fn delete_skill(&self, name: &str) -> Result<bool, String> {
        self.db.delete_skill(name)
            .map_err(|e| format!("Failed to delete skill: {}", e))
    }

    /// Get scripts for a skill
    pub fn get_skill_scripts(&self, skill_name: &str) -> Vec<DbSkillScript> {
        match self.db.get_skill_scripts_by_name(skill_name) {
            Ok(scripts) => scripts,
            Err(e) => {
                log::error!("Failed to get skill scripts: {}", e);
                Vec::new()
            }
        }
    }

    /// Load skills from all configured paths (backward compatibility)
    /// This imports file-based skills into the database
    pub async fn load_all(&self) -> Result<usize, String> {
        use crate::skills::loader::load_skills_from_directory;

        let mut loaded = 0;

        // Load bundled skills (lowest priority)
        if let Some(ref path) = self.bundled_path {
            match load_skills_from_directory(path, SkillSource::Bundled).await {
                Ok(skills) => {
                    for skill in skills {
                        if let Err(e) = self.import_file_skill(&skill) {
                            log::warn!("Failed to import bundled skill {}: {}", skill.metadata.name, e);
                        } else {
                            loaded += 1;
                        }
                    }
                }
                Err(e) => log::warn!("Failed to load bundled skills: {}", e),
            }
        }

        // Load managed skills (medium priority)
        if let Some(ref path) = self.managed_path {
            match load_skills_from_directory(path, SkillSource::Managed).await {
                Ok(skills) => {
                    for skill in skills {
                        if let Err(e) = self.import_file_skill(&skill) {
                            log::warn!("Failed to import managed skill {}: {}", skill.metadata.name, e);
                        } else {
                            loaded += 1;
                        }
                    }
                }
                Err(e) => log::warn!("Failed to load managed skills: {}", e),
            }
        }

        // Load workspace skills (highest priority)
        if let Some(ref path) = self.workspace_path {
            match load_skills_from_directory(path, SkillSource::Workspace).await {
                Ok(skills) => {
                    for skill in skills {
                        if let Err(e) = self.import_file_skill(&skill) {
                            log::warn!("Failed to import workspace skill {}: {}", skill.metadata.name, e);
                        } else {
                            loaded += 1;
                        }
                    }
                }
                Err(e) => log::warn!("Failed to load workspace skills: {}", e),
            }
        }

        log::info!("Loaded {} skills total ({} unique)", loaded, self.len());
        Ok(loaded)
    }

    /// Import a file-based Skill into the database
    fn import_file_skill(&self, skill: &Skill) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();

        let db_skill = DbSkill {
            id: None,
            name: skill.metadata.name.clone(),
            description: skill.metadata.description.clone(),
            body: skill.prompt_template.clone(),
            version: skill.metadata.version.clone(),
            author: skill.metadata.author.clone(),
            homepage: skill.metadata.homepage.clone(),
            metadata: skill.metadata.metadata.clone(),
            enabled: skill.enabled,
            requires_tools: skill.metadata.requires_tools.clone(),
            requires_binaries: skill.metadata.requires_binaries.clone(),
            arguments: skill.metadata.arguments.clone(),
            tags: skill.metadata.tags.clone(),
            subagent_type: skill.metadata.subagent_type.clone(),
            created_at: now.clone(),
            updated_at: now,
        };

        self.db.create_skill(&db_skill)
            .map_err(|e| format!("Failed to create skill in database: {}", e))?;

        Ok(())
    }

    /// Reload all skills from disk (clear and re-import from files)
    pub async fn reload(&self) -> Result<usize, String> {
        // Note: This doesn't clear the database - file-based skills will just be updated
        // Database-only skills (uploaded via ZIP) are preserved
        self.load_all().await
    }

    /// Get skills that require specific tools
    pub fn get_skills_requiring_tools(&self, tool_names: &[String]) -> Vec<Skill> {
        self.list()
            .into_iter()
            .filter(|s| {
                s.metadata
                    .requires_tools
                    .iter()
                    .any(|t| tool_names.contains(t))
            })
            .collect()
    }

    /// Search skills by name or tag
    pub fn search(&self, query: &str) -> Vec<Skill> {
        let query_lower = query.to_lowercase();
        self.list()
            .into_iter()
            .filter(|s| {
                s.metadata.name.to_lowercase().contains(&query_lower)
                    || s.metadata.description.to_lowercase().contains(&query_lower)
                    || s.metadata
                        .tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query_lower))
            })
            .collect()
    }
}

/// Create a default skill registry with standard paths
/// Uses config::skills_dir() so paths are stable regardless of CWD
pub fn create_default_registry(db: Arc<Database>) -> SkillRegistry {
    let skills_dir = PathBuf::from(crate::config::skills_dir());
    let workspace_dir = PathBuf::from(crate::config::workspace_dir());

    SkillRegistry::with_paths(
        db,
        Some(skills_dir.clone()),
        Some(skills_dir.join("managed")),
        Some(workspace_dir.join(".skills")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::SkillMetadata;

    // Tests would require a mock database - skipping for now
}
