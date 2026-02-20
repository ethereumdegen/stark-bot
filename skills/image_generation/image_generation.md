---
name: image_generation
description: "Generate AI images and videos from text prompts via the x402 SuperRouter cloud service at superrouter.defirelay.com. Pays with STARKBOT tokens using the x402 payment protocol."
version: 2.1.2
author: starkbot
homepage: https://superrouter.defirelay.com
metadata: {"clawdbot":{"emoji":"🎨"}}
requires_tools: [x402_post]
arguments:
  prompt:
    description: "Text prompt describing the image or video to generate"
    required: true
  type:
    description: "Generation type: 'image' or 'video'"
    required: false
    default: "image"
  quality:
    description: "Quality tier: 'low', 'medium', or 'high'"
    required: false
    default: "medium"
tags: [image, video, ai, generation, media, creative, x402, fal, super-router]
---

# SuperRouter - AI Media Generation

SuperRouter is an x402-enabled cloud service at **https://superrouter.defirelay.com** that generates **images** and **videos** from text prompts. Payments are made in STARKBOT tokens automatically via the x402 payment protocol.

## Service Info

- **Base URL**: `https://superrouter.defirelay.com`
- **API info**: `POST https://superrouter.defirelay.com/api`
- **Health check**: `GET https://superrouter.defirelay.com/api/health`

## Routes & Quality Tiers

There are 2 routes, each with 3 quality tiers:

### Images (`/generate_image`) [POST]

| Quality | Model | Cost |
|---------|-------|------|
| low | Flux Schnell (fast) | 1,000 STARKBOT |
| medium | Kling v3 | 5,000 STARKBOT |
| high | Kling O3 | 10,000 STARKBOT |

### Videos (`/generate_video`) [POST]

| Quality | Model | Cost |
|---------|-------|------|
| low | MiniMax Hailuo-02 Standard (768p) | 100,000 STARKBOT |
| medium | Kling v3 Standard (1080p) | 150,000 STARKBOT |
| high | Kling v3 Pro (1080p) | 200,000 STARKBOT |

**Default quality is `medium`** — use this unless the user asks for something cheaper (low) or higher quality (high).

## How to Generate Media

Use the `x402_post` tool to call the SuperRouter endpoints. The prompt and quality are passed as a JSON body.

**Tool call format:**
```
x402_post(
  url: "https://superrouter.defirelay.com/generate_image",
  body: {"prompt": "a cute cat", "quality": "medium"}
)
```

**Examples:**
- Medium image (default):
  `x402_post(url: "https://superrouter.defirelay.com/generate_image", body: {"prompt": "a cute cat", "quality": "medium"})`
- Cheap fast image:
  `x402_post(url: "https://superrouter.defirelay.com/generate_image", body: {"prompt": "a cute cat", "quality": "low"})`
- High quality video:
  `x402_post(url: "https://superrouter.defirelay.com/generate_video", body: {"prompt": "a cinematic sunset", "quality": "high"})`

The `x402_post` tool handles the x402 payment protocol automatically — it will sign and submit a STARKBOT permit payment when the server responds with HTTP 402.

## Response Format

The service returns JSON:
```json
{
  "url": "https://cdn.example.com/generate_image/low/{hash}.png",
  "prompt": "the prompt used",
  "cached": false,
  "type": "image",
  "quality": "medium"
}
```

- **url**: Public CDN link to the generated media — share this with the user
- **prompt**: The prompt that was used
- **cached**: Whether this was a cached result (same prompt + quality returns the cached output without charging again)
- **type**: The media type (`"image"` or `"video"`)
- **quality**: The quality tier that was used

**IMPORTANT**: The x402_post response is automatically cached in the `x402_result` register. When sharing the URL with the user, you MUST use `{{x402_result.url}}` in your `say_to_user` message instead of retyping the URL. The URLs contain long SHA256 hashes that get corrupted when retyped — the template is expanded server-side to the exact URL.

**Example say_to_user message:**
```
Here is your generated image: {{x402_result.url}}
```

Other available fields: `{{x402_result.prompt}}`, `{{x402_result.type}}`, `{{x402_result.quality}}`, `{{x402_result.cached}}`

## Usage Guidelines

1. **Default to medium quality** unless the user asks for something specific (e.g. "make it cheap" → low, "best quality" → high)
2. **Just generate immediately** — do NOT ask the user to confirm or say "yes" before generating. Call x402_post right away.
3. **Use `{{x402_result.url}}`** when sharing the URL — NEVER retype the URL manually
4. **Cached results are free** — if the exact same prompt + quality was used before, the cached result is returned at no cost
5. **Default type is image** — if the user doesn't specify, generate an image

## Prompt Tips

- Be specific about style: "a watercolor painting of a sunset over mountains"
- Include mood/atmosphere: "a cozy cabin in the snow, warm light glowing from windows"
- For videos, describe motion: "a cat chasing a laser pointer across a room"
- Mention art styles: "cyberpunk", "studio ghibli", "pixel art", "photorealistic"

## Network & Payment

- **Network**: Base (Ethereum L2)
- **Token**: STARKBOT (ERC-20) at `0x587Cd533F418825521f3A1daa7CCd1E7339A1B07`
- **Payment**: Handled automatically by `x402_post` via EIP-2612 permit signatures (cost is set server-side per endpoint)
- **Facilitator**: x402.org

## Error Handling

- If the user doesn't have enough STARKBOT tokens, the x402 payment will fail — let them know they need STARKBOT on Base
- If an invalid quality is passed, the server returns HTTP 400 with valid options
- If the service is unavailable, retry once after a few seconds
- Rate limit: ~10 requests per minute
