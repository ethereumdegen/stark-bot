// Chat demo configuration
// Each row represents an action in the simulated chat

export type ChatRowType = 'typing' | 'user' | 'tool_call' | 'tool_result' | 'assistant';

export interface ChatRow {
  type: ChatRowType;
  delay: number; // ms before this action starts
  content?: string;
  toolName?: string;
  params?: Record<string, unknown>;
  success?: boolean;
}

export interface ChatSequence {
  id: string;
  name: string;
  rows: ChatRow[];
}

// All available chat sequences
export const chatSequences: ChatSequence[] = [
  {
    id: 'dad-joke',
    name: 'Dad Joke',
    rows: [
      {
        type: 'typing',
        delay: 1000,
        content: 'tell me a joke. https://dad-jokes-agent-production.up.railway.app'
      },
      {
        type: 'user',
        delay: 500,
        content: 'tell me a joke. https://dad-jokes-agent-production.up.railway.app'
      },
      {
        type: 'tool_call',
        delay: 800,
        toolName: 'http_request',
        params: {
          method: 'GET',
          url: 'https://dad-jokes-agent-production.up.railway.app/joke'
        }
      },
      {
        type: 'tool_result',
        delay: 1200,
        toolName: 'http_request',
        success: true,
        content: JSON.stringify({
          setup: "Why do programmers prefer dark mode?",
          punchline: "Because light attracts bugs!"
        }, null, 2)
      },
      {
        type: 'assistant',
        delay: 600,
        content: "Here's a dad joke for you:\n\n**Why do programmers prefer dark mode?**\n\nBecause light attracts bugs!"
      }
    ]
  },
  {
    id: 'starkbot-price-discord',
    name: 'StarkBot Price to Discord',
    rows: [
      {
        type: 'typing',
        delay: 1000,
        content: 'look up the starkbot price and post it to the discord'
      },
      {
        type: 'user',
        delay: 500,
        content: 'look up the starkbot price and post it to the discord'
      },
      {
        type: 'tool_call',
        delay: 800,
        toolName: 'token_price',
        params: {
          token: 'STARKBOT',
          chain: 'base'
        }
      },
      {
        type: 'tool_result',
        delay: 1400,
        toolName: 'token_price',
        success: true,
        content: JSON.stringify({
          token: 'STARKBOT',
          price_usd: 0.00042069,
          price_eth: 0.000000128,
          market_cap: 420690,
          volume_24h: 12500,
          change_24h: '+15.3%'
        }, null, 2)
      },
      {
        type: 'tool_call',
        delay: 600,
        toolName: 'discord_send',
        params: {
          channel: 'price-updates',
          message: 'STARKBOT Price Update: $0.00042069 (+15.3% 24h)'
        }
      },
      {
        type: 'tool_result',
        delay: 1000,
        toolName: 'discord_send',
        success: true,
        content: JSON.stringify({
          status: 'sent',
          channel: 'price-updates',
          message_id: '1234567890'
        }, null, 2)
      },
      {
        type: 'assistant',
        delay: 600,
        content: "Done! STARKBOT is currently trading at **$0.00042069** (up 15.3% in the last 24h). I've posted the update to the #price-updates channel on Discord."
      }
    ]
  },
  {
    id: 'weather-check',
    name: 'Weather Check',
    rows: [
      {
        type: 'typing',
        delay: 1000,
        content: "what's the weather in Tokyo?"
      },
      {
        type: 'user',
        delay: 500,
        content: "what's the weather in Tokyo?"
      },
      {
        type: 'tool_call',
        delay: 800,
        toolName: 'http_request',
        params: {
          method: 'GET',
          url: 'https://api.weather.gov/points/35.6762,139.6503'
        }
      },
      {
        type: 'tool_result',
        delay: 1200,
        toolName: 'http_request',
        success: true,
        content: JSON.stringify({
          location: 'Tokyo, Japan',
          temperature: '18°C',
          condition: 'Partly Cloudy',
          humidity: '65%'
        }, null, 2)
      },
      {
        type: 'assistant',
        delay: 600,
        content: "The weather in **Tokyo** right now:\n\n- Temperature: **18°C**\n- Condition: Partly Cloudy\n- Humidity: 65%"
      }
    ]
  },
  {
    id: 'swap-tokens',
    name: 'Token Swap',
    rows: [
      {
        type: 'typing',
        delay: 1000,
        content: 'swap 0.1 ETH for USDC'
      },
      {
        type: 'user',
        delay: 500,
        content: 'swap 0.1 ETH for USDC'
      },
      {
        type: 'tool_call',
        delay: 800,
        toolName: 'get_swap_quote',
        params: {
          from_token: 'ETH',
          to_token: 'USDC',
          amount: '0.1',
          chain: 'base'
        }
      },
      {
        type: 'tool_result',
        delay: 1400,
        toolName: 'get_swap_quote',
        success: true,
        content: JSON.stringify({
          from: '0.1 ETH',
          to: '324.50 USDC',
          rate: '1 ETH = 3245 USDC',
          gas_estimate: '$0.12',
          slippage: '0.5%'
        }, null, 2)
      },
      {
        type: 'tool_call',
        delay: 600,
        toolName: 'execute_swap',
        params: {
          from_token: 'ETH',
          to_token: 'USDC',
          amount: '0.1',
          min_received: '322.87'
        }
      },
      {
        type: 'tool_result',
        delay: 1800,
        toolName: 'execute_swap',
        success: true,
        content: JSON.stringify({
          status: 'confirmed',
          tx_hash: '0x8f2a...3b4c',
          received: '324.12 USDC'
        }, null, 2)
      },
      {
        type: 'assistant',
        delay: 600,
        content: "Swap complete! You received **324.12 USDC** for 0.1 ETH.\n\nTransaction: `0x8f2a...3b4c`"
      }
    ]
  },
  {
    id: 'moltbook-post',
    name: 'Moltbook Post',
    rows: [
      {
        type: 'typing',
        delay: 1000,
        content: 'post on moltbook about ai agents'
      },
      {
        type: 'user',
        delay: 500,
        content: 'post on moltbook about ai agents'
      },
      {
        type: 'tool_call',
        delay: 800,
        toolName: 'moltbook_post',
        params: {
          title: 'On AI Agent Coordination',
          content: 'The future of AI agents lies not in competition, but in collaboration. When agents share knowledge and coordinate actions, the whole ecosystem benefits.',
          community: 'ai-discussion'
        }
      },
      {
        type: 'tool_result',
        delay: 1200,
        toolName: 'moltbook_post',
        success: true,
        content: JSON.stringify({
          post_id: 'molt_12345',
          title: 'On AI Agent Coordination',
          community: 'ai-discussion',
          upvotes: 42,
          replies: 8
        }, null, 2)
      },
      {
        type: 'assistant',
        delay: 600,
        content: "Posted successfully to Moltbook! Your thoughts on AI agent coordination got **42 upvotes** and **8 replies** in the ai-discussion community."
      }
    ]
  },
  {
    id: 'token-balance-check',
    name: 'Token Balance Check',
    rows: [
      {
        type: 'typing',
        delay: 1000,
        content: 'check my starkbot balance'
      },
      {
        type: 'user',
        delay: 500,
        content: 'check my starkbot balance'
      },
      {
        type: 'tool_call',
        delay: 800,
        toolName: 'token_balance',
        params: {
          token: 'STARKBOT',
          chain: 'base',
          address: '0x742d...89ab'
        }
      },
      {
        type: 'tool_result',
        delay: 1200,
        toolName: 'token_balance',
        success: true,
        content: JSON.stringify({
          token: 'STARKBOT',
          balance: '1250000',
          balance_formatted: '1,250,000',
          value_usd: '525.86',
          price_per_token: '0.00042069'
        }, null, 2)
      },
      {
        type: 'assistant',
        delay: 600,
        content: "Your STARKBOT balance: **1,250,000 tokens** worth **$525.86** at current price of $0.00042069 per token."
      }
    ]
  }
];

// Get a random chat sequence
export function getRandomSequence(): ChatSequence {
  const index = Math.floor(Math.random() * chatSequences.length);
  return chatSequences[index];
}

// Loop delay - how long to wait before restarting the demo
export const LOOP_DELAY = 4000;

// Typing speed in ms per character
export const TYPING_SPEED = 40;
