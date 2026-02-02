# StarkBot


![starkbot_intro1](https://github.com/user-attachments/assets/c4276f9d-46f2-4576-a691-ea822fe3aa00)



[![Deploy to DigitalOcean](https://www.deploytodo.com/do-btn-blue.svg)](https://cloud.digitalocean.com/apps/new?repo=https://github.com/ethereumdegen/stark-bot/tree/master)

A cloud-deployable agentic AI assistant built with Rust and Actix. StarkBot is an intelligent automation hub that interfaces with messaging platforms, executes blockchain transactions, and handles complex multi-step tasks autonomously.

**Key Features:**
- **Multi-platform messaging**: Discord, Slack, Telegram integration
- **Skills system**: Extensible markdown-based skills for code review, deployments, trading, and more
- **Web3 native**: Token swaps, wallet management, transaction execution on EVM chains
- **Memory & continuity**: Persistent memory system with daily logs and user preferences
- **Tool execution**: File operations, git workflows, web fetching, shell commands
- **SIWE authentication**: Sign In With Ethereum wallet authentication
- **Scheduling**: Cron-based task scheduling for automated workflows
- **x402 protocol**: Support for HTTP 402 micropayments
- **Easy deployment**: DigitalOcean, AWS, Docker support

<img width="1351" height="646" alt="Starkbot1" src="https://github.com/user-attachments/assets/4e66b1ce-59f7-405c-9353-67a8bead4868" />

## Use at your own risk 

AI Agents that directly interface with private keys always pose a serious risk. Use small amounts of cryptocurrency. Guard rails are built in to StarkBot but no guarantee is given that your funds will be safe. Significant testing has NOT been done with this tool - there is no assumption of safety or liability.





## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=ethereumdegen/stark-bot&type=date&legend=top-left)](https://www.star-history.com/#ethereumdegen/stark-bot&type=date&legend=top-left)




## Local Development

### Prerequisites

- Rust 1.88+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- SQLite3 (usually pre-installed on Linux)
- Node.js 18+ (for frontend development)

### Environment Setup

Both local development and Docker configurations read from a `.env` file. Set this up once and all commands will use it automatically.

Your `.env` file should contain:
```bash
# Required: Ethereum address that can log in via SIWE (Sign In With Ethereum)
# This is the wallet address you'll use to authenticate with MetaMask
LOGIN_ADMIN_PUBLIC_ADDRESS=0xYourEthereumWalletAddress

# Optional: Private key for bot to spend USDC on Base (for future features)
BURNER_WALLET_BOT_PRIVATE_KEY=

# Optional server configuration
PORT=8080
GATEWAY_PORT=8081
DATABASE_URL=./.db/stark.db
RUST_LOG=info
```

### Authentication

StarkBot uses **SIWE (Sign In With Ethereum)** for authentication:

1. Navigate to `http://localhost:8080`
2. Click "Connect Wallet"
3. Approve the connection in MetaMask
4. Sign the challenge message
5. You're logged in!

Only the wallet address specified in `LOGIN_ADMIN_PUBLIC_ADDRESS` can authenticate.

### Configure AI (After First Login)

API keys are managed through the web UI, not environment variables:

1. Start the server and login
2. Go to **API Keys** in the sidebar
3. Add your Anthropic API key (get one from [console.anthropic.com](https://console.anthropic.com/))
4. Your key is stored securely in the local SQLite database

### Run Locally

There are three ways to run StarkBot locally:

#### Option 1: Backend serves frontend (simplest)

```bash
# Build frontend first
cd stark-frontend && npm install && npm run build && cd ..

# Run the server (serves API + frontend on port 8080)
cargo run -p stark-backend
```

The server starts at `http://localhost:8080`

#### Option 2: Separate frontend dev server (for frontend development)

This gives you hot-reload for frontend changes:

```bash
# Terminal 1: Run backend only (API on 8080, WebSocket on 8081)
DISABLE_FRONTEND=1 cargo run -p stark-backend

# Terminal 2: Run frontend dev server (on 5173, proxies to backend)
cd stark-frontend && npm run dev
```

Open `http://localhost:5173` for hot-reloading frontend.

#### Option 3: Docker dev environment

```bash
docker compose -f docker-compose.dev.yml up --build
```

Open `http://localhost:8080`

### Development Modes Summary

| Environment | Command | API | WebSocket | Frontend |
|-------------|---------|-----|-----------|----------|
| **Local (combined)** | `cargo run -p stark-backend` | localhost:8080 | localhost:8081 | localhost:8080 |
| **Local (separate)** | `DISABLE_FRONTEND=1 cargo run` + `npm run dev` | localhost:8080 | localhost:8081 | localhost:5173 |
| **Docker dev** | `docker compose -f docker-compose.dev.yml up` | internal:8082 | localhost:8081 | localhost:8080 |
| **Docker prod** | `docker compose up` | localhost:8080 | localhost:8081 | localhost:8080 |


### Test Endpoints

```bash
# Health check
curl http://localhost:8080/health

# Generate SIWE challenge (step 1 of login)
curl -X POST http://localhost:8080/api/auth/generate_challenge \
  -H "Content-Type: application/json" \
  -d '{"public_address":"0xYourWalletAddress"}'

# Validate auth with signature (step 2 of login)
# Note: In practice, the signature is generated by your wallet
curl -X POST http://localhost:8080/api/auth/validate_auth \
  -H "Content-Type: application/json" \
  -d '{"public_address":"0x...","challenge":"Signing in to StarkBot as 0x... at 1234567890","signature":"0x..."}'
```

## Local Docker Testing

The production Docker setup reads configuration from your `.env` file automatically (no need to pass `-e` flags).

### Run with Docker Compose (Recommended)

```bash
# Start the container (reads from .env automatically)
docker compose up --build

# Or run in background
docker compose up --build -d

# View logs
docker compose logs -f

# Stop
docker compose down
```

This includes persistent database storage in the `./data` directory.

### Run with Docker Manually

If you prefer manual Docker commands:

```bash
# Build the image
docker build -t starkbot .

# Run with env file
docker run -p 8080:8080 --env-file .env -v $(pwd)/data:/app/.db starkbot

# Or run in detached mode
docker run -d -p 8080:8080 --env-file .env -v $(pwd)/data:/app/.db --name starkbot starkbot

# Stop and remove
docker stop starkbot && docker rm starkbot
```

### Test the Container

```bash
# Health check
curl http://localhost:8080/health

# Open in browser
xdg-open http://localhost:8080  # Linux
open http://localhost:8080      # macOS
```

## Development with Hot Reload (Docker)

For active development, use the dev Docker configuration which provides automatic hot reloading for both frontend and backend changes.

### Start Development Environment

```bash
# Start with hot reload (first run will take longer to build)
docker compose -f docker-compose.dev.yml up --build

# Or run in background
docker compose -f docker-compose.dev.yml up --build -d

# View logs when running in background
docker compose -f docker-compose.dev.yml logs -f
```





### Stop Development Environment

```bash
# Stop containers
docker compose -f docker-compose.dev.yml down

# Stop and remove volumes (clean slate)
docker compose -f docker-compose.dev.yml down -v
```

## Deploy to DigitalOcean App Platform

### 1. Push to GitHub

```bash
git init
git add .
git commit -m "Initial commit"
git remote add origin git@github.com:yourusername/starkbot.git
git push -u origin main
```

### 2. Create App on DigitalOcean

1. Go to [DigitalOcean App Platform](https://cloud.digitalocean.com/apps)
2. Click **Create App**
3. Select **GitHub** and authorize access
4. Choose your `starkbot` repository
5. Select the branch (e.g., `main`)

### 3. Configure the App

DigitalOcean should auto-detect the Dockerfile. If not, manually configure:

- **Type**: Web Service
- **Source**: Dockerfile
- **HTTP Port**: 8080
- **Health Check Path**: `/health`

### 4. Set Environment Variables

In the App settings, add:

| Variable | Value |
|----------|-------|
| `LOGIN_ADMIN_PUBLIC_ADDRESS` | Your Ethereum wallet address (e.g., `0x1234...`) |
| `RUST_LOG` | `info` |
| `DATABASE_URL` | `/app/.db/stark.db` |

### 5. Configure Persistent Storage (Optional)

For persistent SQLite data across deploys:

1. Go to **Components** > your web service > **Settings**
2. Under **Volumes**, click **Add Volume**
3. Set mount path to `/app/.db`
4. Update `DATABASE_URL` to `/app/.db/stark.db`

### 6. Deploy

Click **Create Resources** to deploy. The build takes a few minutes.

Your app will be available at: `https://your-app-name.ondigitalocean.app`

## App Spec (Alternative)

You can also deploy using a `.do/app.yaml` spec file:

```yaml
name: starkbot
services:
  - name: web
    dockerfile_path: Dockerfile
    github:
      repo: yourusername/starkbot
      branch: main
      deploy_on_push: true
    http_port: 8080
    health_check:
      http_path: /health
    instance_size_slug: basic-xxs
    instance_count: 1
    envs:
      - key: LOGIN_ADMIN_PUBLIC_ADDRESS
        scope: RUN_TIME
        value: "0xYourEthereumWalletAddress"
      - key: RUST_LOG
        scope: RUN_TIME
        value: info
      - key: DATABASE_URL
        scope: RUN_TIME
        value: /app/.db/stark.db
```

Deploy with:
```bash
doctl apps create --spec .do/app.yaml
```

## Project Structure

```
starkbot-monorepo/
├── Cargo.toml                 # Workspace manifest
├── Dockerfile                 # Production multi-stage build
├── Dockerfile.dev             # Development build with hot reload
├── docker-compose.yml         # Production Docker Compose
├── docker-compose.dev.yml     # Dev environment with volume mounts
├── SOUL.md                    # Agent identity and personality config
├── skills/                    # Markdown-based skill definitions
│   ├── bankr.md               # Bankr token trading
│   ├── polymarket.md          # Polymarket predictions
│   ├── swap.md                # DEX token swaps
│   ├── github.md              # GitHub integration
│   ├── discord.md             # Discord operations
│   └── ...                    # 20+ skills available
├── stark-backend/             # Actix web server (Rust)
│   └── src/
│       ├── main.rs            # Server entry point
│       ├── config.rs          # Environment config
│       ├── ai/                # AI agent logic
│       ├── channels/          # Discord, Slack, Telegram integrations
│       ├── controllers/       # API endpoints
│       ├── db/                # SQLite database
│       ├── execution/         # Tool execution engine
│       ├── gateway/           # WebSocket gateway
│       ├── memory/            # Agent memory system
│       ├── scheduler/         # Cron scheduling
│       ├── skills/            # Skill loading and parsing
│       └── tools/builtin/     # 35+ built-in tools
└── stark-frontend/            # React/TypeScript frontend
    └── src/
        ├── components/        # Reusable UI components
        ├── pages/             # Application pages
        └── views/             # Chat and dashboard views
```

## Skills System

StarkBot uses a powerful markdown-based skills system. Skills define capabilities and workflows the agent can execute.

### Built-in Skills

| Skill | Description |
|-------|-------------|
| `bankr` | Trade tokens on Bankr (Base network) |
| `polymarket` | Interact with Polymarket prediction markets |
| `swap` | Execute token swaps on DEXs |
| `transfer` | Send tokens and ETH |
| `github` | GitHub repository operations |
| `discord` | Discord channel management |
| `moltbook` | Moltbook integrations |
| `moltx` | MoltX trading |
| `code-review` | Automated code review |
| `commit` | Git commit workflows |
| `deploy-github` | Deploy to GitHub Pages |
| `create-project` | Scaffold new projects |
| `create-skill` | Create new skills |
| `scheduling` | Set up cron-based tasks |
| `weather` | Weather lookups |
| `token_price` | Cryptocurrency price checks |
| `local_wallet` | Local wallet management |
| `weth` | WETH wrap/unwrap operations |

### Installing Custom Skills

Skills can be installed through the web UI:
1. Navigate to **Skills** in the sidebar
2. Upload a `.md` file or `.zip` archive
3. Skills are immediately available to the agent

Skill format follows the Claude Code / Clawd skill specification.

## Built-in Tools

The agent has access to 35+ built-in tools:

**File Operations**: `read_file`, `write_file`, `edit_file`, `delete_file`, `rename_file`, `glob`, `grep`, `list_files`

**Git & Code**: `git`, `committer`, `pr_quality`, `apply_patch`

**Memory**: `memory_store`, `memory_get`, `multi_memory_search`

**Web3**: `web3_tx`, `web3_function_call`, `token_lookup`

**Communication**: `say_to_user`, `ask_user`, `agent_send`, `discord_lookup`

**System**: `exec`, `process_status`, `task_complete`, `subagent`

**Web**: `web_fetch`, `x402_fetch`, `x402_rpc`

## AI Provider Configuration

StarkBot works with OpenAI-compatible APIs. API keys are managed through the web UI under **API Keys**.

### Using Anthropic Claude

Get an API key from [console.anthropic.com](https://console.anthropic.com/) and add it in the API Keys settings.

### Using DigitalOcean AI Agents

Compatible with DigitalOcean's hosted models (e.g., llama-33-instruct):

```
Provider Type    : OpenAI Compatible
API Endpoint URI : https://xxxxxxxxx.agents.do-ai.run/api/v1/chat/completions
API Key          : your-access-key
```

## Messaging Integrations

StarkBot can connect to multiple messaging platforms simultaneously:

| Platform | Status | Configuration |
|----------|--------|---------------|
| Discord | ✅ Supported | Bot token in API Keys |
| Slack | ✅ Supported | App credentials in API Keys |
| Telegram | ✅ Supported | Bot token in API Keys |
| Web Chat | ✅ Built-in | Available at dashboard |

## Memory & Continuity

StarkBot maintains persistent memory across sessions:

- **Long-term memory**: Facts about users, preferences, important context
- **Daily logs**: Session notes, decisions, follow-ups
- **Session history**: Recent conversation context

Memory is automatically stored when the agent uses markers like `[REMEMBER: fact]` or `[DAILY_LOG: note]` in responses.

## Agent Identity (SOUL.md)

The `SOUL.md` file defines StarkBot's personality and behavior guidelines. Key principles:

- **Action over words**: Solve problems, don't narrate them
- **Genuine assistance**: Skip corporate phrases, just help
- **Have opinions**: Disagree when something is a bad idea
- **Respect the access**: Handle API keys and user data with care

You can customize `SOUL.md` to adjust the agent's personality for your use case.
