---
name: figma
description: "Inspect Figma designs — browse files, extract design tokens, export images, read comments, and get component/style details."
version: 1.0.0
author: starkbot
homepage: https://www.figma.com/developers/api
metadata: '{"clawdbot":{"emoji":"🎨"}}'
tags: [design, figma, ui, ux, components, styles, export]
requires_tools: [figma]
requires_api_keys:
  FIGMA_ACCESS_TOKEN:
    description: "Figma personal access token (Settings > Account > Personal access tokens)"
    secret: true
---

# Figma Design Integration

Use the `figma` tool to interact with Figma files. Requires a **FIGMA_ACCESS_TOKEN** (generate at: Figma Settings > Account > Personal access tokens).

## Getting the File Key

Every Figma URL contains the file key:
```
https://www.figma.com/design/ABC123xyz/My-Design?node-id=1:2
                              ^^^^^^^^^^^
                              This is the file_key
```

## Common Workflows

### 1. Browse a Design File
Start with a shallow overview, then drill into specific sections:
```tool:figma
action: get_file
file_key: <file_key>
depth: 1
```
This returns pages and top-level frames. Increase `depth` for more detail.

### 2. Inspect Specific Nodes
Use node IDs from get_file to get full geometry, styles, and properties:
```tool:figma
action: get_nodes
file_key: <file_key>
node_ids: 1:2,1:3
```

### 3. Export Design Elements as Images
Export specific frames/components as PNG, SVG, JPG, or PDF:
```tool:figma
action: get_images
file_key: <file_key>
node_ids: 1:2
format: png
scale: 2
```
Returns temporary download URLs (valid ~14 days).

### 4. Extract Design Tokens
Get published styles (colors, typography, effects):
```tool:figma
action: get_styles
file_key: <file_key>
```

Get design variables (spacing, colors, breakpoints with modes like light/dark):
```tool:figma
action: get_variables
file_key: <file_key>
```

### 5. List Components
Get all published components in a file:
```tool:figma
action: get_components
file_key: <file_key>
```

### 6. Read Design Feedback
```tool:figma
action: get_comments
file_key: <file_key>
```

### 7. Browse Team/Project Files
List all projects in a team:
```tool:figma
action: list_projects
team_id: <team_id>
```

List files in a project:
```tool:figma
action: list_files
project_id: <project_id>
```

## Design-to-Code Tips

When converting Figma designs to code:
1. **Start with get_file** (depth=1) to see the page structure
2. **Use get_nodes** on specific frames to get exact dimensions, colors, fonts, spacing
3. **Use get_styles** to extract reusable design tokens (map to CSS variables / theme)
4. **Use get_variables** for responsive/themed values (light/dark mode)
5. **Export assets** with get_images (SVG for icons, PNG @2x for raster images)
6. Node properties include `absoluteBoundingBox` (position/size), `fills` (colors), `strokes`, `effects` (shadows), and `style` (typography)

## Node ID Format
Node IDs look like `1:2`, `123:456`. They appear in:
- The `id` field of every node in get_file responses
- Figma URLs as `?node-id=1-2` (replace `-` with `:`)
