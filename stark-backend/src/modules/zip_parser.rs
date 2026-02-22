//! Module ZIP parser — extracts module packages from ZIP files.
//!
//! Modeled on `skills/zip_parser.rs`. Finds `module.toml` at root or one level
//! deep, parses the manifest, and collects all files for extraction.

use super::manifest::ModuleManifest;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;
use zip::ZipArchive;

/// A module parsed from a ZIP file, ready to be extracted to disk.
#[derive(Debug, Clone)]
pub struct ParsedModule {
    pub manifest: ModuleManifest,
    pub module_name: String,
    /// All files in the ZIP, keyed by their path relative to the module root.
    pub files: HashMap<String, Vec<u8>>,
}

/// Parse a ZIP file containing a module package.
///
/// Looks for `module.toml` at the ZIP root or one directory level deep.
/// Returns the parsed manifest and all files ready for extraction.
pub fn parse_module_zip(data: &[u8]) -> Result<ParsedModule, String> {
    const MAX_ZIP_BYTES: usize = crate::disk_quota::MAX_SKILL_ZIP_BYTES;

    let cursor = Cursor::new(data);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("Failed to read ZIP file: {}", e))?;

    // Pre-check: sum of uncompressed sizes declared in the archive
    {
        let mut total_uncompressed: u64 = 0;
        for i in 0..archive.len() {
            if let Ok(file) = archive.by_index(i) {
                total_uncompressed += file.size();
            }
        }
        if total_uncompressed > MAX_ZIP_BYTES as u64 {
            return Err(format!(
                "ZIP bomb protection: total uncompressed size ({} bytes) exceeds the 10MB limit.",
                total_uncompressed,
            ));
        }
    }

    // First pass: find module.toml
    let mut manifest_path: Option<String> = None;
    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read ZIP entry: {}", e))?;
        let name = file.name().to_string();
        if name.ends_with('/') {
            continue;
        }

        let normalized = name.trim_start_matches('/').to_string();

        // Match module.toml at root or one level deep (e.g. "my_module/module.toml")
        if normalized == "module.toml"
            || (normalized.ends_with("/module.toml")
                && normalized.matches('/').count() == 1)
        {
            manifest_path = Some(name.clone());
            break;
        }
    }

    let manifest_zip_path = manifest_path
        .ok_or_else(|| "ZIP file must contain a module.toml file (at root or one directory deep)".to_string())?;

    // Read and parse the manifest
    let manifest = {
        let mut file = archive
            .by_name(&manifest_zip_path)
            .map_err(|e| format!("Failed to read module.toml: {}", e))?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| format!("Failed to read module.toml content: {}", e))?;
        ModuleManifest::from_str(&content)?
    };

    let module_name = manifest.module.name.clone();

    // Determine the base directory prefix to strip from file paths
    let base_dir = manifest_zip_path
        .rsplit('/')
        .nth(1)
        .unwrap_or("");

    // Second pass: collect all files
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read ZIP entry: {}", e))?;

        let name = file.name().to_string();
        if name.ends_with('/') {
            continue;
        }

        // Strip the base directory prefix
        let relative = if !base_dir.is_empty() {
            let prefix = format!("{}/", base_dir);
            match name.strip_prefix(&prefix) {
                Some(rel) => rel.to_string(),
                None => continue, // file outside the module directory
            }
        } else {
            name.trim_start_matches('/').to_string()
        };

        if relative.is_empty() {
            continue;
        }

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| format!("Failed to read file '{}': {}", relative, e))?;
        files.insert(relative, buf);
    }

    Ok(ParsedModule {
        manifest,
        module_name,
        files,
    })
}

/// Extract a parsed module to disk under the given modules directory.
///
/// Creates `modules_dir/{module_name}/` and writes all files.
/// Returns the path to the created module directory.
/// Errors if the directory already exists (must uninstall first).
pub fn extract_module_to_dir(
    parsed: &ParsedModule,
    modules_dir: &Path,
) -> Result<std::path::PathBuf, String> {
    let module_dir = modules_dir.join(&parsed.module_name);

    if module_dir.exists() {
        return Err(format!(
            "Module directory '{}' already exists. Uninstall the existing module first.",
            module_dir.display()
        ));
    }

    std::fs::create_dir_all(&module_dir)
        .map_err(|e| format!("Failed to create module directory: {}", e))?;

    for (relative_path, data) in &parsed.files {
        let file_path = module_dir.join(relative_path);

        // Ensure parent directories exist
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory for '{}': {}", relative_path, e))?;
        }

        std::fs::write(&file_path, data)
            .map_err(|e| format!("Failed to write '{}': {}", relative_path, e))?;
    }

    // Make scripts executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for relative_path in parsed.files.keys() {
            let ext = relative_path.rsplit('.').next().unwrap_or("");
            if matches!(ext, "sh" | "py" | "rb") {
                let file_path = module_dir.join(relative_path);
                let _ = std::fs::set_permissions(
                    &file_path,
                    std::fs::Permissions::from_mode(0o755),
                );
            }
        }
    }

    Ok(module_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn make_test_zip(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut buf));
            let options = SimpleFileOptions::default();
            for (name, content) in files {
                writer.start_file(*name, options).unwrap();
                writer.write_all(content.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn test_parse_module_zip_root_manifest() {
        let toml = r#"
[module]
name = "test_module"
version = "1.0.0"
description = "A test module"

[service]
default_port = 9200
command = "python service.py"
"#;
        let zip_data = make_test_zip(&[
            ("module.toml", toml),
            ("service.py", "print('hello')"),
        ]);

        let parsed = parse_module_zip(&zip_data).unwrap();
        assert_eq!(parsed.module_name, "test_module");
        assert_eq!(parsed.manifest.service.default_port, 9200);
        assert!(parsed.files.contains_key("module.toml"));
        assert!(parsed.files.contains_key("service.py"));
    }

    #[test]
    fn test_parse_module_zip_nested_manifest() {
        let toml = r#"
[module]
name = "nested_module"
version = "0.1.0"
description = "Nested test"

[service]
default_port = 9300
"#;
        let zip_data = make_test_zip(&[
            ("nested_module/module.toml", toml),
            ("nested_module/service.py", "print('nested')"),
            ("nested_module/config/settings.json", "{}"),
        ]);

        let parsed = parse_module_zip(&zip_data).unwrap();
        assert_eq!(parsed.module_name, "nested_module");
        assert!(parsed.files.contains_key("module.toml"));
        assert!(parsed.files.contains_key("service.py"));
        assert!(parsed.files.contains_key("config/settings.json"));
    }

    #[test]
    fn test_parse_module_zip_no_manifest() {
        let zip_data = make_test_zip(&[("readme.txt", "no manifest here")]);
        let result = parse_module_zip(&zip_data);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("module.toml"));
    }
}
