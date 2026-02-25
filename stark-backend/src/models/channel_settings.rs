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
    /// Like Minimal, but throttle status message edits (max one every few seconds)
    /// and handle rate-limit backoff. Recommended for external APIs (Telegram, Discord).
    MinimalThrottled,
    /// Don't show tool calls/results at all
    None,
}

impl ToolOutputVerbosity {
    /// Parse from string, defaulting to Full if invalid
    pub fn from_str_or_default(s: &str) -> Self {
        s.parse().unwrap_or_default()
    }

    /// Whether this verbosity mode uses throttled status updates
    pub fn is_throttled(&self) -> bool {
        matches!(self, Self::MinimalThrottled)
    }

    /// The effective display verbosity (MinimalThrottled renders the same as Minimal)
    pub fn display_verbosity(&self) -> Self {
        match self {
            Self::MinimalThrottled => Self::Minimal,
            other => *other,
        }
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
    /// Discord: Bot authentication token
    DiscordBotToken,
    /// Discord: Comma-separated list of Discord user IDs with admin access
    /// If empty, falls back to Discord's built-in Administrator permission
    DiscordAdminUserIds,
    /// Telegram: Bot authentication token from @BotFather
    TelegramBotToken,
    /// Slack: Bot OAuth token (xoxb-...)
    SlackBotToken,
    /// Slack: App-level token for Socket Mode (xapp-...)
    SlackAppToken,
    /// Twitter: Bot's Twitter handle without @ (e.g., "starkbotai")
    TwitterBotHandle,
    /// Twitter: Numeric Twitter user ID (required for mentions API)
    TwitterBotUserId,
    /// Twitter: Poll interval in seconds (min 60, default 120)
    TwitterPollIntervalSecs,
    /// Twitter: Chance (percentage) of replying to each mention
    TwitterReplyChance,
    /// Twitter: Maximum number of mentions to reply to per hour
    TwitterMaxMentionsPerHour,
    /// Twitter: Admin X account numeric user ID — tweets from this account bypass safe mode
    TwitterAdminXAccount,
    /// Telegram: Admin user ID — messages from this user bypass safe mode
    TelegramAdminUserId,
    /// Slack: Comma-separated list of Slack user IDs with admin access
    SlackAdminUserIds,
    /// External Gateway: API token for authenticating external clients
    ExternalChannelApiToken,
    /// External Gateway: Enable safe mode (restricts tool access for untrusted input)
    ExternalChannelSafeMode,
}

impl ChannelSettingKey {
    /// Get the display label for this setting
    pub fn label(&self) -> &'static str {
        match self {
            Self::AutoStartOnBoot => "Auto-Start on Boot",
            Self::DiscordBotToken => "Bot Token",
            Self::DiscordAdminUserIds => "Admin User IDs (Optional)",
            Self::TelegramBotToken => "Bot Token",
            Self::SlackBotToken => "Bot Token",
            Self::SlackAppToken => "App Token (Socket Mode)",
            Self::TwitterBotHandle => "Bot Handle",
            Self::TwitterBotUserId => "Bot User ID",
            Self::TwitterPollIntervalSecs => "Poll Interval (seconds)",
            Self::TwitterReplyChance => "Reply Chance",
            Self::TwitterMaxMentionsPerHour => "Max Replies Per Hour",
            Self::TwitterAdminXAccount => "Admin X User ID (Optional)",
            Self::TelegramAdminUserId => "Admin User ID (Optional)",
            Self::SlackAdminUserIds => "Admin User IDs (Optional)",
            Self::ExternalChannelApiToken => "API Token",
            Self::ExternalChannelSafeMode => "Safe Mode",
        }
    }

    /// Get the description for this setting
    pub fn description(&self) -> &'static str {
        match self {
            Self::AutoStartOnBoot => {
                "Automatically start this channel when the server boots or restores from backup. \
                 Useful for ensuring your bot is always running after container updates."
            }
            Self::DiscordBotToken => {
                "Your Discord bot token from the Discord Developer Portal. \
                 Found under Bot > Token in your application settings."
            }
            Self::DiscordAdminUserIds => {
                "Comma-separated Discord user IDs that have full agent access. \
                 If left empty, Discord's Administrator permission is used. \
                 If any IDs are set, ONLY those users have admin access (Discord admin role is ignored). \
                 Get your ID: enable Developer Mode in Discord settings, then right-click your username."
            }
            Self::TelegramBotToken => {
                "Your Telegram bot token from @BotFather. \
                 Create a bot with /newbot and copy the token provided."
            }
            Self::SlackBotToken => {
                "Your Slack bot OAuth token (starts with xoxb-). \
                 Found under OAuth & Permissions in your Slack app settings."
            }
            Self::SlackAppToken => {
                "Your Slack app-level token for Socket Mode (starts with xapp-). \
                 Found under Basic Information > App-Level Tokens in your Slack app settings."
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
            Self::TwitterReplyChance => {
                "Percentage chance of processing each non-admin mention. \
                 Non-admin mentions are not auto-replied to by the gateway — they trigger \
                 the twitter_mentioned hook instead. Configure an agent persona with a \
                 hooks/twitter_mentioned.md template to reply to mentions."
            }
            Self::TwitterMaxMentionsPerHour => {
                "Maximum number of mentions to process per hour. Once the limit is reached, \
                 remaining mentions are skipped until the next hour. Set to 0 for unlimited."
            }
            Self::TwitterAdminXAccount => {
                "Numeric X (Twitter) user ID of an admin account. Admin tweets get a direct \
                 gateway reply with full tool access. Non-admin mentions only fire the \
                 twitter_mentioned hook — configure an agent persona to handle replies. \
                 Use the numeric ID (not the handle) for security — handles can be changed or spoofed. \
                 Find your ID at tweeterid.com. \
                 WARNING: This account will have full agent access — only set this to an account you control."
            }
            Self::TelegramAdminUserId => {
                "Telegram numeric user ID of the admin. Messages from this user get full agent access; \
                 all other users are restricted to safe mode. If not set, all users get full access \
                 (backwards-compatible). Find your ID by messaging @userinfobot on Telegram. \
                 WARNING: This account gets full agent access — only set this to a user you control."
            }
            Self::SlackAdminUserIds => {
                "Comma-separated Slack user IDs that have full agent access. \
                 If left empty, all users get full access. \
                 If any IDs are set, ONLY those users have admin access; all others use safe mode. \
                 Find user IDs in Slack profile settings or via the Slack API."
            }
            Self::ExternalChannelApiToken => {
                "Secret token used by external clients to authenticate. \
                 Click the dice icon to generate a secure random token. \
                 Treat this like a password — anyone with the token can chat with your agent."
            }
            Self::ExternalChannelSafeMode => {
                "When enabled, external gateway requests are treated as untrusted input — \
                 tool access is restricted to a safe subset. Disable for full agent access \
                 (only if you trust the clients connecting to this channel)."
            }
        }
    }

    /// Get the input type for the UI
    pub fn input_type(&self) -> SettingInputType {
        match self {
            Self::AutoStartOnBoot => SettingInputType::Toggle,
            Self::DiscordBotToken => SettingInputType::Text,
            Self::DiscordAdminUserIds => SettingInputType::Text,
            Self::TelegramBotToken => SettingInputType::Text,
            Self::SlackBotToken => SettingInputType::Text,
            Self::SlackAppToken => SettingInputType::Text,
            Self::TwitterBotHandle => SettingInputType::Text,
            Self::TwitterBotUserId => SettingInputType::Text,
            Self::TwitterPollIntervalSecs => SettingInputType::Number,
            Self::TwitterReplyChance => SettingInputType::Select,
            Self::TwitterMaxMentionsPerHour => SettingInputType::Number,
            Self::TwitterAdminXAccount => SettingInputType::Text,
            Self::TelegramAdminUserId => SettingInputType::Text,
            Self::SlackAdminUserIds => SettingInputType::Text,
            Self::ExternalChannelApiToken => SettingInputType::Text,
            Self::ExternalChannelSafeMode => SettingInputType::Toggle,
        }
    }

    /// Get the placeholder text for the input
    pub fn placeholder(&self) -> &'static str {
        match self {
            Self::AutoStartOnBoot => "",
            Self::DiscordBotToken => "MTIz...abc",
            Self::DiscordAdminUserIds => "123456789012345678, 987654321098765432",
            Self::TelegramBotToken => "123456:ABC-DEF...",
            Self::SlackBotToken => "xoxb-...",
            Self::SlackAppToken => "xapp-...",
            Self::TwitterBotHandle => "starkbotai",
            Self::TwitterBotUserId => "1234567890123456789",
            Self::TwitterPollIntervalSecs => "120",
            Self::TwitterReplyChance => "",
            Self::TwitterMaxMentionsPerHour => "0",
            Self::TwitterAdminXAccount => "1234567890123456789",
            Self::TelegramAdminUserId => "123456789",
            Self::SlackAdminUserIds => "U12345678,U87654321",
            Self::ExternalChannelApiToken => "Click dice to generate a secure token",
            Self::ExternalChannelSafeMode => "",
        }
    }

    /// Get the available options for select inputs
    pub fn options(&self) -> Option<Vec<(&'static str, &'static str)>> {
        match self {
            Self::TwitterReplyChance => Some(vec![
                ("100", "100% (reply to all)"),
                ("50", "50%"),
                ("25", "25%"),
                ("10", "10%"),
                ("5", "5%"),
                ("1", "1%"),
            ]),
            _ => None,
        }
    }

    /// Get the default value for this setting
    pub fn default_value(&self) -> &'static str {
        match self {
            Self::AutoStartOnBoot => "false",
            Self::DiscordBotToken => "",
            Self::DiscordAdminUserIds => "",
            Self::TelegramBotToken => "",
            Self::SlackBotToken => "",
            Self::SlackAppToken => "",
            Self::TwitterBotHandle => "",
            Self::TwitterBotUserId => "",
            Self::TwitterPollIntervalSecs => "120",
            Self::TwitterReplyChance => "100",
            Self::TwitterMaxMentionsPerHour => "0",
            Self::TwitterAdminXAccount => "",
            Self::TelegramAdminUserId => "",
            Self::SlackAdminUserIds => "",
            Self::ExternalChannelApiToken => "",
            Self::ExternalChannelSafeMode => "false",
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
            ChannelSettingKey::DiscordBotToken.into(),
            ChannelSettingKey::DiscordAdminUserIds.into(),
        ],
        ChannelType::Telegram => vec![
            ChannelSettingKey::TelegramBotToken.into(),
            ChannelSettingKey::TelegramAdminUserId.into(),
        ],
        ChannelType::Slack => vec![
            ChannelSettingKey::SlackBotToken.into(),
            ChannelSettingKey::SlackAppToken.into(),
            ChannelSettingKey::SlackAdminUserIds.into(),
        ],
        ChannelType::Twitter => vec![
            ChannelSettingKey::TwitterBotHandle.into(),
            ChannelSettingKey::TwitterBotUserId.into(),
            ChannelSettingKey::TwitterPollIntervalSecs.into(),
            ChannelSettingKey::TwitterReplyChance.into(),
            ChannelSettingKey::TwitterMaxMentionsPerHour.into(),
            ChannelSettingKey::TwitterAdminXAccount.into(),
        ],
        ChannelType::ExternalChannel => vec![
            ChannelSettingKey::ExternalChannelApiToken.into(),
            ChannelSettingKey::ExternalChannelSafeMode.into(),
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
        // 1 common + 2 Discord-specific (bot_token, admin_user_ids)
        assert_eq!(settings.len(), 3);
        assert_eq!(settings[0].key, "auto_start_on_boot");
        assert_eq!(settings[1].key, "discord_bot_token");
        assert_eq!(settings[2].key, "discord_admin_user_ids");
    }

    #[test]
    fn test_telegram_settings() {
        let settings = get_settings_for_channel_type(ChannelType::Telegram);
        // 1 common + 2 Telegram-specific (bot_token, admin_user_id)
        assert_eq!(settings.len(), 3);
        assert_eq!(settings[0].key, "auto_start_on_boot");
        assert_eq!(settings[1].key, "telegram_bot_token");
        assert_eq!(settings[2].key, "telegram_admin_user_id");
    }

    #[test]
    fn test_slack_settings() {
        let settings = get_settings_for_channel_type(ChannelType::Slack);
        // 1 common + 3 Slack-specific (bot_token, app_token, admin_user_ids)
        assert_eq!(settings.len(), 4);
        assert_eq!(settings[0].key, "auto_start_on_boot");
        assert_eq!(settings[1].key, "slack_bot_token");
        assert_eq!(settings[2].key, "slack_app_token");
        assert_eq!(settings[3].key, "slack_admin_user_ids");
    }

    #[test]
    fn test_tool_verbosity_parsing() {
        assert_eq!(ToolOutputVerbosity::from_str_or_default("full"), ToolOutputVerbosity::Full);
        assert_eq!(ToolOutputVerbosity::from_str_or_default("minimal"), ToolOutputVerbosity::Minimal);
        assert_eq!(ToolOutputVerbosity::from_str_or_default("minimal_throttled"), ToolOutputVerbosity::MinimalThrottled);
        assert_eq!(ToolOutputVerbosity::from_str_or_default("none"), ToolOutputVerbosity::None);
        assert_eq!(ToolOutputVerbosity::from_str_or_default("invalid"), ToolOutputVerbosity::Full);
        // MinimalThrottled renders the same as Minimal
        assert_eq!(ToolOutputVerbosity::MinimalThrottled.display_verbosity(), ToolOutputVerbosity::Minimal);
        assert!(ToolOutputVerbosity::MinimalThrottled.is_throttled());
        assert!(!ToolOutputVerbosity::Minimal.is_throttled());
    }
}
