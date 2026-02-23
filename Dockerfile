# Frontend build stage
FROM node:20-slim AS frontend-builder

WORKDIR /app/stark-frontend

# Copy frontend package files
COPY stark-frontend/package*.json ./

# Install dependencies
RUN npm ci

# Copy frontend source
COPY stark-frontend/ ./

# Build frontend
RUN npm run build

# Backend build stage
FROM rust:1.88-slim-bookworm AS backend-builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy source code
COPY . .

# Build all workspace binaries
RUN cargo build --release -p stark-backend

# Runtime stage
FROM debian:bookworm-slim
ARG STARKBOT_VERSION=unknown
LABEL org.opencontainers.image.version="${STARKBOT_VERSION}"

WORKDIR /app

# Install runtime dependencies and tools for skills
# Note: Python is NOT installed here — skill scripts use `uv run` which manages its own Python.
RUN apt-get update && apt-get install -y \
    ca-certificates \
    sqlite3 \
    curl \
    git \
    jq \
    unzip \
    && rm -rf /var/lib/apt/lists/*

# Install uv (fast Python package manager for skills)
RUN curl -LsSf https://astral.sh/uv/install.sh | sh
ENV PATH="/root/.local/bin:$PATH"

# Install GitHub CLI (gh)
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg \
    && chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" | tee /etc/apt/sources.list.d/github-cli.list > /dev/null \
    && apt-get update \
    && apt-get install -y gh \
    && rm -rf /var/lib/apt/lists/*

# Install Railway CLI
RUN curl -fsSL "https://github.com/railwayapp/cli/releases/download/v4.29.0/railway-v4.29.0-x86_64-unknown-linux-gnu.tar.gz" \
    | tar xz -C /usr/local/bin/ \
    && chmod +x /usr/local/bin/railway

# Install Supabase CLI
RUN curl -fsSL "https://github.com/supabase/cli/releases/download/v2.75.0/supabase_linux_amd64.tar.gz" \
    | tar xz -C /usr/local/bin/ supabase \
    && chmod +x /usr/local/bin/supabase

# Install gog CLI (Google Workspace — Gmail, Calendar, Drive, Sheets, Docs)
RUN curl -fsSL "https://github.com/steipete/gogcli/releases/download/v0.11.0/gogcli_0.11.0_linux_amd64.tar.gz" \
    | tar xz -C /usr/local/bin/ \
    && chmod +x /usr/local/bin/gog

# Install Deno (runtime for JS/TS modules like openagent — no npm install needed)
RUN curl -fsSL https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip -o /tmp/deno.zip \
    && unzip -o /tmp/deno.zip -d /usr/local/bin/ \
    && chmod +x /usr/local/bin/deno \
    && rm /tmp/deno.zip
ENV DENO_DIR="/tmp/deno"

# Copy the binaries
COPY --from=backend-builder /app/target/release/stark-backend /app/stark-backend-bin

# Copy the built frontend (dist folder)
COPY --from=frontend-builder /app/stark-frontend/dist /app/stark-frontend/dist

# Copy config directory (tokens.ron, presets, networks)
COPY config /app/config

# Copy ABIs for web3 function calls
COPY abis /app/abis

# Copy the skills directory (bundled skills loaded on boot)
COPY skills /app/skills

# Copy soul_template (default SOUL.md and GUIDELINES.md, copied to soul dir on startup)
COPY soul_template /app/soul_template

# Copy bundled modules (discord_tipping, etc.)
COPY modules /app/modules

# Pre-warm uv cache: download Python + dependencies for all module services
# so first-run startup is instant instead of waiting for downloads
RUN for svc in /app/modules/*/service.py; do \
        [ -f "$svc" ] && (cd "$(dirname "$svc")" && uv run --quiet python -c "print('ok')" 2>/dev/null) || true; \
    done

# Pre-warm Deno cache: download npm dependencies for JS module services
# so first-run startup doesn't block on network downloads
RUN for svc in /app/modules/*/service.js; do \
        [ -f "$svc" ] && (cd "$(dirname "$svc")" && deno cache "$svc" 2>/dev/null) || true; \
    done

# Create directories for workspace, journal, soul, and memory (under stark-backend)
RUN mkdir -p /app/stark-backend/workspace /app/stark-backend/journal /app/stark-backend/soul /app/stark-backend/memory /app/stark-backend/notes /app/stark-backend/modules

# Expose ports (HTTP + Gateway WebSocket)
EXPOSE 8080
EXPOSE 8081

# Run the application
CMD ["/app/stark-backend-bin"]
