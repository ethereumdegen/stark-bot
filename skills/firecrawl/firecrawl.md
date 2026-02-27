---
name: firecrawl
description: "Scrape, crawl, and extract structured data from websites using the Firecrawl API. Turns web pages into clean markdown or structured JSON for research, analysis, and content pipelines."
version: 1.0.0
author: starkbot
homepage: https://docs.firecrawl.dev
metadata: {"clawdbot":{"emoji":"🔥"}}
requires_tools: [web_fetch, exec]
tags: [web, scraping, crawling, research, data, firecrawl, extraction]
arguments:
  url:
    description: "The URL to scrape or the starting URL for a crawl"
    required: true
  mode:
    description: "Operation mode: 'scrape' (single page), 'crawl' (multi-page), 'map' (discover URLs), or 'extract' (structured extraction)"
    required: false
    default: "scrape"
  prompt:
    description: "For 'extract' mode: a natural language prompt describing what data to extract"
    required: false
---

# Firecrawl — Web Scraping & Crawling

Firecrawl turns any website into clean, LLM-ready markdown or structured data. Use it to research topics, extract information from web pages, gather content for analysis, or feed data into other skills.

## Prerequisites

- **API Key**: The `FIRECRAWL_API_KEY` environment variable must be set
- **Base URL**: `https://api.firecrawl.dev/v1`

## Operations

### 1. Scrape a Single Page (default)

Fetches a single URL and returns clean markdown content.

```json
{
  "tool": "web_fetch",
  "url": "https://api.firecrawl.dev/v1/scrape",
  "method": "POST",
  "headers": {
    "Content-Type": "application/json",
    "Authorization": "Bearer {{env.FIRECRAWL_API_KEY}}"
  },
  "body": {
    "url": "{{url}}",
    "formats": ["markdown"]
  },
  "extract_mode": "raw"
}
```

**Response** contains `data.markdown` with the clean page content.

**Options you can add to the body:**
- `"formats": ["markdown", "html", "screenshot"]` — get multiple formats
- `"onlyMainContent": true` — strip navbars, footers, sidebars (default: true)
- `"waitFor": 3000` — wait N ms for JS to render before scraping
- `"mobile": true` — emulate mobile viewport
- `"includeTags": ["article", "main"]` — only include specific HTML tags
- `"excludeTags": ["nav", "footer"]` — exclude specific HTML tags

### 2. Crawl Multiple Pages

Crawl an entire site or section starting from a URL. This is asynchronous — you start the crawl, then poll for results.

**Start the crawl:**
```json
{
  "tool": "web_fetch",
  "url": "https://api.firecrawl.dev/v1/crawl",
  "method": "POST",
  "headers": {
    "Content-Type": "application/json",
    "Authorization": "Bearer {{env.FIRECRAWL_API_KEY}}"
  },
  "body": {
    "url": "{{url}}",
    "limit": 10,
    "maxDepth": 2
  },
  "extract_mode": "raw"
}
```

**Response** returns a `id` for the crawl job.

**Check crawl status:**
```json
{
  "tool": "web_fetch",
  "url": "https://api.firecrawl.dev/v1/crawl/<crawl_id>",
  "method": "GET",
  "headers": {
    "Authorization": "Bearer {{env.FIRECRAWL_API_KEY}}"
  },
  "extract_mode": "raw"
}
```

Poll every 5-10 seconds until `status` is `"completed"`. Results are in `data[]` with markdown for each page.

**Crawl options:**
- `"limit": 10` — max pages to crawl (default 10, be conservative)
- `"maxDepth": 2` — how many links deep to follow
- `"includePaths": ["/blog/*"]` — only crawl matching paths
- `"excludePaths": ["/admin/*"]` — skip matching paths
- `"allowBackwardLinks": false` — don't follow links to parent directories

### 3. Map (Discover URLs)

Quickly discover all URLs on a site without fetching content. Great for understanding site structure.

```json
{
  "tool": "web_fetch",
  "url": "https://api.firecrawl.dev/v1/map",
  "method": "POST",
  "headers": {
    "Content-Type": "application/json",
    "Authorization": "Bearer {{env.FIRECRAWL_API_KEY}}"
  },
  "body": {
    "url": "{{url}}"
  },
  "extract_mode": "raw"
}
```

**Response** contains `links[]` — an array of discovered URLs.

**Options:**
- `"search": "pricing"` — filter results by a search term
- `"limit": 100` — max URLs to return

### 4. Extract (Structured Data)

Extract structured data from a page using a natural language prompt. Returns JSON matching your description.

```json
{
  "tool": "web_fetch",
  "url": "https://api.firecrawl.dev/v1/scrape",
  "method": "POST",
  "headers": {
    "Content-Type": "application/json",
    "Authorization": "Bearer {{env.FIRECRAWL_API_KEY}}"
  },
  "body": {
    "url": "{{url}}",
    "formats": ["extract"],
    "extract": {
      "prompt": "{{prompt}}"
    }
  },
  "extract_mode": "raw"
}
```

**Example prompt:** `"Extract all product names, prices, and ratings from this page"`

The response `data.extract` contains the structured JSON.

You can also provide a JSON schema for more precise extraction:
```json
{
  "extract": {
    "prompt": "Extract product information",
    "schema": {
      "type": "object",
      "properties": {
        "products": {
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "name": {"type": "string"},
              "price": {"type": "number"},
              "rating": {"type": "number"}
            }
          }
        }
      }
    }
  }
}
```

## Quick Reference

| Action | Endpoint | Method |
|--------|----------|--------|
| Scrape single page | `/v1/scrape` | POST |
| Start crawl | `/v1/crawl` | POST |
| Check crawl status | `/v1/crawl/{id}` | GET |
| Cancel crawl | `/v1/crawl/{id}` | DELETE |
| Map site URLs | `/v1/map` | POST |

## Workflow

1. **User asks to scrape/research a URL** → Use **scrape** mode (single page) or **crawl** (multi-page)
2. **User asks "what pages are on this site"** → Use **map** mode
3. **User asks to extract specific data** → Use **extract** mode with a prompt
4. **Store useful results** in memory for later use with `memory_store`
5. **Feed results** into other skills (e.g., summarize content, generate images from descriptions, etc.)

## Error Handling

- **401 Unauthorized**: Check that `FIRECRAWL_API_KEY` is set correctly
- **402 Payment Required**: Firecrawl plan limit reached
- **429 Rate Limited**: Wait and retry after a few seconds
- **Timeout on crawl**: Reduce `limit` and `maxDepth` — large crawls take time
- If a page requires JavaScript rendering, add `"waitFor": 5000` to let it load

## Usage Tips

- Start with **scrape** for a single page — it's fastest
- Use **map** first to understand a site before crawling it
- Keep crawl `limit` low (5-20) unless the user specifically needs more
- For JS-heavy sites (SPAs), use `"waitFor"` to let content render
- Combine with other skills: scrape content → summarize → generate video with Remotion
