<p align="center">
  <img src="https://github.com/user-attachments/assets/c4276f9d-46f2-4576-a691-ea822fe3aa00" alt="StarkBot" width="400" />
</p>

<h1 align="center">StarkBot</h1>

<p align="center">
  <strong>An autonomous AI agent with a crypto wallet, on-chain identity, and a soul.</strong><br/>
  Thinks, trades, deploys, and pays for its own services — across every chain and every chat platform.<br/>
  Your agent. Your keys. Your infrastructure.
</p>

<p align="center">
  <a href="https://github.com/ethereumdegen/stark-bot">
    <img src="https://img.shields.io/static/v1?label=Core&message=Rust&color=DEA584" />
  </a>
  <a href="https://github.com/ethereumdegen/stark-bot">
    <img src="https://img.shields.io/github/stars/ethereumdegen/stark-bot?style=flat&color=yellow" />
  </a>
  <a href="https://github.com/ethereumdegen/stark-bot">
    <img src="https://img.shields.io/static/v1?label=Frontend&message=React+%2B+TypeScript&color=61DAFB" />
  </a>
</p>

<p align="center">
  <a href="#the-problem">The Problem</a> •
  <a href="#capabilities">Capabilities</a> •
  <a href="#how-it-works">How It Works</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#tech-stack">Tech Stack</a> •
  <a href="#skills-system">Skills</a>
</p>

> **One-click deploy:** [![Deploy to DigitalOcean](https://www.deploytodo.com/do-btn-blue.svg)](https://cloud.digitalocean.com/apps/new?repo=https://github.com/ethereumdegen/stark-bot/tree/master) — or run locally, Docker, Railway, AWS. Connect your wallet, add an API key, go.

<p align="center">
  <img width="1351" height="646" alt="StarkBot Dashboard" src="https://github.com/user-attachments/assets/4e66b1ce-59f7-405c-9353-67a8bead4868" />
</p>

---

## The Problem

Most AI agents can talk. Few can *do* anything. They can't hold a wallet. They can't sign a transaction. They can't pay for an API call with real money. They can't remember what you told them yesterday. They live inside a single chat window, disconnected from the systems that actually matter.

The agents that *can* interact with crypto are usually toy wrappers around an LLM with a private key bolted on — no memory, no identity, no safety rails, no real tool system. They can't execute a multi-step DeFi strategy, manage a Gnosis Safe, or generate an image and pay for it autonomously.

StarkBot is the full stack. An autonomous agent built in Rust with a real wallet, persistent graph memory, 60+ extensible skills, multi-platform presence, on-chain identity, and the ability to pay for services using the x402 micropayment protocol — without asking you for permission every time.

---

## Built for Autonomy

StarkBot isn't a chatbot with a wallet taped on. It's an autonomous system that converges AI reasoning with blockchain execution. It has its own wallet, its own identity, its own memory, and its own judgment about when to act.

**For DeFi operators** — StarkBot executes swaps via 0x, lends on Aave V3, trades prediction markets on Polymarket, manages yield on Pendle, and handles multi-sig operations through Gnosis Safe. It understands allowances, health factors, and slippage. It doesn't just submit transactions — it reasons about whether it should.

**For teams and communities** — drop StarkBot into Discord, Slack, Telegram, or Twitter. It normalizes conversations across platforms into a unified context, maintains per-user memory, and operates with configurable safety modes so you control what it can do in each channel.

**For builders** — 60+ markdown-based skills mean you extend StarkBot without touching Rust. Write a `.md` file that describes a workflow, drop it in, and the agent picks it up immediately. Skills can bundle ABIs, scripts, and tool presets. Install custom skills through the web UI or create your own.

**For sovereign agents** — EIP-8004 on-chain identity gives StarkBot a verifiable presence on-chain. It mints its own identity NFT, builds reputation through a public registry, and can be discovered and validated by other agents. This isn't a gimmick — it's the foundation for an agent economy.

### Deploy Your Way

| Method | What You Get |
|--------|-------------|
| **[DigitalOcean](https://cloud.digitalocean.com/apps/new?repo=https://github.com/ethereumdegen/stark-bot/tree/master)** | One-click deploy. Set env vars, done. |
| **Docker** | `docker compose up` — production-ready with persistent storage. |
| **Docker Dev** | `docker compose -f docker-compose.dev.yml up` — hot reload for both frontend and backend. |
| **Local** | `cargo run -p stark-backend` — API + frontend on port 8080. |
| **Railway** | Push to GitHub Container Registry, deploy from image. |

---

## Capabilities

### Web3 Execution

StarkBot has a real Ethereum wallet and knows how to use it:

- **Token swaps** — DEX trading via 0x aggregator with automatic allowance management
- **Lending & borrowing** — Aave V3 across Ethereum, Base, Arbitrum, Optimism, Polygon, and Avalanche
- **Prediction markets** — Polymarket trading with order book and position tracking (Ed25519 API auth)
- **Yield trading** — Pendle Finance with Principal Tokens, Yield Tokens, and LP strategies
- **Multi-sig management** — Deploy 1-of-N Gnosis Safes, propose/sign/execute transactions, manage signers
- **Token transfers** — ETH and ERC-20 transfers on Base and Ethereum
- **WETH operations** — Wrap and unwrap with a single command
- **USDC bridging** — Cross-network USDC transfers
- **On-chain data** — Alchemy Enhanced APIs, DexScreener charts, GeckoTerminal analytics

Two wallet modes, same interface:

| Mode | Provider | Keys |
|------|----------|------|
| **Standard** | `EnvWalletProvider` | Private key in `.env` — you hold the key |
| **Flash** | `FlashWalletProvider` | Privy-custodied embedded wallet — keys never touch the host |

### x402 Autonomous Payments

StarkBot pays for its own services. The [x402 protocol](https://www.x402.org/) lets any HTTP endpoint return `402 Payment Required` with a price, and StarkBot autonomously signs a permit and pays — no human in the loop.

- **AI image generation** — Flux Schnell, Kling v3, Kling O3 via SuperRouter
- **AI video generation** — MiniMax Hailuo, Kling v3 Standard/Pro
- **DeFi market data** — PayToll API for on-chain analytics
- **Custom endpoints** — any x402-compatible service via the `x402_post` tool
- **Spending limits** — per-token configurable limits in the database

Payment token: **STARKBOT** (ERC-20 on Base) — `0x587Cd533F418825521f3A1daa7CCd1E7339A1B07`

### Messaging

Native adapters for five platforms with a unified message dispatcher:

| Platform | Features |
|----------|----------|
| **Discord** | Full bot API — server discovery, message management, moderation, emoji/sticker uploads, reactions |
| **Slack** | Slack Morphism SDK — workspace-aware messaging with event handling |
| **Telegram** | Teloxide — direct DMs, group support, media attachments |
| **Twitter/X** | OAuth 1.0a — post, reply, quote tweet |
| **Web Chat** | Built-in dashboard — real-time streaming via WebSocket |

Per-channel configuration: safety modes (Safe / Standard / Dangerous), rate limits, tool restrictions, payment mode (free / x402 / metered). The agent can be simultaneously present on all platforms with a single deployment.

### Memory

Not a vector database with raw dumps. StarkBot's memory is a three-tier system that combines keyword precision, semantic flexibility, and graph-based context:

**Tier 1: Typed Memory Store** — Seven categories (daily logs, long-term memory, preferences, facts, entities, tasks, observations) with importance scoring, expiration tracking, and per-user isolation.

**Tier 2: Vector Embeddings** — 384-dimensional embeddings for semantic similarity search. Find related memories even when the words don't match.

**Tier 3: Graph Associations** — Typed relationships (RelatedTo, Updates, Contradicts, CausedBy, ResultOf, PartOf) connecting memories into a navigable knowledge graph.

**Retrieval**: Reciprocal Rank Fusion merging all three tiers. The agent doesn't dump search results — it synthesizes relevant context from structured knowledge.

Memory markers for automatic storage:
- `[REMEMBER: fact]` → long-term memory
- `[DAILY_LOG: note]` → session logging
- Graph associations auto-created for connected memories

### Multi-Agent Orchestration

StarkBot runs a hierarchical agent system:

- **Director agent** — routes tasks, spawns specialized subagents
- **Subagent roles** — finance, code engineering, secretary — each with constrained tool sets
- **Isolated execution** — subagents get their own session context, preventing cross-contamination
- **Parallel execution** — multiple subagents work simultaneously with result synthesis
- **Session lane manager** — prevents race conditions across concurrent sessions

### Scheduling & Automation

- **Cron jobs** — flexible cron expressions with max 5 concurrent executions
- **Heartbeat system** — periodic self-reflection cycles (configurable intervals, active hours, day-of-week)
- **Impulse maps** — knowledge graph nodes the agent traverses and refines autonomously
- **Error resilience** — exponential backoff (30s → 1m → 5m → 15m → 60m), 10-minute timeout per job
- **Task system** — `define_tasks` for breaking complex operations into sequential steps

### Tools

35+ built-in tools across six categories:

| Category | Tools |
|----------|-------|
| **File ops** | `read_file`, `write_file`, `edit_file`, `delete_file`, `rename_file`, `glob`, `grep`, `list_files` |
| **Git & code** | `git`, `committer`, `pr_quality`, `apply_patch` |
| **Memory** | `memory_store`, `memory_get`, `multi_memory_search`, `memory_graph`, `memory_associate`, `memory_merge` |
| **Web3** | `web3_tx`, `web3_function_call`, `token_lookup`, `send_eth`, `swap_execute`, `erc20_approve_swap`, `x402_post`, `x402_rpc` |
| **Communication** | `say_to_user`, `ask_user`, `agent_send`, `discord_read`, `discord_write`, `twitter_post` |
| **System** | `exec`, `process_status`, `web_fetch`, `subagent`, `notes`, `define_tasks` |

### Dashboard

A full React + TypeScript frontend with 30+ pages:

- **Agent Chat** — conversational interface with real-time streaming
- **Skills Browser** — enable/disable skills, upload custom `.md` or `.zip` skills, relationship graph visualization
- **Memory Browser** — browse typed memories, explore the knowledge graph, visualize associations
- **Crypto Transactions** — transaction history, payment logs, spending tracking
- **Scheduling** — cron job management, heartbeat configuration
- **Channels** — Discord/Slack/Telegram channel configuration and safety modes
- **API Keys** — manage Anthropic, GitHub, Twitter, Polymarket, and other service credentials
- **Cloud Backup** — ECIES-encrypted backup and restore of agent state
- **Identity** — EIP-8004 on-chain identity registration and management
- **Kanban Board** — task tracking with column state
- **Impulse Map** — knowledge graph visualization with D3.js
- **System** — logs, file browser, configuration editor

---

## How It Works

### The Agent Loop

```
User sends message (Discord / Slack / Telegram / Twitter / Web)
    → Message dispatcher normalizes it
        → AI engine processes with tool-calling loop
            → Agent reasons, selects tools, executes
            → Tools interact with blockchain, files, APIs, memory
            → x402 payments happen autonomously when needed
        → Response streamed back via WebSocket
    → Response delivered to originating platform
```

### The Wallet

StarkBot's wallet isn't an afterthought. It's woven into the execution layer:

- **SIWE authentication** — the agent proves its identity with Ethereum signatures
- **EIP-2612 permits** — gasless token approvals for x402 payments
- **Typed data signing** — full EIP-712 support for DeFi protocol interactions
- **ECIES encryption** — encrypt agent state for secure cloud backup
- **Multi-sig** — deploy and manage Gnosis Safe wallets with threshold signing

### The Skills System

Skills are markdown files. No Rust compilation required.

```
skills/
├── swap.md              # DEX token swaps
├── aave.md              # Lending & borrowing
├── polymarket_us.md     # Prediction markets
├── safe_wallet.md       # Gnosis Safe management
├── github.md            # PR workflows & code quality
├── discord.md           # Discord operations
├── image_generation.md  # AI image gen (x402-paid)
├── video_generation.md  # AI video gen (x402-paid)
├── notes.md             # Persistent note-taking
├── heartbeat.md         # Self-reflection automation
├── scheduling.md        # Cron job setup
└── ...                  # 60+ skills total
```

Each skill can bundle:
- **Tool definitions** — custom tools with parameters and execution logic
- **ABI files** — smart contract interfaces for on-chain interactions
- **Scripts** — Python/shell scripts for complex operations
- **Presets** — pre-configured tool parameters for common operations

Install custom skills through the web UI or drop `.md` files into the skills directory.

### The Memory Graph

```
[Fact: "User prefers Base network"]
    ──RelatedTo──→ [Preference: "Low gas fees"]
    ──Updates───→ [Fact: "User previously used Ethereum mainnet"]

[Decision: "Swapped 100 USDC for ETH"]
    ──CausedBy──→ [Observation: "ETH price dropped 5%"]
    ──PartOf────→ [Task: "Rebalance portfolio"]
```

Memories decay over time. Identity memories are exempt. The graph is traversable, searchable, and automatically pruned.

---

## Quick Start

### Prerequisites

- **Rust** 1.88+ ([rustup.rs](https://rustup.rs/))
- **Node.js** 18+ (for frontend)
- **SQLite3** (usually pre-installed on Linux)
- **[uv](https://docs.astral.sh/uv/)** (for Python-based skill scripts)

### Build and Run

```bash
git clone https://github.com/ethereumdegen/stark-bot
cd stark-bot

# Build frontend
cd stark-frontend && npm install && npm run build && cd ..

# Run (API + frontend on port 8080)
cargo run -p stark-backend
```

### Configure

Create a `.env` file:

```bash
# Required: Ethereum address for SIWE login
LOGIN_ADMIN_PUBLIC_ADDRESS=0xYourWalletAddress

# Optional: Private key for wallet operations
BURNER_WALLET_BOT_PRIVATE_KEY=

# Server
PORT=8080
GATEWAY_PORT=8081
DATABASE_URL=./.db/stark.db
RUST_LOG=info
```

### First Login

1. Open `http://localhost:8080`
2. Click **Connect Wallet** → approve in MetaMask → sign the challenge
3. Go to **API Keys** → add your Anthropic API key
4. Start chatting

API keys are stored in the local SQLite database, not in environment variables. No secrets in `.env` beyond the wallet key.

### Development Modes

| Environment | Command | API | WebSocket | Frontend |
|-------------|---------|-----|-----------|----------|
| **Local (combined)** | `cargo run -p stark-backend` | :8080 | :8081 | :8080 |
| **Local (separate)** | `DISABLE_FRONTEND=1 cargo run` + `npm run dev` | :8080 | :8081 | :5173 |
| **Docker dev** | `docker compose -f docker-compose.dev.yml up` | internal | :8081 | :8080 |
| **Docker prod** | `docker compose up` | :8080 | :8081 | :8080 |

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Language | **Rust** (edition 2021) — ~60,000 lines of production backend code |
| Web framework | **Actix-web 4** with WebSocket support (Actix-WS) |
| Async runtime | **Tokio** (full features) |
| Database | **SQLite 3** with WAL mode, R2D2 connection pooling, Moka in-memory cache |
| Wallet | **Ethers.rs 2.0** — dual wallet provider, SIWE, EIP-2612, EIP-712, ECIES |
| Frontend | **React 18** + TypeScript 5 + Vite 5 + Tailwind CSS 3 |
| Visualization | **D3.js 7** (knowledge graphs) + **Anime.js 4** (animations) |
| Discord | **Serenity** — gateway, cache, events, rich messages |
| Slack | **slack-morphism** — Socket Mode, events, workspace messaging |
| Telegram | **teloxide** — long-poll, media, group/DM support |
| Crypto (frontend) | **ethers.js 6** + **eth-crypto 3** |
| Deployment | **Docker** multi-stage build, DigitalOcean, Railway, AWS |

No microservices. No Kubernetes. Everything runs in a single binary with an embedded database.

---

## Security

### Use at Your Own Risk

AI agents that interface with private keys pose inherent risk. Use small amounts of cryptocurrency. Guard rails are built into StarkBot but no guarantee is given that funds will be safe. Significant testing has NOT been done with this tool — there is no assumption of safety or liability.

### Built-in Protections

- **SIWE authentication** — only your wallet address can access the dashboard
- **Per-channel safety modes** — restrict dangerous tools per channel
- **Session token validation** — on all protected API endpoints
- **Secret detection** — in git commit workflows
- **ECIES encryption** — for cloud backup of agent state
- **Rate limiting** — per-user and per-channel
- **No API keys in env** — credentials stored encrypted in SQLite

---

## Agent Identity (SOUL.md)

`SOUL.md` defines who your agent is. Key principles:

- **Action over words** — solve problems, don't narrate them
- **Genuine assistance** — skip corporate phrases, just help
- **Have opinions** — disagree when something is a bad idea
- **Respect the access** — handle keys and user data with care

Customize `SOUL.md` to adjust personality, behavior, and guardrails for your use case.

---

## Project Structure

```
stark-bot/
├── Cargo.toml                 # Workspace manifest
├── Dockerfile                 # Production multi-stage build
├── docker-compose.yml         # Production Docker Compose
├── docker-compose.dev.yml     # Dev environment with hot reload
├── SOUL.md                    # Agent personality config
├── skills/                    # 60+ markdown-based skill definitions
│   ├── swap.md                # DEX trading
│   ├── aave.md                # Lending/borrowing
│   ├── polymarket_us.md       # Prediction markets
│   ├── safe_wallet.md         # Gnosis Safe multi-sig
│   ├── image_generation.md    # AI image generation
│   └── ...
├── stark-backend/             # Actix web server (Rust)
│   └── src/
│       ├── main.rs            # Server entry point
│       ├── ai/                # AI agent logic & orchestration
│       ├── channels/          # Discord, Slack, Telegram, Twitter
│       ├── controllers/       # API endpoints (30+)
│       ├── db/                # SQLite schema (50+ tables)
│       ├── execution/         # Tool execution engine
│       ├── gateway/           # WebSocket gateway
│       ├── memory/            # Three-tier memory system
│       ├── scheduler/         # Cron & heartbeat scheduling
│       ├── skills/            # Skill loading & registry
│       ├── wallet/            # Dual wallet provider
│       ├── x402/              # x402 payment protocol
│       └── tools/builtin/     # 35+ built-in tools
└── stark-frontend/            # React + TypeScript dashboard
    └── src/
        ├── components/        # Reusable UI components
        ├── pages/             # 30+ application pages
        └── views/             # Chat, dashboard, graph views
```

---

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=ethereumdegen/stark-bot&type=date&legend=top-left)](https://www.star-history.com/#ethereumdegen/stark-bot&type=date&legend=top-left)

---

## Contributing

1. Fork the repo
2. Create a feature branch
3. Make your changes
4. Submit a PR

---

## License

MIT — see [LICENSE](LICENSE) for details.
