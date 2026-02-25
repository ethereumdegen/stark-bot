use crate::db::Database;
use crate::skills::types::{DbSkill, DbSkillScript, Skill, SkillSource};
use crate::skills::zip_parser::{parse_skill_md, parse_skill_zip, ParsedSkill};
use crate::skills::types::{DbSkillAbi, DbSkillPreset};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Info about a bundled skill available for restore (not currently installed).
#[derive(Serialize, Clone)]
pub struct BundledSkillInfo {
    pub name: String,
    pub description: String,
    pub version: String,
    pub tags: Vec<String>,
}

/// Registry that provides access to skills stored in the database.
/// The disk (runtime_skills_dir) is the primary store; DB is a synced index
/// used for embeddings, search, and fast lookups.
pub struct SkillRegistry {
    db: Arc<Database>,
    /// Path to the runtime skills directory (disk-primary store)
    skills_dir: PathBuf,
}

impl SkillRegistry {
    pub fn new(db: Arc<Database>, skills_dir: PathBuf) -> Self {
        SkillRegistry { db, skills_dir }
    }

    /// Get the runtime skills directory path
    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    /// Get a skill by name (from DB — synced index)
    pub fn get(&self, name: &str) -> Option<Skill> {
        match self.db.get_skill(name) {
            Ok(Some(db_skill)) => Some(db_skill.into_skill()),
            _ => None,
        }
    }

    /// List all registered skills (from DB — synced index)
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

    /// Enable or disable a skill (DB-only preference, not written to disk)
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

    /// Create a skill from a parsed ZIP file — writes to disk, then syncs to DB
    pub fn create_skill_from_zip(&self, data: &[u8]) -> Result<DbSkill, String> {
        let parsed = parse_skill_zip(data)?;
        self.create_skill_from_parsed(parsed)
    }

    /// Create a skill from markdown content, bypassing version checks (force update)
    /// Writes to disk folder, then syncs to DB
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
            requires_api_keys: metadata.requires_api_keys,
            scripts: Vec::new(),
            abis: Vec::new(),
            presets_content: None,
        };

        self.create_skill_from_parsed_force(parsed)
    }

    /// Create a skill from markdown content (SKILL.md format)
    /// Writes to disk folder, then syncs to DB
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
            requires_api_keys: metadata.requires_api_keys,
            scripts: Vec::new(),
            abis: Vec::new(),
            presets_content: None,
        };

        self.create_skill_from_parsed(parsed)
    }

    /// Create a skill from parsed skill data
    pub fn create_skill_from_parsed(&self, parsed: ParsedSkill) -> Result<DbSkill, String> {
        self.create_skill_from_parsed_internal(parsed, false)
    }

    /// Create a skill from parsed skill data, bypassing version checks (force update)
    pub fn create_skill_from_parsed_force(&self, parsed: ParsedSkill) -> Result<DbSkill, String> {
        self.create_skill_from_parsed_internal(parsed, true)
    }

    fn create_skill_from_parsed_internal(&self, parsed: ParsedSkill, force: bool) -> Result<DbSkill, String> {
        // Write to disk first
        write_skill_folder(&self.skills_dir, &parsed)
            .map_err(|e| format!("Failed to write skill to disk: {}", e))?;

        // Then sync to DB
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
            requires_api_keys: parsed.requires_api_keys,
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        // Insert skill into database
        let skill_id = if force {
            self.db.create_skill_force(&db_skill)
        } else {
            self.db.create_skill(&db_skill)
        }.map_err(|e| format!("Failed to create skill: {}", e))?;

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

        // Insert ABIs
        for abi in parsed.abis {
            let db_abi = DbSkillAbi {
                id: None,
                skill_id,
                name: abi.name,
                content: abi.content,
                created_at: now.clone(),
            };
            self.db.create_skill_abi(&db_abi)
                .map_err(|e| format!("Failed to create skill ABI: {}", e))?;
        }

        // Insert preset
        if let Some(presets_content) = parsed.presets_content {
            let db_preset = DbSkillPreset {
                id: None,
                skill_id,
                content: presets_content,
                created_at: now.clone(),
            };
            self.db.create_skill_preset(&db_preset)
                .map_err(|e| format!("Failed to create skill preset: {}", e))?;
        }

        // Return the created skill
        self.db.get_skill(&parsed.name)
            .map_err(|e| format!("Failed to retrieve created skill: {}", e))?
            .ok_or_else(|| "Skill not found after creation".to_string())
    }

    /// Create a skill from a module's skill directory (full skill folder with scripts, ABIs, etc.)
    /// Finds the `.md` file inside the dir, loads it via the standard file-based loader,
    /// and imports into DB — full parity with a normal skill folder.
    pub async fn create_skill_from_module_dir(&self, skill_dir: &Path) -> Result<DbSkill, String> {
        use crate::skills::loader::load_skill_from_file_with_dir;

        // Find the .md file: prefer {dirname}.md, then SKILL.md, then first *.md
        let dir_name = skill_dir.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let named_md = skill_dir.join(format!("{}.md", dir_name));
        let skill_md = skill_dir.join("SKILL.md");

        let md_path = if named_md.exists() {
            named_md
        } else if skill_md.exists() {
            skill_md
        } else {
            // Scan for first .md file
            let mut found = None;
            if let Ok(entries) = std::fs::read_dir(skill_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().map_or(false, |e| e == "md") && p.is_file() {
                        found = Some(p);
                        break;
                    }
                }
            }
            found.ok_or_else(|| format!("No .md file found in skill dir {}", skill_dir.display()))?
        };

        // Load the skill using the standard loader (sets skill_dir for script/ABI discovery)
        let skill = load_skill_from_file_with_dir(
            &md_path,
            SkillSource::Managed,
            Some(skill_dir.to_path_buf()),
        ).await.map_err(|e| format!("Failed to load skill from {}: {}", md_path.display(), e))?;

        // Import into DB (handles scripts, ABIs, presets)
        self.import_file_skill(&skill)
            .map_err(|e| format!("Failed to import skill '{}': {}", skill.metadata.name, e))?;

        // Return the DB skill
        self.db
            .get_skill(&skill.metadata.name)
            .map_err(|e| format!("Failed to retrieve skill: {}", e))?
            .ok_or_else(|| "Skill not found after creation".to_string())
    }

    /// Delete a skill from disk AND database
    pub fn delete_skill(&self, name: &str) -> Result<bool, String> {
        // Delete from disk (idempotent — safe if already removed)
        delete_skill_folder(&self.skills_dir, name);

        // Delete from DB
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

    /// Sync all skills from disk to database.
    /// Reads every skill folder in skills_dir, imports into DB (preserving enabled state).
    /// Also removes DB entries for skills no longer present on disk.
    pub async fn sync_to_db(&self) -> Result<usize, String> {
        use crate::skills::loader::load_skills_from_directory;

        let mut loaded = 0;
        let mut disk_skill_names: Vec<String> = Vec::new();

        match load_skills_from_directory(&self.skills_dir, SkillSource::Managed).await {
            Ok(skills) => {
                for skill in &skills {
                    disk_skill_names.push(skill.metadata.name.clone());
                }
                for skill in skills {
                    if let Err(e) = self.import_file_skill(&skill) {
                        log::warn!("Failed to import skill {}: {}", skill.metadata.name, e);
                    } else {
                        loaded += 1;
                    }
                }
            }
            Err(e) => log::warn!("Failed to load skills from disk: {}", e),
        }

        // Clean up stale DB entries for skills that no longer exist on disk
        if !disk_skill_names.is_empty() {
            if let Ok(db_skills) = self.db.list_skills() {
                for db_skill in db_skills {
                    if !disk_skill_names.contains(&db_skill.name) {
                        log::info!("Removing stale DB entry for skill '{}' (no longer on disk)", db_skill.name);
                        let _ = self.db.delete_skill(&db_skill.name);
                    }
                }
            }
        }

        log::info!("Synced {} skills from disk ({} total in DB)", loaded, self.len());
        Ok(loaded)
    }

    /// Import a file-based Skill into the database, including any scripts/ alongside SKILL.md
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
            requires_api_keys: skill.metadata.requires_api_keys.clone(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        let skill_id = self.db.create_skill(&db_skill)
            .map_err(|e| format!("Failed to create skill in database: {}", e))?;

        // Import scripts: if frontmatter declares scripts:, import only those from skill dir root.
        // Otherwise fall back to scanning scripts/ subfolder (legacy).
        if !skill.path.is_empty() {
            let skill_md_path = std::path::Path::new(&skill.path);
            if let Some(parent) = skill_md_path.parent() {
                if let Some(ref script_names) = skill.metadata.scripts {
                    // New convention: import named scripts from skill folder root
                    for script_name in script_names {
                        let script_path = parent.join(script_name);
                        if !script_path.is_file() {
                            log::warn!(
                                "Declared script '{}' not found at {} for skill '{}'",
                                script_name, script_path.display(), skill.metadata.name
                            );
                            continue;
                        }
                        let language = crate::skills::zip_parser::ParsedScript::detect_language(script_name);
                        if language == "unknown" {
                            continue;
                        }
                        let code = match std::fs::read_to_string(&script_path) {
                            Ok(c) => c,
                            Err(e) => {
                                log::warn!("Failed to read script {}: {}", script_path.display(), e);
                                continue;
                            }
                        };
                        let db_script = DbSkillScript {
                            id: None,
                            skill_id,
                            name: script_name.clone(),
                            code,
                            language,
                            created_at: now.clone(),
                        };
                        if let Err(e) = self.db.create_skill_script(&db_script) {
                            log::warn!("Failed to import script '{}' for skill '{}': {}", script_name, skill.metadata.name, e);
                        } else {
                            log::info!("Imported script '{}' for skill '{}'", script_name, skill.metadata.name);
                        }
                    }
                } else {
                    // Legacy: scan scripts/ subdirectory
                    let scripts_dir = parent.join("scripts");
                    if scripts_dir.is_dir() {
                        if let Ok(entries) = std::fs::read_dir(&scripts_dir) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if !path.is_file() {
                                    continue;
                                }
                                let file_name = match path.file_name() {
                                    Some(n) => n.to_string_lossy().to_string(),
                                    None => continue,
                                };
                                let language = crate::skills::zip_parser::ParsedScript::detect_language(&file_name);
                                if language == "unknown" {
                                    continue;
                                }
                                let code = match std::fs::read_to_string(&path) {
                                    Ok(c) => c,
                                    Err(e) => {
                                        log::warn!("Failed to read script {}: {}", path.display(), e);
                                        continue;
                                    }
                                };
                                let db_script = DbSkillScript {
                                    id: None,
                                    skill_id,
                                    name: file_name.clone(),
                                    code,
                                    language,
                                    created_at: now.clone(),
                                };
                                if let Err(e) = self.db.create_skill_script(&db_script) {
                                    log::warn!("Failed to import script '{}' for skill '{}': {}", file_name, skill.metadata.name, e);
                                } else {
                                    log::info!("Imported script '{}' for skill '{}'", file_name, skill.metadata.name);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Import ABIs from {skill_dir}/abis/ into DB
        if let Some(ref sd) = skill.skill_dir {
            let abis_dir = sd.join("abis");
            if abis_dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&abis_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map_or(false, |e| e == "json") {
                            if let Some(stem) = path.file_stem() {
                                let abi_name = stem.to_string_lossy().to_string();
                                match std::fs::read_to_string(&path) {
                                    Ok(content) => {
                                        let db_abi = DbSkillAbi {
                                            id: None,
                                            skill_id,
                                            name: abi_name.clone(),
                                            content,
                                            created_at: now.clone(),
                                        };
                                        if let Err(e) = self.db.create_skill_abi(&db_abi) {
                                            log::warn!("Failed to import ABI '{}' for skill '{}': {}", abi_name, skill.metadata.name, e);
                                        } else {
                                            log::debug!("Imported ABI '{}' for skill '{}'", abi_name, skill.metadata.name);
                                        }
                                    }
                                    Err(e) => log::warn!("Failed to read ABI {}: {}", path.display(), e),
                                }
                            }
                        }
                    }
                }
            }

            // Import web3_presets.ron into DB (also check legacy presets.ron)
            let presets_path = sd.join("web3_presets.ron");
            let presets_path = if presets_path.exists() {
                presets_path
            } else {
                sd.join("presets.ron")
            };
            if presets_path.exists() {
                match std::fs::read_to_string(&presets_path) {
                    Ok(content) => {
                        let db_preset = DbSkillPreset {
                            id: None,
                            skill_id,
                            content,
                            created_at: now.clone(),
                        };
                        if let Err(e) = self.db.create_skill_preset(&db_preset) {
                            log::warn!("Failed to import presets for skill '{}': {}", skill.metadata.name, e);
                        } else {
                            log::debug!("Imported presets for skill '{}'", skill.metadata.name);
                        }
                    }
                    Err(e) => log::warn!("Failed to read presets {}: {}", presets_path.display(), e),
                }
            }
        }

        Ok(())
    }

    /// Reload all skills from disk (clear in-memory indexes, re-sync from disk)
    pub async fn reload(&self) -> Result<usize, String> {
        // Clear in-memory indexes before reloading
        crate::tools::presets::clear_skill_web3_presets();
        crate::web3::clear_abi_index();
        let result = self.sync_to_db().await;
        // Reload ABIs and presets from DB into in-memory indexes
        crate::web3::load_all_abis_from_db(&self.db);
        crate::tools::presets::load_all_skill_presets_from_db(&self.db);
        result
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

    /// List bundled skills that are not currently installed in the runtime directory.
    /// Returns metadata for each bundled skill that's available for restore.
    pub async fn list_bundled_available(&self) -> Vec<BundledSkillInfo> {
        use crate::skills::loader::load_skills_from_directory;

        let bundled_dir = std::path::PathBuf::from(crate::config::bundled_skills_dir());
        if !bundled_dir.exists() {
            return Vec::new();
        }

        // Get installed skill names
        let installed: std::collections::HashSet<String> = self
            .list()
            .into_iter()
            .map(|s| s.metadata.name)
            .collect();

        // Load all bundled skills
        let bundled_skills = match load_skills_from_directory(&bundled_dir, SkillSource::Bundled).await {
            Ok(skills) => skills,
            Err(e) => {
                log::warn!("Failed to load bundled skills directory: {}", e);
                return Vec::new();
            }
        };

        bundled_skills
            .into_iter()
            .filter(|s| !installed.contains(&s.metadata.name))
            .map(|s| BundledSkillInfo {
                name: s.metadata.name,
                description: s.metadata.description,
                version: s.metadata.version,
                tags: s.metadata.tags,
            })
            .collect()
    }

    /// Restore a bundled skill by copying it from the bundled directory to the runtime
    /// directory and importing it into the database.
    pub async fn restore_bundled_skill(&self, name: &str) -> Result<DbSkill, String> {
        use crate::skills::loader::load_skill_from_file_with_dir;

        // Validate name
        validate_skill_name(name)?;

        let bundled_dir = std::path::PathBuf::from(crate::config::bundled_skills_dir());
        let runtime_dir = &self.skills_dir;

        let src = bundled_dir.join(name);
        let dst = runtime_dir.join(name);

        if !src.exists() {
            return Err(format!("Bundled skill '{}' not found", name));
        }
        if dst.exists() {
            return Err(format!("Skill '{}' already exists in runtime directory", name));
        }

        // Copy the bundled skill directory to runtime
        crate::config::copy_dir_recursive(&src, &dst)
            .map_err(|e| format!("Failed to copy bundled skill '{}': {}", name, e))?;

        // Find the skill markdown file in the copied directory
        let named_md = dst.join(format!("{}.md", name));
        let legacy_md = dst.join("SKILL.md");
        let skill_file = if named_md.exists() {
            named_md
        } else if legacy_md.exists() {
            legacy_md
        } else {
            // Clean up the copy on failure
            let _ = std::fs::remove_dir_all(&dst);
            return Err(format!("No SKILL.md found in bundled skill '{}'", name));
        };

        // Load the skill from the copied file
        let skill = load_skill_from_file_with_dir(&skill_file, SkillSource::Managed, Some(dst))
            .await
            .map_err(|e| format!("Failed to load restored skill '{}': {}", name, e))?;

        // Import into DB
        self.import_file_skill(&skill)
            .map_err(|e| format!("Failed to import restored skill '{}': {}", name, e))?;

        // Return the DB skill
        self.db
            .get_skill(name)
            .map_err(|e| format!("Failed to retrieve restored skill: {}", e))?
            .ok_or_else(|| "Skill not found after restore".to_string())
    }

    // -----------------------------------------------------------------------
    // Module skill helpers — single entry points for sync/disable/delete
    // -----------------------------------------------------------------------

    /// Resolve the skill name embedded in a module's skill markdown.
    fn resolve_module_skill_name(module: &dyn crate::modules::Module) -> Option<String> {
        if let Some(skill_dir) = module.skill_dir() {
            let dir_name = skill_dir.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let named_md = skill_dir.join(format!("{}.md", dir_name));
            let skill_md = skill_dir.join("SKILL.md");
            let md_path = if named_md.exists() { named_md } else if skill_md.exists() { skill_md } else { return None };
            if let Ok(content) = std::fs::read_to_string(&md_path) {
                if let Ok((meta, _)) = crate::skills::zip_parser::parse_skill_md(&content) {
                    return Some(meta.name);
                }
            }
        }
        if let Some(skill_md) = module.skill_content() {
            if let Ok((meta, _)) = crate::skills::zip_parser::parse_skill_md(skill_md) {
                return Some(meta.name);
            }
        }
        None
    }

    /// Load (or reload) a module's skill into the DB and enable it.
    /// Call this after install or enable.
    pub async fn sync_module_skill(&self, module_name: &str) {
        let registry = crate::modules::ModuleRegistry::new();
        let module = match registry.get(module_name) {
            Some(m) => m,
            None => return,
        };
        if let Some(skill_dir) = module.skill_dir() {
            match self.create_skill_from_module_dir(skill_dir).await {
                Ok(s) => { self.set_enabled(&s.name, true); }
                Err(e) => log::warn!("Failed to sync skill for module '{}': {}", module_name, e),
            }
        } else if let Some(skill_md) = module.skill_content() {
            match self.create_skill_from_markdown(skill_md) {
                Ok(s) => { self.set_enabled(&s.name, true); }
                Err(e) => log::warn!("Failed to sync skill for module '{}': {}", module_name, e),
            }
        }
    }

    /// Disable a module's skill without deleting it.
    pub fn disable_module_skill(&self, module_name: &str) {
        let registry = crate::modules::ModuleRegistry::new();
        if let Some(module) = registry.get(module_name) {
            if let Some(skill_name) = Self::resolve_module_skill_name(module.as_ref()) {
                self.set_enabled(&skill_name, false);
            }
        }
    }

    /// Delete a module's skill entirely.
    pub fn delete_module_skill(&self, module_name: &str) {
        let registry = crate::modules::ModuleRegistry::new();
        if let Some(module) = registry.get(module_name) {
            if let Some(skill_name) = Self::resolve_module_skill_name(module.as_ref()) {
                let _ = self.delete_skill(&skill_name);
            }
        }
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

// ---------------------------------------------------------------------------
// Disk operations
// ---------------------------------------------------------------------------

/// Write a parsed skill to a folder on disk: {skills_dir}/{name}/SKILL.md + scripts + ABIs + presets
pub fn write_skill_folder(skills_dir: &Path, parsed: &ParsedSkill) -> Result<(), String> {
    // Validate skill name to prevent path traversal
    validate_skill_name(&parsed.name)?;

    let skill_dir = skills_dir.join(&parsed.name);
    std::fs::create_dir_all(&skill_dir)
        .map_err(|e| format!("Failed to create skill directory: {}", e))?;

    // Write SKILL.md (frontmatter + body)
    let skill_md = reconstruct_skill_md(parsed);
    let md_path = skill_dir.join("SKILL.md");
    std::fs::write(&md_path, &skill_md)
        .map_err(|e| format!("Failed to write SKILL.md: {}", e))?;

    // Write scripts
    for script in &parsed.scripts {
        let script_path = skill_dir.join(&script.name);
        std::fs::write(&script_path, &script.code)
            .map_err(|e| format!("Failed to write script {}: {}", script.name, e))?;
    }

    // Write ABIs
    if !parsed.abis.is_empty() {
        let abis_dir = skill_dir.join("abis");
        std::fs::create_dir_all(&abis_dir)
            .map_err(|e| format!("Failed to create abis directory: {}", e))?;
        for abi in &parsed.abis {
            let abi_path = abis_dir.join(format!("{}.json", abi.name));
            std::fs::write(&abi_path, &abi.content)
                .map_err(|e| format!("Failed to write ABI {}: {}", abi.name, e))?;
        }
    }

    // Write web3 presets
    if let Some(ref presets) = parsed.presets_content {
        let presets_path = skill_dir.join("web3_presets.ron");
        std::fs::write(&presets_path, presets)
            .map_err(|e| format!("Failed to write web3_presets.ron: {}", e))?;
    }

    Ok(())
}

/// Reconstruct SKILL.md content from a ParsedSkill (YAML frontmatter + body)
pub fn reconstruct_skill_md(parsed: &ParsedSkill) -> String {
    let mut lines = Vec::new();
    lines.push("---".to_string());
    lines.push(format!("name: \"{}\"", parsed.name.replace('"', "\\\"")));
    lines.push(format!("description: \"{}\"", parsed.description.replace('"', "\\\"")));
    lines.push(format!("version: \"{}\"", parsed.version.replace('"', "\\\"")));

    if let Some(ref author) = parsed.author {
        lines.push(format!("author: \"{}\"", author.replace('"', "\\\"")));
    }
    if let Some(ref homepage) = parsed.homepage {
        lines.push(format!("homepage: \"{}\"", homepage.replace('"', "\\\"")));
    }
    if let Some(ref metadata) = parsed.metadata {
        // metadata is often JSON — quote it to prevent YAML colon issues
        lines.push(format!("metadata: \"{}\"", metadata.replace('"', "\\\"")));
    }
    if let Some(ref subagent_type) = parsed.subagent_type {
        lines.push(format!("subagent_type: {}", subagent_type));
    }

    if !parsed.requires_tools.is_empty() {
        lines.push(format!("requires_tools: [{}]", parsed.requires_tools.join(", ")));
    }
    if !parsed.requires_binaries.is_empty() {
        lines.push(format!("requires_binaries: [{}]", parsed.requires_binaries.join(", ")));
    }
    if !parsed.tags.is_empty() {
        lines.push(format!("tags: [{}]", parsed.tags.join(", ")));
    }

    if !parsed.scripts.is_empty() {
        let script_names: Vec<&str> = parsed.scripts.iter().map(|s| s.name.as_str()).collect();
        lines.push(format!("scripts: [{}]", script_names.join(", ")));
    }

    if !parsed.arguments.is_empty() {
        lines.push("arguments:".to_string());
        for (name, arg) in &parsed.arguments {
            lines.push(format!("  {}:", name));
            lines.push(format!("    description: \"{}\"", arg.description.replace('"', "\\\"")));
            if arg.required {
                lines.push("    required: true".to_string());
            }
            if let Some(ref default) = arg.default {
                lines.push(format!("    default: \"{}\"", default.replace('"', "\\\"")));
            }
        }
    }

    if !parsed.requires_api_keys.is_empty() {
        lines.push("requires_api_keys:".to_string());
        for (key_name, key_def) in &parsed.requires_api_keys {
            lines.push(format!("  {}:", key_name));
            lines.push(format!("    description: \"{}\"", key_def.description.replace('"', "\\\"")));
            if !key_def.secret {
                lines.push("    secret: false".to_string());
            }
        }
    }

    lines.push("---".to_string());
    lines.push(String::new());
    lines.push(parsed.body.clone());

    lines.join("\n")
}

/// Reconstruct SKILL.md from a DbSkill (for backup restore)
pub fn reconstruct_skill_md_from_db(db_skill: &DbSkill) -> String {
    let parsed = ParsedSkill {
        name: db_skill.name.clone(),
        description: db_skill.description.clone(),
        body: db_skill.body.clone(),
        version: db_skill.version.clone(),
        author: db_skill.author.clone(),
        homepage: db_skill.homepage.clone(),
        metadata: db_skill.metadata.clone(),
        requires_tools: db_skill.requires_tools.clone(),
        requires_binaries: db_skill.requires_binaries.clone(),
        arguments: db_skill.arguments.clone(),
        tags: db_skill.tags.clone(),
        subagent_type: db_skill.subagent_type.clone(),
        requires_api_keys: db_skill.requires_api_keys.clone(),
        scripts: Vec::new(),
        abis: Vec::new(),
        presets_content: None,
    };
    reconstruct_skill_md(&parsed)
}

/// Validate a skill name for filesystem safety
fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Skill name cannot be empty".to_string());
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err("Skill name contains invalid characters".to_string());
    }
    if name.starts_with('.') {
        return Err("Skill name cannot start with '.'".to_string());
    }
    Ok(())
}

/// Delete a skill folder from disk
pub fn delete_skill_folder(skills_dir: &Path, name: &str) {
    if let Err(e) = validate_skill_name(name) {
        log::warn!("Refusing to delete skill with invalid name '{}': {}", name, e);
        return;
    }
    let skill_dir = skills_dir.join(name);
    if skill_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&skill_dir) {
            log::warn!("Failed to delete skill folder {:?}: {}", skill_dir, e);
        } else {
            log::info!("Deleted skill folder: {:?}", skill_dir);
        }
    }
}

/// Create a default skill registry with the runtime skills directory
pub fn create_default_registry(db: Arc<Database>) -> SkillRegistry {
    let skills_dir = PathBuf::from(crate::config::runtime_skills_dir());
    SkillRegistry::new(db, skills_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::SkillMetadata;

    // Tests would require a mock database - skipping for now
}
