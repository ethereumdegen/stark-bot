//! Configuration for Discord hooks

use std::collections::HashSet;
use std::sync::Arc;

use crate::db::Database;
use crate::models::ChannelSettingKey;
use serenity::all::{Context, Message, Permissions};

/// Configuration for the Discord hooks module
#[derive(Debug, Clone)]
pub struct DiscordHooksConfig {
    /// Discord user IDs that have admin access (full agentic commands)
    /// If empty, falls back to Discord's built-in Administrator permission
    admin_user_ids: HashSet<String>,
    /// Whether to require @mention in server channels (default: true)
    pub require_mention_in_servers: bool,
    /// Whether to allow DMs without @mention (default: true)
    pub allow_dm_without_mention: bool,
}

impl DiscordHooksConfig {
    /// Create a new config from channel settings in the database
    ///
    /// Reads the discord_admin_user_ids setting for the given channel
    pub fn from_channel_settings(db: &Arc<Database>, channel_id: i64) -> Self {
        let admin_ids: HashSet<String> = db
            .get_channel_setting(channel_id, ChannelSettingKey::DiscordAdminUserIds.as_ref())
            .ok()
            .flatten()
            .map(|ids| {
                ids.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        if admin_ids.is_empty() {
            log::info!(
                "Discord hooks: No admin user IDs configured for channel {}. \
                Configure in channel settings to enable admin commands.",
                channel_id
            );
        } else {
            log::info!(
                "Discord hooks: Configured {} admin user ID(s) for channel {}",
                admin_ids.len(),
                channel_id
            );
        }

        Self {
            admin_user_ids: admin_ids,
            require_mention_in_servers: true,
            allow_dm_without_mention: true,
        }
    }

    /// Create a new config from environment variables (legacy/fallback)
    ///
    /// Reads:
    /// - `DISCORD_ADMIN_USER_IDS`: Comma-separated list of Discord user IDs
    #[allow(dead_code)]
    pub fn from_env() -> Self {
        let admin_ids: HashSet<String> = std::env::var("DISCORD_ADMIN_USER_IDS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if admin_ids.is_empty() {
            log::warn!(
                "Discord hooks: No admin user IDs configured. \
                Set DISCORD_ADMIN_USER_IDS env var to enable admin commands."
            );
        } else {
            log::info!(
                "Discord hooks: Configured {} admin user ID(s)",
                admin_ids.len()
            );
        }

        Self {
            admin_user_ids: admin_ids,
            require_mention_in_servers: true,
            allow_dm_without_mention: true,
        }
    }

    /// Create an empty config (no admins)
    pub fn empty() -> Self {
        Self {
            admin_user_ids: HashSet::new(),
            require_mention_in_servers: true,
            allow_dm_without_mention: true,
        }
    }

    /// Create a config with specific admin IDs
    pub fn with_admins(admin_ids: Vec<String>) -> Self {
        Self {
            admin_user_ids: admin_ids.into_iter().collect(),
            require_mention_in_servers: true,
            allow_dm_without_mention: true,
        }
    }

    /// Check if a user ID is an admin (sync version for explicit admin IDs)
    pub fn is_admin_by_id(&self, user_id: &str) -> bool {
        self.admin_user_ids.contains(user_id)
    }

    /// Check if a user is admin - if explicit admin_user_ids are configured,
    /// only those users are admins. Otherwise falls back to Discord Administrator permission.
    pub async fn is_admin(&self, user_id: &str, msg: &Message, ctx: &Context) -> bool {
        // If explicit admin user IDs are configured, only use that list
        if !self.admin_user_ids.is_empty() {
            return self.admin_user_ids.contains(user_id);
        }

        // Fallback: no explicit admins configured, use Discord Administrator permission
        Self::has_discord_admin_permission(msg, ctx).await
    }

    /// Check if the message author has Discord Administrator permission
    pub async fn has_discord_admin_permission(msg: &Message, ctx: &Context) -> bool {
        // DMs don't have guild permissions - treat as admin for convenience
        let guild_id = match msg.guild_id {
            Some(id) => id,
            None => {
                log::debug!("Discord hooks: DM detected, treating as admin");
                return true;
            }
        };

        // First check if user is guild owner
        match ctx.http.get_guild(guild_id).await {
            Ok(guild) => {
                if guild.owner_id == msg.author.id {
                    log::debug!("Discord hooks: User {} is guild owner", msg.author.name);
                    return true;
                }
            }
            Err(e) => {
                log::warn!("Discord hooks: Failed to get guild: {}", e);
            }
        }

        // Get the member's permissions in this guild
        match guild_id.member(&ctx.http, msg.author.id).await {
            Ok(member) => {
                // Check if any of the member's roles have Administrator permission
                // Get the guild to access roles
                match ctx.http.get_guild(guild_id).await {
                    Ok(guild) => {
                        for role_id in &member.roles {
                            if let Some(role) = guild.roles.get(role_id) {
                                if role.permissions.contains(Permissions::ADMINISTRATOR) {
                                    log::debug!(
                                        "Discord hooks: User {} has Administrator via role {}",
                                        msg.author.name,
                                        role.name
                                    );
                                    return true;
                                }
                            }
                        }
                        log::debug!(
                            "Discord hooks: User {} does not have Administrator permission",
                            msg.author.name
                        );
                        false
                    }
                    Err(e) => {
                        log::warn!("Discord hooks: Failed to get guild for role check: {}", e);
                        false
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "Discord hooks: Failed to get member for {}: {}",
                    msg.author.name,
                    e
                );
                false
            }
        }
    }

    /// Get the number of configured admins
    pub fn admin_count(&self) -> usize {
        self.admin_user_ids.len()
    }

    /// Check if any explicit admins are configured
    pub fn has_explicit_admins(&self) -> bool {
        !self.admin_user_ids.is_empty()
    }
}

impl Default for DiscordHooksConfig {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_config() {
        let config = DiscordHooksConfig::empty();
        assert!(!config.is_admin_by_id("123"));
        assert_eq!(config.admin_count(), 0);
        assert!(!config.has_explicit_admins());
    }

    #[test]
    fn test_with_admins() {
        let config = DiscordHooksConfig::with_admins(vec![
            "123456789".to_string(),
            "987654321".to_string(),
        ]);

        assert!(config.is_admin_by_id("123456789"));
        assert!(config.is_admin_by_id("987654321"));
        assert!(!config.is_admin_by_id("111111111"));
        assert_eq!(config.admin_count(), 2);
        assert!(config.has_explicit_admins());
    }
}
