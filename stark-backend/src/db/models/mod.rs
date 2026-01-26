//! Database model modules - split from sqlite.rs for better organization
//!
//! Each module contains `impl Database` blocks for a specific table or related tables.

mod auth_sessions;
mod api_keys;
mod channels;
mod agent_settings;
mod chat_sessions;
mod identities;
mod memories;
mod tool_configs;
mod skills;
mod cron_jobs;
mod heartbeat;
mod gmail;
