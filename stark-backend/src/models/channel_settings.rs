//! Channel-specific settings that can be configured per channel instance.
//!
//! Each channel type can have different available settings. The schema
//! defines what settings are available, and values are stored per-channel.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumIter, EnumString};

use super::channel::ChannelType;

/// Controls how verbose tool call/result output is in channel messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, EnumString, AsRefStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ToolOutputVerbosity {
    /// Show tool name and full parameters/content
    #[default]
    Full,
    /// Show only tool name, no parameters or content details
    Minimal,
    /// Don't show tool calls/results at all
    None,
}

impl ToolOutputVerbosity {
    /// Parse from string, defaulting to Full if invalid
    pub fn from_str_or_default(s: &str) -> Self {
        s.parse().unwrap_or_default()
    }
}

/// Available setting keys for channels.
/// Each variant maps to a specific channel type's configurable option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumString, AsRefStr, EnumIter)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ChannelSettingKey {
    /// Common: Auto-start this channel when the server boots (after restore from backup)
    AutoStartOnBoot,
    /// Discord: Comma-separated list of Discord user IDs with admin access
    /// If empty, falls back to Discord's built-in Administrator permission
    DiscordAdminUserIds,
    /// Twitter: Bot's Twitter handle without @ (e.g., "starkbotai")
    TwitterBotHandle,
    /// Twitter: Numeric Twitter user ID (required for mentions API)
    TwitterBotUserId,
    /// Twitter: Poll interval in seconds (min 60, default 120)
    TwitterPollIntervalSecs,
}

impl ChannelSettingKey {
    /// Get the display label for this setting
    pub fn label(&self) -> &'static str {
        match self {
            Self::AutoStartOnBoot => "Auto-Start on Boot",
            Self::DiscordAdminUserIds => "Admin User IDs (Optional)",
            Self::TwitterBotHandle => "Bot Handle",
            Self::TwitterBotUserId => "Bot User ID",
            Self::TwitterPollIntervalSecs => "Poll Interval (seconds)",
        }
    }

    /// Get the description for this setting
    pub fn description(&self) -> &'static str {
        match self {
            Self::AutoStartOnBoot => {
                "Automatically start this channel when the server boots or restores from backup. \
                 Useful for ensuring your bot is always running after container updates."
            }
            Self::DiscordAdminUserIds => {
                "Comma-separated Discord user IDs that have full agent access. \
                 If left empty, Discord's Administrator permission is used. \
                 If any IDs are set, ONLY those users have admin access (Discord admin role is ignored). \
                 Get your ID: enable Developer Mode in Discord settings, then right-click your username."
            }
            Self::TwitterBotHandle => {
                "Your bot's Twitter handle without the @ symbol (e.g., 'starkbotai'). \
                 This is used to remove self-mentions from incoming tweets."
            }
            Self::TwitterBotUserId => {
                "Your bot's numeric Twitter user ID. Required for the mentions API. \
                 You can find this by looking up your account at tweeterid.com."
            }
            Self::TwitterPollIntervalSecs => {
                "How often to check for new mentions in seconds. Minimum is 60 seconds. \
                 Higher values reduce API usage but increase response latency."
            }
        }
    }

    /// Get the input type for the UI
    pub fn input_type(&self) -> SettingInputType {
        match self {
            Self::AutoStartOnBoot => SettingInputType::Toggle,
            Self::DiscordAdminUserIds => SettingInputType::Text,
            Self::TwitterBotHandle => SettingInputType::Text,
            Self::TwitterBotUserId => SettingInputType::Text,
            Self::TwitterPollIntervalSecs => SettingInputType::Number,
        }
    }

    /// Get the placeholder text for the input
    pub fn placeholder(&self) -> &'static str {
        match self {
            Self::AutoStartOnBoot => "",
            Self::DiscordAdminUserIds => "123456789012345678, 987654321098765432",
            Self::TwitterBotHandle => "starkbotai",
            Self::TwitterBotUserId => "1234567890123456789",
            Self::TwitterPollIntervalSecs => "120",
        }
    }

    /// Get the available options for select inputs
    pub fn options(&self) -> Option<Vec<(&'static str, &'static str)>> {
        None
    }

    /// Get the default value for this setting
    pub fn default_value(&self) -> &'static str {
        match self {
            Self::AutoStartOnBoot => "false",
            Self::DiscordAdminUserIds => "",
            Self::TwitterBotHandle => "",
            Self::TwitterBotUserId => "",
            Self::TwitterPollIntervalSecs => "120",
        }
    }

    /// Check if this setting applies to all channel types (common setting)
    pub fn is_common(&self) -> bool {
        matches!(self, Self::AutoStartOnBoot)
    }
}

/// Input type for rendering the setting in the UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingInputType {
    /// Single-line text input
    Text,
    /// Multi-line text area
    TextArea,
    /// Boolean toggle
    Toggle,
    /// Numeric input
    Number,
    /// Dropdown select
    Select,
}

/// Option for select input type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

/// Definition of a channel setting for the schema API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSettingDefinition {
    pub key: String,
    pub label: String,
    pub description: String,
    pub input_type: SettingInputType,
    pub placeholder: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<SelectOption>>,
    pub default_value: String,
}

impl From<ChannelSettingKey> for ChannelSettingDefinition {
    fn from(key: ChannelSettingKey) -> Self {
        Self {
            key: key.as_ref().to_string(),
            label: key.label().to_string(),
            description: key.description().to_string(),
            input_type: key.input_type(),
            placeholder: key.placeholder().to_string(),
            options: key.options().map(|opts| {
                opts.into_iter()
                    .map(|(value, label)| SelectOption {
                        value: value.to_string(),
                        label: label.to_string(),
                    })
                    .collect()
            }),
            default_value: key.default_value().to_string(),
        }
    }
}

/// A stored channel setting value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSetting {
    pub channel_id: i64,
    pub setting_key: String,
    pub setting_value: String,
}

/// Response for channel settings API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSettingsResponse {
    pub success: bool,
    pub settings: Vec<ChannelSetting>,
}

/// Response for channel settings schema API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSettingsSchemaResponse {
    pub success: bool,
    pub channel_type: String,
    pub settings: Vec<ChannelSettingDefinition>,
}

/// Request to update channel settings
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateChannelSettingsRequest {
    pub settings: Vec<SettingUpdate>,
}

/// A single setting update
#[derive(Debug, Clone, Deserialize)]
pub struct SettingUpdate {
    pub key: String,
    pub value: String,
}

/// Get common settings that apply to all channel types
fn get_common_settings() -> Vec<ChannelSettingDefinition> {
    vec![
        ChannelSettingKey::AutoStartOnBoot.into(),
    ]
}

/// Get the available settings for a channel type
pub fn get_settings_for_channel_type(channel_type: ChannelType) -> Vec<ChannelSettingDefinition> {
    let mut settings = get_common_settings();

    let type_specific: Vec<ChannelSettingDefinition> = match channel_type {
        ChannelType::Discord => vec![
            ChannelSettingKey::DiscordAdminUserIds.into(),
        ],
        ChannelType::Telegram => vec![
            // No custom settings yet
        ],
        ChannelType::Slack => vec![
            // No custom settings yet
        ],
        ChannelType::Twitter => vec![
            ChannelSettingKey::TwitterBotHandle.into(),
            ChannelSettingKey::TwitterBotUserId.into(),
            ChannelSettingKey::TwitterPollIntervalSecs.into(),
        ],
    };

    settings.extend(type_specific);
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setting_key_serialization() {
        let key = ChannelSettingKey::DiscordAdminUserIds;
        assert_eq!(key.as_ref(), "discord_admin_user_ids");
    }

    #[test]
    fn test_discord_settings() {
        let settings = get_settings_for_channel_type(ChannelType::Discord);
        // 1 common setting (auto_start_on_boot) + 1 Discord-specific setting
        assert_eq!(settings.len(), 2);
        assert_eq!(settings[0].key, "auto_start_on_boot"); // Common setting first
        assert_eq!(settings[1].key, "discord_admin_user_ids");
    }

    #[test]
    fn test_telegram_settings() {
        let settings = get_settings_for_channel_type(ChannelType::Telegram);
        // Only common setting (auto_start_on_boot)
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].key, "auto_start_on_boot");
    }

    #[test]
    fn test_tool_verbosity_parsing() {
        assert_eq!(ToolOutputVerbosity::from_str_or_default("full"), ToolOutputVerbosity::Full);
        assert_eq!(ToolOutputVerbosity::from_str_or_default("minimal"), ToolOutputVerbosity::Minimal);
        assert_eq!(ToolOutputVerbosity::from_str_or_default("none"), ToolOutputVerbosity::None);
        assert_eq!(ToolOutputVerbosity::from_str_or_default("invalid"), ToolOutputVerbosity::Full);
    }
}
