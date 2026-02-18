//! Notes system — Obsidian-compatible markdown notes with FTS5 indexing
//!
//! Produces markdown files with YAML frontmatter, [[wikilinks]], and #tags.
//! The agent creates/edits notes via a dedicated tool; users browse them
//! in Obsidian or the read-only web UI.

pub mod file_ops;
pub mod frontmatter;
pub mod store;

pub use store::NoteStore;
