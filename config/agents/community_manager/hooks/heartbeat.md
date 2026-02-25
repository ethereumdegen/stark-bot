Heartbeat fired at {timestamp}.

## Step 1 — Check if already posted today

Extract today's date from `{timestamp}` (use the `YYYY-MM-DD` portion).

Call `kv_store(action="get", key="CM_TWEETED_YYYY_MM_DD")` (replace YYYY_MM_DD with today's date, e.g. `CM_TWEETED_2026_02_24`).

- If the key has a value → call `task_fully_completed(summary="Already posted today — skipping")` and **stop**.
- If the key is empty / not found → continue.

## Step 2 — Generate image

Call `x402_post` to generate an image:

- URL: `https://superrouter.defirelay.com/generate_image`
- Prompt: come up with a vivid, creative scene featuring a sleek blue robot in an inspirational setting. Vary it every day — cityscapes, space vistas, sunrise horizons, neon-lit streets, lush nature, futuristic labs, etc.
- Quality: `"low"`

## Step 3 — Compose and post tweet

Write an inspirational tweet (max 280 chars). Tone: motivational, futuristic, AI-forward. Topics: tech, innovation, building, perseverance.

Post via `twitter_post(text="<your tweet>", media_url="{{x402_result.url}}")`.

## Step 4 — Set dedup flag

Call `kv_store(action="set", key="CM_TWEETED_YYYY_MM_DD", value="true")` (same date key from Step 1).

## Step 5 — Done

Call `task_fully_completed(summary="Posted daily inspirational tweet: <brief description>")`.
