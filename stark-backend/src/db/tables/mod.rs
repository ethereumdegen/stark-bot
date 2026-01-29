//! Database model modules - extends Database with domain-specific methods
//!
//! Each module adds `impl Database` blocks with methods for a specific table group.

mod auth;           // auth_sessions, auth_challenges
mod api_keys;       // external_api_keys
mod channels;       // external_channels
mod agent_settings; // agent_settings
mod bot_settings;   // bot_settings
mod chat_sessions;  // chat_sessions, session_messages (+ compaction)
mod identities;     // identity_links
mod memories;       // memories
mod tool_configs;   // tool_configs, tool_executions
mod skills;         // skills, skill_scripts
mod cron_jobs;      // cron_jobs, cron_job_runs
mod heartbeat;      // heartbeat_configs
mod gmail;          // gmail_configs
