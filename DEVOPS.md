# Starkbot DevOps

## Seeding Agent Packs into Axoniac

Starkbot ships with a set of agent pack definitions that get provisioned into Axoniac (the global agent pack registry). Users then install these packs onto their Starflask agents during setup.

### One-time setup (you do this, not users)

**Prerequisites:**
- An Axoniac API key (`ax_...`) with write access
- The `axoniac` crate (at `~/ai/axoniac-monorepo/axoniac-rs/`, publish to crates.io when ready)

**Steps:**

```bash
# From the stark-bot repo root:

# 1. Set your Axoniac API key
export AXONIAC_API_KEY=ax_your_key_here

# 2. (Optional) Point to a custom Axoniac instance
# export AXONIAC_BASE_URL=http://localhost:3000

# 3. Run the seed script
cargo run -p seed-packs
```

**What it does:**
1. Reads pack definitions from `seed-packs/packs/*.json` (5 packs: general, crypto, image_gen, video_gen, social_media)
2. For each pack, calls `POST /api/service/packs/provision` on Axoniac — creates the soul, personas, and agent pack globally
3. Writes the resulting content hashes to `config/starflask_seed.ron`
4. **Idempotent** — if a capability already has a real hash in the config, it's skipped

**After running:**
- `config/starflask_seed.ron` will have real SHA256 content hashes instead of placeholders
- Commit this file — the hashes are what users' Starkbot instances use to install packs

### Pack definitions

Pack definition JSONs live in `seed-packs/packs/`:

```
seed-packs/packs/
  general.json       — General-purpose AI assistant
  crypto.json        — Crypto transactions, swaps, bridges
  image_gen.json     — Text-to-image generation (fal.ai)
  video_gen.json     — Text-to-video generation (fal.ai)
  social_media.json  — X/Twitter posting
```

Each JSON contains `soul`, `personas[]`, and `pack` (with `definition`). Edit these to change what agents do, then re-run `cargo run -p seed-packs` to get new hashes.

### Updating packs

Edit a pack definition, then re-provision:

```bash
# Re-provision everything (ignores existing hashes, provisions all packs fresh)
AXONIAC_API_KEY=ax_... cargo run -p seed-packs -- --force

# Re-provision just one capability
AXONIAC_API_KEY=ax_... cargo run -p seed-packs -- --only crypto
```

Axoniac's `provision_pack` is idempotent by content hash — if the content hasn't changed, it returns the same hash (`created: false`). If you changed the definition, it creates a new pack with a new hash.

After re-provisioning, commit the updated `config/starflask_seed.ron`. New Starkbot instances will pick up the new hashes automatically. Existing instances need to reprovision (hit the reprovision button in the UI or `POST /api/starflask/reprovision/{capability}`).

### CLI reference

```bash
# Provision new packs only (skip capabilities that already have hashes)
AXONIAC_API_KEY=ax_... cargo run -p seed-packs

# Re-provision all packs (--force ignores existing hashes)
AXONIAC_API_KEY=ax_... cargo run -p seed-packs -- --force

# Re-provision a single capability
AXONIAC_API_KEY=ax_... cargo run -p seed-packs -- --only image_gen

# Show help
cargo run -p seed-packs -- --help
```

| Flag | Short | Description |
|------|-------|-------------|
| `--force` | `-f` | Re-provision all packs, ignore existing hashes |
| `--only <cap>` | `-o` | Only provision the specified capability |
| `--help` | `-h` | Show usage info |

---

## User Setup Flow

When a user boots Starkbot for the first time:

1. **Step 1 — API Key**: User enters their Starflask API key (`sk_...`) on the setup page
2. **Step 2 — Deploy Agents**: User clicks "Deploy Agents" which:
   - Syncs any existing agents from their Starflask account
   - Creates new agents and installs packs from `config/starflask_seed.ron` (by content hash)
3. **Dashboard**: Once agents are deployed, user lands on the full dashboard

The setup flow gates on two conditions:
- `starflask_api_key_set` — is there an API key?
- `starflask_agents_provisioned > 0` — are there agents?

Both must be true to exit setup.

---

## Docker

```bash
# Build and run (foreground)
./docker_run.sh run

# Build and run (background)
./docker_run.sh daemon

# Other commands
./docker_run.sh logs|shell|restart|down|status
```

Starkbot runs at `http://localhost:8080`.

---

## Publishing the Axoniac Crate

The `axoniac` Rust crate lives at `~/ai/axoniac-monorepo/axoniac-rs/`.

```bash
cd ~/ai/axoniac-monorepo/axoniac-rs
cargo publish
```

Once published, update `seed-packs/Cargo.toml` to use the crates.io version:
```toml
axoniac = "0.1"
```
