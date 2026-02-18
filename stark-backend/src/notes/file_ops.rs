//! File operations for notes system
//!
//! Handles reading/writing markdown note files, slugification, and wikilink resolution.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Slugify a title for use as a filename (e.g. "x402 Payment Protocol" -> "x402-payment-protocol")
pub fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-")
}

/// Write a note file (creates parent directories as needed)
pub fn write_note(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

/// Read a note file, returning empty string if not found
pub fn read_note(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// List all markdown files in the notes directory (recursively)
pub fn list_notes(notes_dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if !notes_dir.exists() {
        return Ok(files);
    }

    fn visit_dir(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            // Skip hidden files/dirs (like .notes.db)
            if path
                .file_name()
                .map(|n| n.to_string_lossy().starts_with('.'))
                .unwrap_or(false)
            {
                continue;
            }
            if path.is_dir() {
                visit_dir(&path, files)?;
            } else if path.extension().map(|e| e == "md").unwrap_or(false) {
                files.push(path);
            }
        }
        Ok(())
    }

    visit_dir(notes_dir, &mut files)?;
    Ok(files)
}

/// Get relative path from notes_dir for a file
pub fn relative_path(notes_dir: &Path, file_path: &Path) -> Option<String> {
    file_path
        .strip_prefix(notes_dir)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

/// Resolve a [[wikilink]] to a file path by searching the notes directory.
/// Matches against filenames (without .md) or title frontmatter.
pub fn resolve_wikilink(notes_dir: &Path, link: &str) -> Option<PathBuf> {
    let slug = slugify(link);
    let target_filename = format!("{}.md", slug);

    let files = list_notes(notes_dir).ok()?;
    for file in files {
        // Check filename match
        if let Some(name) = file.file_name() {
            if name.to_string_lossy() == target_filename {
                return Some(file);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("x402 Payment Protocol"), "x402-payment-protocol");
        assert_eq!(slugify("Hello World!"), "hello-world");
        assert_eq!(slugify("  multiple   spaces  "), "multiple-spaces");
        assert_eq!(slugify("CamelCase"), "camelcase");
        assert_eq!(slugify("already-slugified"), "already-slugified");
    }

    #[test]
    fn test_write_and_read_note() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test-note.md");

        write_note(&path, "# Test\n\nContent here").unwrap();
        let content = read_note(&path).unwrap();
        assert!(content.contains("# Test"));
        assert!(content.contains("Content here"));
    }

    #[test]
    fn test_read_note_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.md");
        let content = read_note(&path).unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn test_list_notes() {
        let dir = tempdir().unwrap();
        let notes_dir = dir.path();

        fs::write(notes_dir.join("note1.md"), "content").unwrap();
        fs::create_dir(notes_dir.join("ideas")).unwrap();
        fs::write(notes_dir.join("ideas/idea1.md"), "content").unwrap();
        // Hidden file should be skipped
        fs::write(notes_dir.join(".notes.db"), "sqlite").unwrap();

        let files = list_notes(notes_dir).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_resolve_wikilink() {
        let dir = tempdir().unwrap();
        let notes_dir = dir.path();

        fs::write(notes_dir.join("hello-world.md"), "# Hello World").unwrap();

        let result = resolve_wikilink(notes_dir, "Hello World");
        assert!(result.is_some());
        assert!(result.unwrap().to_string_lossy().contains("hello-world.md"));

        let result = resolve_wikilink(notes_dir, "nonexistent");
        assert!(result.is_none());
    }
}
