use crate::ai::multi_agent::types::AgentSubtype;
use crate::gateway::protocol::GatewayEvent;
use crate::tools::registry::Tool;
use crate::tools::types::{
    PropertySchema, ToolContext, ToolDefinition, ToolGroup, ToolInputSchema, ToolResult,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Tool to switch between agent subtypes (Finance, CodeEngineer, Secretary)
/// This controls which tools and skills are available to the agent.
/// Think of subtypes as "toolboxes" - each one unlocks different capabilities.
///
/// IMPORTANT: This tool MUST be called FIRST before any other tools can be used.
/// The agent starts with no subtype selected and must choose based on the user's request.
pub struct SetAgentSubtypeTool {
    definition: ToolDefinition,
}

impl SetAgentSubtypeTool {
    pub fn new() -> Self {
        let mut properties = HashMap::new();
        properties.insert(
            "subtype".to_string(),
            PropertySchema {
                schema_type: "string".to_string(),
                description: "The agent subtype/toolbox to activate:\n\
                    • 'finance' - DeFi/crypto operations (swaps, transfers, web3)\n\
                    • 'code_engineer' - Software development (code editing, git, testing)\n\
                    • 'secretary' - Social media, messaging, scheduling, marketing".to_string(),
                default: None,
                items: None,
                enum_values: Some(vec![
                    "finance".to_string(),
                    "code_engineer".to_string(),
                    "secretary".to_string(),
                ]),
            },
        );

        SetAgentSubtypeTool {
            definition: ToolDefinition {
                name: "set_agent_subtype".to_string(),
                description: "⚡ REQUIRED FIRST TOOL: Select your toolbox before doing anything else!\n\n\
                    You MUST call this tool FIRST based on what the user wants:\n\
                    • 'finance' - For crypto/DeFi: swaps, transfers, balances, token lookups\n\
                    • 'code_engineer' - For coding: edit files, git, grep/glob, run commands\n\
                    • 'secretary' - For social: MoltX, messaging, scheduling, marketing\n\n\
                    Choose based on the user's request, then proceed with the appropriate tools.".to_string(),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties,
                    required: vec!["subtype".to_string()],
                },
                group: ToolGroup::System,
                hidden: false,
            },
        }
    }

    /// Get a description of available tools for a subtype
    fn describe_subtype(subtype: AgentSubtype) -> String {
        match subtype {
            AgentSubtype::None => {
                "❓ No toolbox selected. Call set_agent_subtype first!".to_string()
            }
            AgentSubtype::Finance => {
                "💰 Finance toolbox activated.\n\n\
                 ## Skills (use set_agent_subtype then pick a skill with use_skill)\n\
                 Most tasks are handled by a skill. Match the user's request to one of these:\n\n\
                 Trading & Swaps:\n\
                 • swap — Swap ERC20 tokens on Base via 0x DEX aggregator\n\
                 • polymarket_trading — Explore and trade on Polymarket prediction markets\n\
                 • bankr — AI-powered trading agent (advanced orders, NFTs, yield)\n\n\
                 Transfers & Bridging:\n\
                 • transfer — Send ERC20 tokens or native ETH to an address\n\
                 • bridge_usdc — Bridge USDC cross-chain (Base, Polygon, Ethereum, Arbitrum, Optimism)\n\
                 • discord_tipping — Tip ERC20 tokens to Discord users\n\
                 • broadcast_transactions — Broadcast queued transactions\n\n\
                 DeFi & Yield:\n\
                 • aave — Lend/borrow on Aave (supply, withdraw, check APY)\n\
                 • pendle — Fixed-income yield trading on Pendle (PT/YT)\n\
                 • weth — Wrap ETH→WETH or unwrap WETH→ETH\n\n\
                 Prices & Research:\n\
                 • token_price — Look up crypto/token prices and market data (CoinGecko)\n\
                 • dexscreener — DEX pair data, liquidity, and on-chain price charts\n\
                 • geckoterminal — On-chain charts, pool data, and trending tokens\n\n\
                 Wallet & Payments:\n\
                 • local_wallet — Check balances of the burner wallet across networks\n\
                 • x402_payment — Make x402 micropayments with USDC\n\n\
                 👉 Pick the matching skill and follow its instructions. Skills define the full \
                 workflow including which tools to call and in what order.\n\n\
                 ## Low-level tools (only when no skill fits)\n\
                 select_web3_network, web3_tx, web3_function_call, token_lookup, \
                 x402_rpc, x402_fetch, set_address, ask_user\n\n\
                  "
                    .to_string()
            }
            AgentSubtype::CodeEngineer => {
                "🛠️ CodeEngineer toolbox activated.\n\n\
                 ## Skills (use set_agent_subtype then pick a skill with use_skill)\n\
                 Most tasks are handled by a skill. Match the user's request to one of these:\n\n\
                 Development:\n\
                 • plan — Create a structured implementation plan for a task\n\
                 • debug — Analyze errors, trace through code, and suggest fixes\n\
                 • code-review — Review code changes for bugs, style, and security\n\
                 • create-project — Scaffold a new project from scratch\n\
                 • create-skill — Author a new Starkbot skill file\n\n\
                 Git & GitHub:\n\
                 • commit — Create a well-formatted git commit with proper messaging\n\
                 • github — PR creation, CI/CD monitoring, deployment operations\n\
                 • github_discussions — Interact with GitHub Discussions (GraphQL API)\n\
                 • full-dev-workflow — End-to-end dev workflow (branch, code, test, PR, deploy)\n\n\
                 Testing:\n\
                 • test — Run tests, detect framework, and analyze failures\n\n\
                 Deployment & Infrastructure:\n\
                 • vercel — Deploy and manage projects on Vercel\n\
                 • cloudflare_dns — Manage Cloudflare DNS records (all types, zones, search, bulk ops)\n\
                 • railway — Deploy and manage services on Railway\n\
                 • deploy-github — Deploy via GitHub Actions CI/CD\n\n\
                 👉 Pick the matching skill and follow its instructions.\n\n\
                 ## Low-level tools (only when no skill fits)\n\
                 grep, glob, edit_file, write_file, delete_file, rename_file, git, exec"
                    .to_string()
            }
            AgentSubtype::Secretary => {
                "📱 Secretary toolbox activated.\n\n\
                 ## Skills (use set_agent_subtype then pick a skill with use_skill)\n\
                 Most tasks are handled by a skill. Match the user's request to one of these:\n\
                 • moltx — Post, reply, like, follow, and build feeds on moltx.io (X for agents)\n\
                 • moltbook — Post, comment, vote, and browse communities on Moltbook\n\
                 • twitter — Post, reply, like, and follow on X/Twitter\n\
                 • discord — Send messages and interact on Discord\n\
                 • 4claw — Post and browse threads on 4claw imageboard for agents\n\
                 • x402book — Publish content with micropayments on x402book\n\
                 • journal — Write journal entries, notes, and documentation\n\
                 • scheduling — Create scheduled/recurring tasks (cron jobs, reminders)\n\n\
                 👉 Pick the matching skill and follow its instructions.\n\n\
                 ## Low-level tools (only when no skill fits)\n\
                 agent_send, memory_search, memory_read"
                    .to_string()
            }
        }
    }
}

impl Default for SetAgentSubtypeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct SetAgentSubtypeParams {
    subtype: String,
}

#[async_trait]
impl Tool for SetAgentSubtypeTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    async fn execute(&self, params: Value, context: &ToolContext) -> ToolResult {
        let params: SetAgentSubtypeParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
        };

        // Parse the subtype
        let subtype = match AgentSubtype::from_str(&params.subtype) {
            Some(s) => s,
            None => {
                return ToolResult::error(format!(
                    "Invalid subtype '{}'. Valid options: 'finance', 'code_engineer', 'secretary'",
                    params.subtype
                ))
            }
        };

        // Broadcast the subtype change event
        if let (Some(broadcaster), Some(channel_id)) = (&context.broadcaster, context.channel_id) {
            broadcaster.broadcast(GatewayEvent::agent_subtype_change(
                channel_id,
                subtype.as_str(),
                subtype.label(),
            ));
        }

        // Return success with description of available tools
        let description = Self::describe_subtype(subtype);
        ToolResult::success(description).with_metadata(json!({
            "subtype": subtype.as_str(),
            "label": subtype.label(),
            "emoji": subtype.emoji(),
            "allowed_tool_groups": subtype.allowed_tool_groups()
                .iter()
                .map(|g| g.as_str())
                .collect::<Vec<_>>(),
            "allowed_skill_tags": subtype.allowed_skill_tags(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_set_subtype_finance() {
        let tool = SetAgentSubtypeTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "subtype": "finance" }), &context)
            .await;

        assert!(result.success);
        assert!(result.content.contains("Finance toolbox"));
    }

    #[tokio::test]
    async fn test_set_subtype_code_engineer() {
        let tool = SetAgentSubtypeTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "subtype": "code_engineer" }), &context)
            .await;

        assert!(result.success);
        assert!(result.content.contains("CodeEngineer toolbox"));
    }

    #[tokio::test]
    async fn test_set_subtype_secretary() {
        let tool = SetAgentSubtypeTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "subtype": "secretary" }), &context)
            .await;

        assert!(result.success);
        assert!(result.content.contains("Secretary toolbox"));
    }

    #[tokio::test]
    async fn test_invalid_subtype() {
        let tool = SetAgentSubtypeTool::new();
        let context = ToolContext::new();

        let result = tool
            .execute(json!({ "subtype": "invalid" }), &context)
            .await;

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Invalid subtype"));
    }
}
