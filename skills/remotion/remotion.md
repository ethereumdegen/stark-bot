---
name: remotion
description: "Create beautiful programmatic videos using Remotion (React-based video framework). Build compositions with text, images, animations, and data — then render to MP4/WebM."
version: 1.0.0
author: starkbot
homepage: https://remotion.dev
metadata: {"clawdbot":{"emoji":"🎬"}}
requires_tools: [exec, write_file, read_file, list_files]
tags: [video, remotion, react, animation, content, creative, media, rendering]
arguments:
  topic:
    description: "The topic or concept for the video"
    required: true
  style:
    description: "Visual style: 'minimal', 'bold', 'cinematic', 'data-viz', 'social'"
    required: false
    default: "bold"
  duration:
    description: "Video duration in seconds"
    required: false
    default: "15"
  resolution:
    description: "Output resolution: '720p', '1080p', '4k'"
    required: false
    default: "1080p"
---

# Remotion — Programmatic Video Creation

Remotion is a React-based framework for creating videos programmatically. Write React components, animate them with code, and render to MP4/WebM. Perfect for data visualizations, social media content, explainers, and branded videos.

## Prerequisites

- **Node.js** (v18+) and **npm/bun** must be available
- A Remotion project directory (created per video or reused)

## Workspace & Output

All Remotion projects and rendered output live in the **workspace** directory:

- **Remotion project**: `workspace/remotion/` (source code, components)
- **Rendered videos**: `workspace/remotion/out/` (MP4, WebM, stills)

This keeps everything persistent and accessible to other skills (e.g., posting to Twitter/Discord).

## Project Setup

### First-time setup (if no Remotion project exists)

Create a new Remotion project in the workspace:

```json
{
  "tool": "exec",
  "command": "mkdir -p workspace/remotion && cd workspace/remotion && npx create-video@latest . --template blank && npm install"
}
```

This creates the project at `workspace/remotion/` with the standard structure:

```
workspace/remotion/
├── src/
│   ├── Root.tsx          # Entry point — registers all compositions
│   ├── Composition.tsx   # Default composition component
│   └── ...
├── out/                  # Rendered videos and stills
├── remotion.config.ts    # Remotion config
├── package.json
└── tsconfig.json
```

### If a project already exists

Reuse `workspace/remotion/` — just update the composition files.

## Resolution Reference

| Preset | Width | Height |
|--------|-------|--------|
| 720p | 1280 | 720 |
| 1080p | 1920 | 1080 |
| 4k | 3840 | 2160 |
| Square (social) | 1080 | 1080 |
| Story/Reel | 1080 | 1920 |

## Creating a Video — Step by Step

### Step 1: Design the Composition

Plan the video scenes based on the topic. A typical video has:
- **Intro** (2-3s): Title card with fade-in
- **Body** (variable): Main content, data, or animation
- **Outro** (2-3s): Closing card, CTA, or branding

### Step 2: Write the React Component

Create or update the main composition file. Here's a template for a bold, text-driven video:

```json
{
  "tool": "write_file",
  "path": "workspace/remotion/src/MyVideo.tsx",
  "content": "import { AbsoluteFill, useCurrentFrame, useVideoConfig, interpolate, spring, Sequence, Img } from 'remotion';\n\nexport const MyVideo: React.FC<{ title: string; subtitle: string; }> = ({ title, subtitle }) => {\n  const frame = useCurrentFrame();\n  const { fps, width, height } = useVideoConfig();\n\n  // Fade in\n  const opacity = interpolate(frame, [0, 30], [0, 1], { extrapolateRight: 'clamp' });\n\n  // Spring animation for title\n  const titleScale = spring({ frame, fps, config: { damping: 12, stiffness: 200 } });\n\n  return (\n    <AbsoluteFill style={{ backgroundColor: '#0a0a0a', justifyContent: 'center', alignItems: 'center' }}>\n      <Sequence from={0} durationInFrames={fps * 4}>\n        <AbsoluteFill style={{ justifyContent: 'center', alignItems: 'center', opacity }}>\n          <h1 style={{\n            color: '#fff',\n            fontSize: 80,\n            fontWeight: 900,\n            textAlign: 'center',\n            transform: `scale(${titleScale})`,\n            maxWidth: '80%',\n          }}>{title}</h1>\n        </AbsoluteFill>\n      </Sequence>\n      <Sequence from={fps * 4}>\n        <AbsoluteFill style={{ justifyContent: 'center', alignItems: 'center' }}>\n          <p style={{\n            color: '#ccc',\n            fontSize: 48,\n            textAlign: 'center',\n            maxWidth: '70%',\n          }}>{subtitle}</p>\n        </AbsoluteFill>\n      </Sequence>\n    </AbsoluteFill>\n  );\n};\n"
}
```

### Step 3: Register the Composition

Update `Root.tsx` to register the composition with the desired duration and resolution:

```json
{
  "tool": "write_file",
  "path": "workspace/remotion/src/Root.tsx",
  "content": "import { Composition } from 'remotion';\nimport { MyVideo } from './MyVideo';\n\nexport const RemotionRoot: React.FC = () => {\n  return (\n    <>\n      <Composition\n        id=\"MyVideo\"\n        component={MyVideo}\n        durationInFrames={30 * 15}\n        fps={30}\n        width={1920}\n        height={1080}\n        defaultProps={{\n          title: 'Hello World',\n          subtitle: 'Built with Remotion',\n        }}\n      />\n    </>\n  );\n};\n"
}
```

**Key parameters:**
- `durationInFrames`: fps * seconds (e.g., 30fps * 15s = 450 frames)
- `fps`: 30 is standard, 60 for smooth motion
- `width` / `height`: see resolution table above
- `defaultProps`: passed to the component

### Step 4: Preview (optional)

Open the Remotion Studio for live preview:

```json
{
  "tool": "exec",
  "command": "cd workspace/remotion && npx remotion studio"
}
```

This opens a browser-based preview at `http://localhost:3000`.

### Step 5: Render the Video

Render to MP4:

```json
{
  "tool": "exec",
  "command": "cd workspace/remotion && npx remotion render MyVideo out/output-video.mp4 --codec h264"
}
```

**Render options:**
- `--codec h264` — MP4 output (most compatible)
- `--codec vp8` — WebM output (smaller file)
- `--quality 80` — JPEG quality for frames (0-100)
- `--crf 18` — Constant Rate Factor (lower = better quality, 15-23 recommended)
- `--concurrency 4` — parallel frame rendering (faster on multi-core)
- `--muted` — skip audio rendering
- `--image-format png` — use PNG instead of JPEG for frames (better for text/graphics)

**Render a still frame (thumbnail):**
```json
{
  "tool": "exec",
  "command": "cd workspace/remotion && npx remotion still MyVideo out/thumbnail.png --frame 0"
}
```

## Remotion Primitives Reference

### Core Components

| Component | Purpose |
|-----------|---------|
| `<AbsoluteFill>` | Full-frame container (like a slide) |
| `<Sequence>` | Time-based section (`from`, `durationInFrames`) |
| `<Series>` | Sequential scenes (auto-advancing) |
| `<Img>` | Image (use instead of `<img>`) |
| `<Video>` | Embed video clips |
| `<Audio>` | Audio tracks |
| `<OffthreadVideo>` | Video optimized for rendering (not preview) |

### Animation Utilities

| Utility | Purpose |
|---------|---------|
| `useCurrentFrame()` | Current frame number |
| `useVideoConfig()` | Get fps, width, height, durationInFrames |
| `interpolate(frame, inputRange, outputRange)` | Map frame to animated value |
| `spring({ frame, fps, config })` | Physics-based spring animation |
| `Easing.*` | Easing functions (linear, ease, bezier) |

### Common Patterns

**Fade in:**
```tsx
const opacity = interpolate(frame, [0, 30], [0, 1], { extrapolateRight: 'clamp' });
```

**Slide in from left:**
```tsx
const translateX = interpolate(frame, [0, 20], [-200, 0], { extrapolateRight: 'clamp' });
```

**Spring bounce:**
```tsx
const scale = spring({ frame, fps, config: { damping: 10, stiffness: 100 } });
```

**Staggered items:**
```tsx
{items.map((item, i) => {
  const delay = i * 10;
  const opacity = interpolate(frame, [delay, delay + 20], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' });
  return <div style={{ opacity }}>{item}</div>;
})}
```

**Color transition:**
```tsx
const color = interpolateColors(frame, [0, 60], ['#ff0000', '#0000ff']);
```

## Style Presets

### Minimal
- Background: white or very light gray (`#fafafa`)
- Typography: thin/light weight, large size, dark text
- Animations: subtle fades, gentle slides
- Good for: professional, clean, corporate

### Bold
- Background: dark (`#0a0a0a`) or vibrant solid color
- Typography: extra-bold, large, high contrast
- Animations: springs, bounces, scale transforms
- Good for: social media, attention-grabbing, crypto/tech

### Cinematic
- Background: gradients or background images with overlays
- Typography: serif fonts, elegant spacing
- Animations: slow fades, parallax movement
- Good for: storytelling, documentaries, intros

### Data-viz
- Background: dark or muted
- Elements: charts, counters, progress bars animated over time
- Animations: number counting, bar growth, pie chart fill
- Good for: stats, metrics, financial data, reports

### Social (vertical)
- Resolution: 1080x1920 (story/reel format)
- Typography: large, readable on mobile
- Animations: fast, punchy, attention-grabbing
- Good for: TikTok, Instagram Reels, YouTube Shorts

## Workflow Summary

1. User describes the video topic and style
2. Plan the scenes (intro, body sections, outro)
3. Write the React composition component(s) in `workspace/remotion/src/`
4. Register in Root.tsx with correct duration/resolution
5. Render to MP4 in `workspace/remotion/out/` with `npx remotion render`
6. Return the output file path to the user (e.g., `workspace/remotion/out/output-video.mp4`)

## Combining with Other Skills

- **Firecrawl** → Scrape data from a URL → Visualize it in a Remotion video
- **Image Generation** → Generate background images → Composite in Remotion
- **Twitter/Discord** → Auto-post rendered videos to social
- **Token Price / DexScreener** → Fetch price data → Animated price chart video

## Error Handling

- **"Cannot find module 'remotion'"**: Run `npm install` in the project directory
- **Render hangs**: Reduce `--concurrency` or check for infinite loops in animation logic
- **Black frames**: Ensure component renders something for all frame ranges
- **"Composition not found"**: Check that Root.tsx exports the composition with the correct `id`
- **Large file size**: Lower `--crf` value, reduce resolution, or use `--codec vp8` for WebM
- **Missing fonts**: Use system fonts or install via `npm install @fontsource/inter` and import in component

## Tips for Great Videos

- Keep text concise — videos move fast, less is more
- Use `spring()` for natural-feeling motion (better than linear interpolation)
- Add slight delays between elements appearing (stagger) for a polished feel
- Use `extrapolateRight: 'clamp'` to prevent values going past their target
- Test with `npx remotion still` to quickly preview individual frames before full render
- For data-driven videos, pass data as `defaultProps` and animate through it
