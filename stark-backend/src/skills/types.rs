use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Source of a skill (simplified — all skills are disk-primary now)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    /// Bundled with the application (legacy compat)
    Bundled,
    /// Managed / installed (disk-primary)
    Managed,
    /// Workspace-specific skill (legacy compat)
    Workspace,
}

impl SkillSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillSource::Bundled => "bundled",
            SkillSource::Managed => "managed",
            SkillSource::Workspace => "workspace",
        }
    }

    pub fn from_str(s: &str) -> Option<SkillSource> {
        match s.to_lowercase().as_str() {
            "bundled" => Some(SkillSource::Bundled),
            "managed" | "disk" => Some(SkillSource::Managed),
            "workspace" => Some(SkillSource::Workspace),
            _ => None,
        }
    }

    /// Priority for skill loading (higher = takes precedence)
    pub fn priority(&self) -> u8 {
        match self {
            SkillSource::Workspace => 3,
            SkillSource::Managed => 2,
            SkillSource::Bundled => 1,
        }
    }
}

/// API key requirement declared by a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillApiKey {
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_secret")]
    pub secret: bool,
}

fn default_secret() -> bool {
    true
}

/// Argument definition for a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillArgument {
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

/// Skill metadata from SKILL.md frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub requires_tools: Vec<String>,
    #[serde(default)]
    pub requires_binaries: Vec<String>,
    #[serde(default)]
    pub arguments: HashMap<String, SkillArgument>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub metadata: Option<String>,
    #[serde(default, alias = "sets_agent_subtype")]
    pub subagent_type: Option<String>,
    #[serde(default)]
    pub requires_api_keys: HashMap<String, SkillApiKey>,
    /// Script files bundled with this skill (e.g. ["predict.py"])
    #[serde(default)]
    pub scripts: Option<Vec<String>>,
    /// ABI files bundled with this skill (e.g. ["0x_settler"])
    #[serde(default)]
    pub abis: Option<Vec<String>>,
    /// Skill-local web3 presets RON file (e.g. "web3_presets.ron")
    #[serde(default)]
    pub presets_file: Option<String>,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

impl Default for SkillMetadata {
    fn default() -> Self {
        SkillMetadata {
            name: String::new(),
            description: String::new(),
            version: default_version(),
            requires_tools: vec![],
            requires_binaries: vec![],
            arguments: HashMap::new(),
            tags: vec![],
            author: None,
            homepage: None,
            metadata: None,
            subagent_type: None,
            requires_api_keys: HashMap::new(),
            scripts: None,
            abis: None,
            presets_file: None,
        }
    }
}

/// A complete skill definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub metadata: SkillMetadata,
    /// The skill's prompt template (content after frontmatter)
    pub prompt_template: String,
    /// Source of this skill (bundled, managed, workspace)
    pub source: SkillSource,
    /// Path to the SKILL.md file
    pub path: String,
    /// Whether the skill is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Directory where the skill lives on disk (for resolving scripts, ABIs, presets)
    #[serde(default)]
    pub skill_dir: Option<PathBuf>,
}

fn default_enabled() -> bool {
    true
}

impl Skill {
    /// Render the skill prompt with provided arguments
    pub fn render_prompt(&self, args: &HashMap<String, String>) -> String {
        let mut prompt = self.prompt_template.clone();

        // Replace argument placeholders {{arg_name}} with values
        for (name, arg_def) in &self.metadata.arguments {
            let placeholder = format!("{{{{{}}}}}", name);
            let value = args
                .get(name)
                .cloned()
                .or_else(|| arg_def.default.clone())
                .unwrap_or_default();
            prompt = prompt.replace(&placeholder, &value);
        }

        prompt
    }

    /// Check if all required binaries are available
    pub fn check_binaries(&self) -> Result<(), Vec<String>> {
        let missing: Vec<String> = self
            .metadata
            .requires_binaries
            .iter()
            .filter(|bin| which::which(bin).is_err())
            .cloned()
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }

    /// Validate required arguments are provided
    pub fn validate_args(&self, args: &HashMap<String, String>) -> Result<(), Vec<String>> {
        let missing: Vec<String> = self
            .metadata
            .arguments
            .iter()
            .filter(|(_, def)| def.required && def.default.is_none())
            .map(|(name, _)| name.clone())
            .filter(|name| !args.contains_key(name))
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }
}

/// Database record for skills (new database-backed schema)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSkill {
    pub id: Option<i64>,
    pub name: String,
    pub description: String,
    pub body: String,                    // The prompt template
    pub version: String,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub metadata: Option<String>,
    pub enabled: bool,
    pub requires_tools: Vec<String>,
    pub requires_binaries: Vec<String>,
    pub arguments: HashMap<String, SkillArgument>,
    pub tags: Vec<String>,
    pub subagent_type: Option<String>,
    pub requires_api_keys: HashMap<String, SkillApiKey>,
    pub created_at: String,
    pub updated_at: String,
}

impl DbSkill {
    /// Convert to Skill for API compatibility
    pub fn into_skill(self) -> Skill {
        Skill {
            metadata: SkillMetadata {
                name: self.name,
                description: self.description,
                version: self.version,
                requires_tools: self.requires_tools,
                requires_binaries: self.requires_binaries,
                arguments: self.arguments,
                tags: self.tags,
                author: self.author,
                homepage: self.homepage,
                metadata: self.metadata,
                subagent_type: self.subagent_type,
                requires_api_keys: self.requires_api_keys,
                scripts: None,
                abis: None,
                presets_file: None,
            },
            prompt_template: self.body,
            source: SkillSource::Managed, // All DB skills are "managed"
            path: String::new(),          // No file path for DB skills
            enabled: self.enabled,
            skill_dir: None,
        }
    }
}

/// Database record for skill ABIs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSkillAbi {
    pub id: Option<i64>,
    pub skill_id: i64,
    pub name: String,
    pub content: String,
    pub created_at: String,
}

/// Database record for skill presets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSkillPreset {
    pub id: Option<i64>,
    pub skill_id: i64,
    pub content: String,
    pub created_at: String,
}

/// Database record for skill scripts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbSkillScript {
    pub id: Option<i64>,
    pub skill_id: i64,
    pub name: String,
    pub code: String,
    pub language: String,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_render_prompt() {
        let mut arguments = HashMap::new();
        arguments.insert(
            "path".to_string(),
            SkillArgument {
                description: "Path to review".to_string(),
                required: false,
                default: Some(".".to_string()),
            },
        );

        let skill = Skill {
            metadata: SkillMetadata {
                name: "test".to_string(),
                description: "Test skill".to_string(),
                arguments,
                ..Default::default()
            },
            prompt_template: "Review code at {{path}}".to_string(),
            source: SkillSource::Bundled,
            path: "/test/SKILL.md".to_string(),
            enabled: true,
            skill_dir: None,
        };

        // With argument provided
        let mut args = HashMap::new();
        args.insert("path".to_string(), "./src".to_string());
        assert_eq!(skill.render_prompt(&args), "Review code at ./src");

        // With default value
        let empty_args = HashMap::new();
        assert_eq!(skill.render_prompt(&empty_args), "Review code at .");
    }

    #[test]
    fn test_skill_source_priority() {
        assert!(SkillSource::Workspace.priority() > SkillSource::Managed.priority());
        assert!(SkillSource::Managed.priority() > SkillSource::Bundled.priority());
    }
}
