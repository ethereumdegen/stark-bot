---
name: excalidraw
description: "Generate architecture diagrams as .excalidraw files from codebase analysis, with optional PNG/SVG export. Use when asked to create architecture diagrams, system diagrams, visualize codebase structure, or generate excalidraw files."
version: 1.1.0
author: starkbot
metadata: '{"clawdbot":{"emoji":"📐"}}'
tags: [diagram, architecture, excalidraw, visualization, codebase, svg, png]
requires_tools: [glob, grep, read_file, write_file, exec, run_skill_script, say_to_user]
scripts: [excalidraw.py]
arguments:
  path:
    description: "Root path of the codebase to analyze (defaults to current workspace)"
    required: false
    default: "."
  output:
    description: "Output directory for the diagram (defaults to docs/architecture/)"
    required: false
    default: "docs/architecture"
  export_format:
    description: "Optional export format: png, svg, or both"
    required: false
---

# Excalidraw Diagram Generator

Generate architecture diagrams as `.excalidraw` files directly from codebase analysis, with optional export to PNG and SVG.

## Quick Start

**User asks:**
```
"Generate an architecture diagram for this project"
"Create an excalidraw diagram of the system"
"Visualize this codebase as an excalidraw file"
```

**The skill will:**
1. Analyze the codebase (any language/framework)
2. Identify components, services, databases, APIs
3. Map relationships and data flows
4. Generate valid `.excalidraw` JSON with dynamic IDs and labels
5. Optionally export to PNG and/or SVG

**No prerequisites:** Works without existing diagrams, Terraform, or specific file types.

---

## Critical Rules

### 1. NEVER Use Diamond Shapes

Diamond arrow connections are broken in raw Excalidraw JSON. Use styled rectangles instead:

| Semantic Meaning | Rectangle Style |
|------------------|-----------------|
| Orchestrator/Hub | Coral (`#ffa8a8`/`#c92a2a`) + strokeWidth: 3 |
| Decision Point | Orange (`#ffd8a8`/`#e8590c`) + dashed stroke |

### 2. Labels Require TWO Elements

The `label` property does NOT work in raw JSON. Every labeled shape needs:

```json
// 1. Shape with boundElements reference
{
  "id": "my-box",
  "type": "rectangle",
  "boundElements": [{ "type": "text", "id": "my-box-text" }]
}

// 2. Separate text element with containerId
{
  "id": "my-box-text",
  "type": "text",
  "containerId": "my-box",
  "text": "My Label"
}
```

### 3. Elbow Arrows Need Three Properties

For 90-degree corners (not curved):

```json
{
  "type": "arrow",
  "roughness": 0,
  "roundness": null,
  "elbowed": true
}
```

### 4. Arrow Edge Calculations

Arrows must start/end at shape edges, not centers:

| Edge | Formula |
|------|---------|
| Top | `(x + width/2, y)` |
| Bottom | `(x + width/2, y + height)` |
| Left | `(x, y + height/2)` |
| Right | `(x + width, y + height/2)` |

**Full arrow reference:** See `references/arrows.md`

---

## Element Types

| Type | Use For |
|------|---------|
| `rectangle` | Services, databases, containers, orchestrators |
| `ellipse` | Users, external systems, start/end points |
| `text` | Labels inside shapes, titles, annotations |
| `arrow` | Data flow, connections, dependencies |
| `line` | Grouping boundaries, separators |

**Full JSON format:** See `references/json-format.md`

---

## Workflow

### Step 1: Analyze Codebase

Discover components by looking for:

| Codebase Type | What to Look For |
|---------------|------------------|
| Monorepo | `packages/*/package.json`, workspace configs |
| Microservices | `docker-compose.yml`, k8s manifests |
| IaC | Terraform/Pulumi resource definitions |
| Backend API | Route definitions, controllers, DB models |
| Frontend | Component hierarchy, API calls |
| Rust | `Cargo.toml`, module structure, `mod.rs` files |

**Use tools:**
```tool:glob
pattern: "**/package.json"
limit: 30
```

```tool:glob
pattern: "**/Dockerfile"
limit: 20
```

```tool:grep
pattern: "app.get|@Controller|CREATE TABLE|pub fn|async fn"
glob: "*.{rs,ts,js,py,go}"
output_mode: files_with_matches
```

```tool:read_file
path: README.md
max_lines: 100
```

### Step 2: Plan Layout

**Vertical flow (most common):**
```
Row 1: Users/Entry points       (y: 100)
Row 2: Frontend/Gateway          (y: 230)
Row 3: Orchestration             (y: 380)
Row 4: Services                  (y: 530)
Row 5: Data layer                (y: 680)

Columns: x = 100, 300, 500, 700, 900
Element size: 160-200px x 80-90px
```

**Other layout patterns:** See `references/examples.md`

### Step 3: Generate Elements

For each component:
1. Create shape with unique `id`
2. Add `boundElements` referencing text
3. Create text with `containerId`
4. Choose color based on type

**Color palettes:** See `references/colors.md`

### Step 4: Add Connections

For each relationship:
1. Calculate source edge point
2. Plan elbow route (avoid overlaps)
3. Create arrow with `points` array
4. Match stroke color to destination type

**Arrow patterns:** See `references/arrows.md`

### Step 5: Add Grouping (Optional)

For logical groupings:
- Large transparent rectangle with `strokeStyle: "dashed"`
- Standalone text label at top-left

### Step 6: Write and Validate

Write the `.excalidraw` file to `{{output}}/` or user-specified path, then validate it using the bundled script:

```tool:run_skill_script
script: excalidraw.py
action: validate
args: {"file": "{{output}}/diagram.excalidraw"}
```

If validation returns errors, fix them and re-validate before proceeding.

**Validation checklist:** See `references/validation.md`

### Step 7: Export to PNG/SVG and Share

If the user requests `{{export_format}}` (png, svg, or both), export using the bundled script. Use `save_public: true` to make the image accessible via the web UI:

```tool:run_skill_script
script: excalidraw.py
action: export
args: {"file": "{{output}}/diagram.excalidraw", "format": "png", "save_public": true}
```

The script returns a `public_url` (e.g., `/public/diagram.png`). Share it with the user so the image renders inline:

```tool:say_to_user
message: "Here's the architecture diagram: /public/diagram.png"
```

Alternatively, export using the exec tool with a Node.js script leveraging `@excalidraw/utils`.

**Full export procedure:** See `references/export.md`

---

## Quick Arrow Reference

**Straight down:**
```json
{ "points": [[0, 0], [0, 110]], "x": 590, "y": 290 }
```

**L-shape (left then down):**
```json
{ "points": [[0, 0], [-325, 0], [-325, 125]], "x": 525, "y": 420 }
```

**U-turn (callback):**
```json
{ "points": [[0, 0], [50, 0], [50, -125], [20, -125]], "x": 710, "y": 440 }
```

**Arrow width/height** = bounding box of points:
```
points [[0,0], [-440,0], [-440,70]] -> width=440, height=70
```

**Multiple arrows from same edge** - stagger positions:
```
5 arrows: 20%, 35%, 50%, 65%, 80% across edge width
```

---

## Default Color Palette

| Component | Background | Stroke |
|-----------|------------|--------|
| Frontend | `#a5d8ff` | `#1971c2` |
| Backend/API | `#d0bfff` | `#7048e8` |
| Database | `#b2f2bb` | `#2f9e44` |
| Storage | `#ffec99` | `#f08c00` |
| AI/ML | `#e599f7` | `#9c36b5` |
| External APIs | `#ffc9c9` | `#e03131` |
| Orchestration | `#ffa8a8` | `#c92a2a` |
| Message Queue | `#fff3bf` | `#fab005` |
| Cache | `#ffe8cc` | `#fd7e14` |
| Users | `#e7f5ff` | `#1971c2` |

**Cloud-specific palettes (AWS, Azure, GCP, K8s):** See `references/colors.md`

---

## Quick Validation Checklist

Before writing file:
- [ ] Every shape with label has boundElements + text element
- [ ] Text elements have containerId matching shape
- [ ] Multi-point arrows have `elbowed: true`, `roundness: null`
- [ ] Arrow x,y = source shape edge point
- [ ] Arrow final point offset reaches target edge
- [ ] No diamond shapes
- [ ] No duplicate IDs

**Full validation algorithm:** See `references/validation.md`

---

## Common Issues

| Issue | Fix |
|-------|-----|
| Labels don't appear | Use TWO elements (shape + text), not `label` property |
| Arrows curved | Add `elbowed: true`, `roundness: null`, `roughness: 0` |
| Arrows floating | Calculate x,y from shape edge, not center |
| Arrows overlapping | Stagger start positions across edge |

---

## Reference Files

| File | Contents |
|------|----------|
| `references/json-format.md` | Element types, required properties, text bindings |
| `references/arrows.md` | Routing algorithm, patterns, bindings, staggering |
| `references/colors.md` | Default, AWS, Azure, GCP, K8s palettes |
| `references/examples.md` | Complete JSON examples, layout patterns |
| `references/validation.md` | Checklists, validation algorithm, bug fixes |
| `references/export.md` | PNG/SVG export procedure |

---

## Output

- **Location:** `{{output}}/` or user-specified path
- **Filename:** Descriptive, e.g., `system-architecture.excalidraw`
- **Exports (optional):** `system-architecture.svg` and/or `system-architecture.png` in same directory
- **Testing:** Open `.excalidraw` in https://excalidraw.com or VS Code extension

## Tools Used

| Tool | Purpose |
|------|---------|
| `glob` | Find files by pattern during codebase analysis |
| `grep` | Search file contents for components and connections |
| `read_file` | Read config files, entry points, READMEs |
| `write_file` | Write the `.excalidraw` JSON file |
| `exec` | Run export scripts for PNG/SVG conversion |
| `run_skill_script` | Run `excalidraw.py` for validation and export |
| `say_to_user` | Share the public URL so images render inline in chat |
