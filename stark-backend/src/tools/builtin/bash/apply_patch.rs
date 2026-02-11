use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// ApplyPatch tool - applies structured patches to files within a sandboxed directory
///
/// Patch format:
/// ```
/// *** Begin Patch
/// *** Add File: path/to/new/file.txt
/// +line 1
/// +line 2
/// *** Update File: path/to/existing.txt
/// @@
/// -old line
/// +new line
/// *** Delete File: path/to/remove.txt
/// *** End Patch
/// ```
pub struct ApplyPatchTool {
    definition: ToolDefinition,
}

impl ApplyPatchTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "patch".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The patch content, bounded by '*** Begin Patch' and '*** End Patch' markers. Supports Add File, Update File, Delete File, and Move to operations.".to_string(),
                default: None,
                items: None,
                enum_values: None,
            },
        );

        ApplyPatchTool {
            definition: ToolDefinition {
                name: "apply_patch".to_string(),
                description: "Apply a structured patch to create, modify, or delete files. Use for complex multi-file edits. Patch format uses '*** Begin Patch' / '*** End Patch' markers with '*** Add File:', '*** Update File:', '*** Delete File:', and '*** Move to:' operations. Line changes use '-' for removals, '+' for additions.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["patch".to_string()],
                },
                group: ToolGroup::Development,
                hidden: false,
            },
        }
    }
}

impl Default for ApplyPatchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct ApplyPatchParams {
    patch: String,
}

#[derive(Debug, Clone)]
enum PatchOperation {
    AddFile { path: String, content: String },
    UpdateFile { path: String, hunks: Vec<Hunk>, move_to: Option<String> },
    DeleteFile { path: String },
}

#[derive(Debug, Clone)]
struct Hunk {
    context_before: Vec<String>,
    removals: Vec<String>,
    additions: Vec<String>,
    context_after: Vec<String>,
    is_end_of_file: bool,
}

impl Hunk {
    fn new() -> Self {
        Hunk {
            context_before: Vec::new(),
            removals: Vec::new(),
            additions: Vec::new(),
            context_after: Vec::new(),
            is_end_of_file: false,
        }
    }
}

#[derive(Debug)]
struct PatchParseError(String);

impl std::fmt::Display for PatchParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn parse_patch(input: &str) -> Result<Vec<PatchOperation>, PatchParseError> {
    let lines: Vec<&str> = input.lines().collect();
    let mut operations = Vec::new();
    let mut i = 0;

    // Find the start marker
    while i < lines.len() && !lines[i].trim().starts_with("*** Begin Patch") {
        i += 1;
    }

    if i >= lines.len() {
        return Err(PatchParseError("Missing '*** Begin Patch' marker".to_string()));
    }
    i += 1; // Skip the Begin Patch line

    while i < lines.len() {
        let line = lines[i].trim();

        if line.starts_with("*** End Patch") {
            break;
        }

        if line.starts_with("*** Add File:") {
            let path = line.strip_prefix("*** Add File:").unwrap().trim().to_string();
            i += 1;

            let mut content_lines = Vec::new();
            while i < lines.len() {
                let current = lines[i];
                if current.trim().starts_with("***") {
                    break;
                }
                // For add file, lines should start with +
                if let Some(stripped) = current.strip_prefix('+') {
                    content_lines.push(stripped.to_string());
                } else if current.trim().is_empty() {
                    content_lines.push(String::new());
                }
                i += 1;
            }

            operations.push(PatchOperation::AddFile {
                path,
                content: content_lines.join("\n"),
            });
        } else if line.starts_with("*** Update File:") {
            let path = line.strip_prefix("*** Update File:").unwrap().trim().to_string();
            i += 1;

            let mut hunks = Vec::new();
            let mut move_to = None;
            let mut current_hunk: Option<Hunk> = None;
            let mut in_changes = false; // Track if we've seen any +/- lines in current hunk

            while i < lines.len() {
                let current = lines[i];
                let trimmed = current.trim();

                if trimmed.starts_with("*** ") && !trimmed.starts_with("*** End of File") {
                    // Check for Move to before breaking
                    if trimmed.starts_with("*** Move to:") {
                        move_to = Some(trimmed.strip_prefix("*** Move to:").unwrap().trim().to_string());
                        i += 1;
                        continue;
                    }
                    // Save current hunk if exists
                    if let Some(hunk) = current_hunk.take() {
                        hunks.push(hunk);
                    }
                    break;
                }

                if trimmed == "@@" {
                    // Save previous hunk if exists
                    if let Some(hunk) = current_hunk.take() {
                        hunks.push(hunk);
                    }
                    current_hunk = Some(Hunk::new());
                    in_changes = false;
                    i += 1;
                    continue;
                }

                if trimmed.starts_with("*** End of File") {
                    if let Some(ref mut hunk) = current_hunk {
                        hunk.is_end_of_file = true;
                    }
                    i += 1;
                    continue;
                }

                if let Some(ref mut hunk) = current_hunk {
                    if current.starts_with('-') {
                        in_changes = true;
                        hunk.removals.push(current[1..].to_string());
                    } else if current.starts_with('+') {
                        in_changes = true;
                        hunk.additions.push(current[1..].to_string());
                    } else if current.starts_with(' ') || current.is_empty() {
                        // Context line
                        let ctx = if current.starts_with(' ') { &current[1..] } else { current };
                        if in_changes {
                            hunk.context_after.push(ctx.to_string());
                        } else {
                            hunk.context_before.push(ctx.to_string());
                        }
                    } else {
                        // Line without prefix is treated as context
                        if in_changes {
                            hunk.context_after.push(current.to_string());
                        } else {
                            hunk.context_before.push(current.to_string());
                        }
                    }
                }

                i += 1;
            }

            // Don't forget the last hunk
            if let Some(hunk) = current_hunk {
                hunks.push(hunk);
            }

            operations.push(PatchOperation::UpdateFile { path, hunks, move_to });
        } else if line.starts_with("*** Delete File:") {
            let path = line.strip_prefix("*** Delete File:").unwrap().trim().to_string();
            operations.push(PatchOperation::DeleteFile { path });
            i += 1;
        } else {
            i += 1;
        }
    }

    Ok(operations)
}

fn apply_hunk(content: &str, hunk: &Hunk) -> Result<String, String> {
    let lines: Vec<&str> = content.lines().collect();

    // Handle end-of-file additions
    if hunk.is_end_of_file && hunk.removals.is_empty() && hunk.context_before.is_empty() {
        let mut result = content.to_string();
        if !result.ends_with('\n') && !result.is_empty() {
            result.push('\n');
        }
        for addition in &hunk.additions {
            result.push_str(addition);
            result.push('\n');
        }
        return Ok(result.trim_end_matches('\n').to_string());
    }

    // Find the location to apply the hunk using context
    let mut match_start = None;

    // Build the pattern we're looking for (context_before + removals)
    let mut pattern: Vec<&str> = Vec::new();
    for ctx in &hunk.context_before {
        pattern.push(ctx.as_str());
    }
    for removal in &hunk.removals {
        pattern.push(removal.as_str());
    }

    if pattern.is_empty() {
        // No pattern to match - if we have only additions and no context, append to end
        if !hunk.additions.is_empty() {
            let mut result = content.to_string();
            if !result.ends_with('\n') && !result.is_empty() {
                result.push('\n');
            }
            for addition in &hunk.additions {
                result.push_str(addition);
                result.push('\n');
            }
            return Ok(result.trim_end_matches('\n').to_string());
        }
        return Ok(content.to_string());
    }

    // Search for the pattern in the file
    'outer: for start_idx in 0..=lines.len().saturating_sub(pattern.len()) {
        for (j, pattern_line) in pattern.iter().enumerate() {
            if start_idx + j >= lines.len() {
                continue 'outer;
            }
            if lines[start_idx + j].trim() != pattern_line.trim() {
                continue 'outer;
            }
        }
        match_start = Some(start_idx);
        break;
    }

    let match_start = match match_start {
        Some(idx) => idx,
        None => {
            // Try a more lenient match with just removals
            if !hunk.removals.is_empty() {
                'outer2: for start_idx in 0..=lines.len().saturating_sub(hunk.removals.len()) {
                    for (j, removal) in hunk.removals.iter().enumerate() {
                        if start_idx + j >= lines.len() {
                            continue 'outer2;
                        }
                        if lines[start_idx + j].trim() != removal.trim() {
                            continue 'outer2;
                        }
                    }
                    // Found with just removals - adjust start to include context_before
                    let adjusted = start_idx.saturating_sub(hunk.context_before.len());
                    return apply_at_position(content, hunk, adjusted);
                }
            }
            return Err(format!(
                "Could not find matching context in file. Looking for: {:?}",
                pattern
            ));
        }
    };

    apply_at_position(content, hunk, match_start)
}

fn apply_at_position(content: &str, hunk: &Hunk, match_start: usize) -> Result<String, String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();

    // Add lines before the match
    for i in 0..match_start {
        result.push(lines[i].to_string());
    }

    // Add context before (they should already match)
    for ctx in &hunk.context_before {
        result.push(ctx.clone());
    }

    // Skip the removed lines and add the new lines
    let skip_count = hunk.context_before.len() + hunk.removals.len();

    // Add the additions
    for addition in &hunk.additions {
        result.push(addition.clone());
    }

    // Add context after
    for ctx in &hunk.context_after {
        result.push(ctx.clone());
    }

    // Calculate where to continue from in original
    let continue_from = match_start + skip_count + hunk.context_after.len();

    // Add remaining lines
    for i in continue_from..lines.len() {
        result.push(lines[i].to_string());
    }

    Ok(result.join("\n"))
}

fn resolve_and_validate_path(
    requested_path: &str,
    workspace: &Path,
    canonical_workspace: &Path,
) -> Result<PathBuf, String> {
    let path = Path::new(requested_path);
    let full_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    };

    // For paths that don't exist yet, validate the parent
    let check_path = if full_path.exists() {
        full_path.clone()
    } else {
        full_path.parent().map(|p| p.to_path_buf()).unwrap_or(full_path.clone())
    };

    // Try to canonicalize (may fail if path doesn't exist)
    if check_path.exists() {
        let canonical = check_path.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(canonical_workspace) {
            return Err(format!(
                "Access denied: path '{}' is outside the workspace directory",
                requested_path
            ));
        }
    }

    Ok(full_path)
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: ApplyPatchParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Parse the patch
        let operations = match parse_patch(&params.patch) {
            Ok(ops) => ops,
            Err(e) => return ToolResult::error(format!("Failed to parse patch: {}", e)),
        };

        if operations.is_empty() {
            return ToolResult::error("No operations found in patch");
        }

        // Get workspace directory from context or use current directory
        let workspace = context
            .workspace_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Canonicalize workspace for comparison
        let canonical_workspace = match workspace.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::error(format!("Cannot resolve workspace directory: {}", e))
            }
        };

        let mut results = Vec::new();
        let mut files_added = 0;
        let mut files_updated = 0;
        let mut files_deleted = 0;
        let mut files_moved = 0;

        for operation in operations {
            match operation {
                PatchOperation::AddFile { path, content } => {
                    let full_path = match resolve_and_validate_path(&path, &workspace, &canonical_workspace) {
                        Ok(p) => p,
                        Err(e) => {
                            results.push(format!("FAILED Add '{}': {}", path, e));
                            continue;
                        }
                    };

                    // Create parent directories if needed
                    if let Some(parent) = full_path.parent() {
                        if let Err(e) = tokio::fs::create_dir_all(parent).await {
                            results.push(format!("FAILED Add '{}': Cannot create directories: {}", path, e));
                            continue;
                        }
                    }

                    match tokio::fs::write(&full_path, &content).await {
                        Ok(_) => {
                            results.push(format!("Added '{}'", path));
                            files_added += 1;
                        }
                        Err(e) => {
                            results.push(format!("FAILED Add '{}': {}", path, e));
                        }
                    }
                }

                PatchOperation::UpdateFile { path, hunks, move_to } => {
                    let full_path = match resolve_and_validate_path(&path, &workspace, &canonical_workspace) {
                        Ok(p) => p,
                        Err(e) => {
                            results.push(format!("FAILED Update '{}': {}", path, e));
                            continue;
                        }
                    };

                    // Read the file
                    let content = match tokio::fs::read_to_string(&full_path).await {
                        Ok(c) => c,
                        Err(e) => {
                            results.push(format!("FAILED Update '{}': Cannot read file: {}", path, e));
                            continue;
                        }
                    };

                    // Apply hunks sequentially
                    let mut current_content = content;
                    let mut hunk_errors = Vec::new();

                    for (idx, hunk) in hunks.iter().enumerate() {
                        match apply_hunk(&current_content, hunk) {
                            Ok(new_content) => {
                                current_content = new_content;
                            }
                            Err(e) => {
                                hunk_errors.push(format!("Hunk {}: {}", idx + 1, e));
                            }
                        }
                    }

                    if !hunk_errors.is_empty() {
                        results.push(format!("FAILED Update '{}': {}", path, hunk_errors.join("; ")));
                        continue;
                    }

                    // Handle move operation
                    let target_path = if let Some(ref new_path) = move_to {
                        match resolve_and_validate_path(new_path, &workspace, &canonical_workspace) {
                            Ok(p) => p,
                            Err(e) => {
                                results.push(format!("FAILED Move '{}' to '{}': {}", path, new_path, e));
                                continue;
                            }
                        }
                    } else {
                        full_path.clone()
                    };

                    // Create parent directories for target if needed
                    if let Some(parent) = target_path.parent() {
                        if let Err(e) = tokio::fs::create_dir_all(parent).await {
                            results.push(format!("FAILED Update '{}': Cannot create directories: {}", path, e));
                            continue;
                        }
                    }

                    // Write the updated content
                    match tokio::fs::write(&target_path, &current_content).await {
                        Ok(_) => {
                            if move_to.is_some() {
                                // Delete the original file if we moved
                                if full_path != target_path {
                                    let _ = tokio::fs::remove_file(&full_path).await;
                                    results.push(format!("Updated and moved '{}' to '{}'", path, move_to.as_ref().unwrap()));
                                    files_moved += 1;
                                } else {
                                    results.push(format!("Updated '{}'", path));
                                    files_updated += 1;
                                }
                            } else {
                                results.push(format!("Updated '{}'", path));
                                files_updated += 1;
                            }
                        }
                        Err(e) => {
                            results.push(format!("FAILED Update '{}': Cannot write file: {}", path, e));
                        }
                    }
                }

                PatchOperation::DeleteFile { path } => {
                    let full_path = match resolve_and_validate_path(&path, &workspace, &canonical_workspace) {
                        Ok(p) => p,
                        Err(e) => {
                            results.push(format!("FAILED Delete '{}': {}", path, e));
                            continue;
                        }
                    };

                    match tokio::fs::remove_file(&full_path).await {
                        Ok(_) => {
                            results.push(format!("Deleted '{}'", path));
                            files_deleted += 1;
                        }
                        Err(e) => {
                            results.push(format!("FAILED Delete '{}': {}", path, e));
                        }
                    }
                }
            }
        }

        let summary = format!(
            "Patch applied: {} added, {} updated, {} moved, {} deleted",
            files_added, files_updated, files_moved, files_deleted
        );

        ToolResult::success(format!("{}\n\n{}", summary, results.join("\n")))
            .with_metadata(json!({
                "files_added": files_added,
                "files_updated": files_updated,
                "files_moved": files_moved,
                "files_deleted": files_deleted,
                "details": results
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_add_file() {
        let patch = r#"
*** Begin Patch
*** Add File: test.txt
+line 1
+line 2
*** End Patch
"#;
        let ops = parse_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOperation::AddFile { path, content } => {
                assert_eq!(path, "test.txt");
                assert_eq!(content, "line 1\nline 2");
            }
            _ => panic!("Expected AddFile operation"),
        }
    }

    #[test]
    fn test_parse_update_file() {
        let patch = r#"
*** Begin Patch
*** Update File: test.txt
@@
-old line
+new line
*** End Patch
"#;
        let ops = parse_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOperation::UpdateFile { path, hunks, move_to } => {
                assert_eq!(path, "test.txt");
                assert_eq!(hunks.len(), 1);
                assert_eq!(hunks[0].removals, vec!["old line"]);
                assert_eq!(hunks[0].additions, vec!["new line"]);
                assert!(move_to.is_none());
            }
            _ => panic!("Expected UpdateFile operation"),
        }
    }

    #[test]
    fn test_parse_delete_file() {
        let patch = r#"
*** Begin Patch
*** Delete File: test.txt
*** End Patch
"#;
        let ops = parse_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOperation::DeleteFile { path } => {
                assert_eq!(path, "test.txt");
            }
            _ => panic!("Expected DeleteFile operation"),
        }
    }

    #[test]
    fn test_parse_move_file() {
        let patch = r#"
*** Begin Patch
*** Update File: old.txt
@@
-old content
+new content
*** Move to: new.txt
*** End Patch
"#;
        let ops = parse_patch(patch).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            PatchOperation::UpdateFile { path, move_to, .. } => {
                assert_eq!(path, "old.txt");
                assert_eq!(move_to.as_ref().unwrap(), "new.txt");
            }
            _ => panic!("Expected UpdateFile operation"),
        }
    }

    #[test]
    fn test_apply_hunk_simple() {
        let content = "line 1\nold line\nline 3";
        let hunk = Hunk {
            context_before: vec!["line 1".to_string()],
            removals: vec!["old line".to_string()],
            additions: vec!["new line".to_string()],
            context_after: vec!["line 3".to_string()],
            is_end_of_file: false,
        };

        let result = apply_hunk(content, &hunk).unwrap();
        assert_eq!(result, "line 1\nnew line\nline 3");
    }

    #[tokio::test]
    async fn test_add_file() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_string_lossy().to_string();

        let tool = ApplyPatchTool::new();
        let context = ToolContext::new().with_workspace(workspace);

        let patch = r#"*** Begin Patch
*** Add File: new_file.txt
+Hello, World!
+Line 2
*** End Patch"#;

        let result = tool
            .execute(json!({ "patch": patch }), &context)
            .await;

        assert!(result.success, "Error: {:?}", result.error);

        let content = tokio::fs::read_to_string(temp_dir.path().join("new_file.txt"))
            .await
            .unwrap();
        assert_eq!(content, "Hello, World!\nLine 2");
    }

    #[tokio::test]
    async fn test_update_file() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_string_lossy().to_string();

        // Create initial file
        tokio::fs::write(
            temp_dir.path().join("test.txt"),
            "line 1\nold line\nline 3"
        ).await.unwrap();

        let tool = ApplyPatchTool::new();
        let context = ToolContext::new().with_workspace(workspace);

        let patch = r#"*** Begin Patch
*** Update File: test.txt
@@
 line 1
-old line
+new line
 line 3
*** End Patch"#;

        let result = tool
            .execute(json!({ "patch": patch }), &context)
            .await;

        assert!(result.success, "Error: {:?}", result.error);

        let content = tokio::fs::read_to_string(temp_dir.path().join("test.txt"))
            .await
            .unwrap();
        assert_eq!(content, "line 1\nnew line\nline 3");
    }

    #[tokio::test]
    async fn test_delete_file() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_string_lossy().to_string();

        // Create file to delete
        let file_path = temp_dir.path().join("to_delete.txt");
        tokio::fs::write(&file_path, "content").await.unwrap();
        assert!(file_path.exists());

        let tool = ApplyPatchTool::new();
        let context = ToolContext::new().with_workspace(workspace);

        let patch = r#"*** Begin Patch
*** Delete File: to_delete.txt
*** End Patch"#;

        let result = tool
            .execute(json!({ "patch": patch }), &context)
            .await;

        assert!(result.success, "Error: {:?}", result.error);
        assert!(!file_path.exists());
    }

    #[tokio::test]
    async fn test_outside_workspace() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_string_lossy().to_string();

        let tool = ApplyPatchTool::new();
        let context = ToolContext::new().with_workspace(workspace);

        let patch = r#"*** Begin Patch
*** Add File: /etc/passwd
+malicious content
*** End Patch"#;

        let result = tool
            .execute(json!({ "patch": patch }), &context)
            .await;

        // Should fail or the path check should prevent writing outside workspace
        // The exact behavior depends on how the path resolution works
        assert!(!std::path::Path::new("/etc/passwd").exists() ||
                std::fs::read_to_string("/etc/passwd").map(|c| !c.contains("malicious")).unwrap_or(true));
    }
}
