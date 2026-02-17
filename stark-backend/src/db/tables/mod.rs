//! Database model modules - extends Database with domain-specific methods
//!
//! Each module adds `impl Database` blocks with methods for a specific table group.

pub mod agent_subtypes; // agent_subtypes (configurable agent toolboxes)
mod auth;           // auth_sessions, auth_challenges
mod api_keys;       // external_api_keys
mod channels;       // external_channels
mod channel_settings; // channel_settings (per-channel config)
mod agent_settings; // agent_settings
mod bot_settings;   // bot_settings
mod chat_sessions;  // chat_sessions, session_messages (+ compaction)
mod identities;     // identity_links
mod tool_configs;   // tool_configs, tool_executions
mod skills;         // skills, skill_scripts
mod cron_jobs;      // cron_jobs, cron_job_runs
mod heartbeat;      // heartbeat_configs
mod gmail;          // gmail_configs
mod agent_contexts; // agent_contexts (multi-agent orchestrator state)
mod twitter_mentions; // twitter_processed_mentions (track processed tweets)
pub mod broadcasted_transactions; // broadcasted_transactions (crypto tx history)
pub mod mind_nodes;  // mind_nodes, mind_node_connections (mind map feature)
pub mod telegram_chat_log; // telegram_chat_messages (passive chat log for readHistory)
pub mod x402_payment_limits; // x402_payment_limits (per-call max amounts per token)
pub mod kanban;          // kanban_items (kanban board task management)
pub mod modules;         // installed_modules (plugin system registry)
pub mod telemetry;       // execution_spans, rollouts, attempts, resource_versions
pub mod special_roles;   // special_roles, special_role_assignments (enriched safe mode)
