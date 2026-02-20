---
name: github_discussions
description: "Read and write GitHub Discussions using the GraphQL API via gh CLI."
version: 1.0.0
author: starkbot
homepage: https://docs.github.com/en/graphql/guides/using-the-graphql-api-for-discussions
metadata: {"requires_auth": true, "clawdbot":{"emoji":"💬"}}
requires_tools: [exec, api_keys_check]
tags: [github, discussions, community, graphql, api]
---

# GitHub Discussions Guide

GitHub Discussions use the **GraphQL API** (not REST). Use `gh api graphql` via the `exec` tool for all operations.

## Authentication

Discussions API requires `repo` scope (private repos) or `public_repo` scope (public repos).

```json
{"tool": "api_keys_check", "key_name": "GITHUB_TOKEN"}
```

---

## Quick Reference

| Operation | Section |
|-----------|---------|
| List discussions | [List Discussions](#list-discussions) |
| View single discussion | [Get Discussion](#get-single-discussion) |
| Get categories | [Get Categories](#get-discussion-categories) |
| Create discussion | [Create Discussion](#create-discussion) |
| Add comment | [Add Comment](#add-discussion-comment) |
| Reply to comment | [Reply to Comment](#reply-to-comment) |
| Mark as answer | [Mark as Answer](#mark-comment-as-answer) |
| Update discussion | [Update Discussion](#update-discussion) |
| Delete discussion | [Delete Discussion](#delete-discussion) |

---

## List Discussions

List discussions in a repository:

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { discussions(first: 10, orderBy: {field: CREATED_AT, direction: DESC}) { nodes { number title author { login } category { name } createdAt isAnswered url } } } }'"
}
```

### Filter by Category

First get category ID, then filter:

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { discussions(first: 10, categoryId: \"CATEGORY_ID\") { nodes { number title author { login } createdAt url } } } }'"
}
```

### With Pagination

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { discussions(first: 10, after: \"CURSOR\") { pageInfo { hasNextPage endCursor } nodes { number title url } } } }'"
}
```

---

## Get Single Discussion

View a specific discussion by number:

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { discussion(number: NUMBER) { id title body author { login } category { name } createdAt isAnswered answer { id body author { login } } comments(first: 20) { totalCount nodes { id body author { login } createdAt isAnswer replies(first: 5) { nodes { body author { login } } } } } } } }'"
}
```

### Get Discussion ID (needed for mutations)

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { discussion(number: NUMBER) { id } } }' --jq '.data.repository.discussion.id'"
}
```

---

## Get Discussion Categories

Get available categories (required before creating discussions):

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { discussionCategories(first: 25) { nodes { id name description emoji isAnswerable } } } }'"
}
```

### Get Repository ID (needed for creating discussions)

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { id } }' --jq '.data.repository.id'"
}
```

---

## Create Discussion

**Requires:** Repository ID and Category ID (get them first with queries above)

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='mutation { createDiscussion(input: { repositoryId: \"REPO_ID\", categoryId: \"CATEGORY_ID\", title: \"Discussion Title\", body: \"Discussion content in markdown\" }) { discussion { id number title url } } }'"
}
```

### Complete Workflow: Create Discussion

1. **Get Repository ID:**
```json
{"tool": "exec", "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { id } }' --jq '.data.repository.id'"}
```

2. **Get Category ID (pick one from the list):**
```json
{"tool": "exec", "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { discussionCategories(first: 25) { nodes { id name } } } }'"}
```

3. **Create Discussion:**
```json
{"tool": "exec", "command": "gh api graphql -f query='mutation { createDiscussion(input: { repositoryId: \"R_xxx\", categoryId: \"DIC_xxx\", title: \"My Title\", body: \"Content here\" }) { discussion { number url } } }'"}
```

---

## Add Discussion Comment

**Requires:** Discussion ID (not number!)

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='mutation { addDiscussionComment(input: { discussionId: \"DISCUSSION_ID\", body: \"Your comment text\" }) { comment { id body createdAt author { login } } } }'"
}
```

### Complete Workflow: Add Comment

1. **Get Discussion ID from number:**
```json
{"tool": "exec", "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { discussion(number: NUMBER) { id } } }' --jq '.data.repository.discussion.id'"}
```

2. **Add comment:**
```json
{"tool": "exec", "command": "gh api graphql -f query='mutation { addDiscussionComment(input: { discussionId: \"D_xxx\", body: \"My comment\" }) { comment { id url } } }'"}
```

---

## Reply to Comment

Reply to an existing comment (creates a threaded reply):

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='mutation { addDiscussionComment(input: { discussionId: \"DISCUSSION_ID\", body: \"Reply text\", replyToId: \"COMMENT_ID\" }) { comment { id body } } }'"
}
```

---

## Mark Comment as Answer

For Q&A category discussions, mark a comment as the answer:

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='mutation { markDiscussionCommentAsAnswer(input: { id: \"COMMENT_ID\" }) { discussion { isAnswered answer { id } } } }'"
}
```

### Unmark as Answer

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='mutation { unmarkDiscussionCommentAsAnswer(input: { id: \"COMMENT_ID\" }) { discussion { isAnswered } } }'"
}
```

---

## Update Discussion

Update title, body, or category of existing discussion:

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='mutation { updateDiscussion(input: { discussionId: \"DISCUSSION_ID\", title: \"New Title\", body: \"Updated body\" }) { discussion { id title body } } }'"
}
```

### Update Just the Category

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='mutation { updateDiscussion(input: { discussionId: \"DISCUSSION_ID\", categoryId: \"NEW_CATEGORY_ID\" }) { discussion { id category { name } } } }'"
}
```

---

## Update Comment

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='mutation { updateDiscussionComment(input: { commentId: \"COMMENT_ID\", body: \"Updated comment text\" }) { comment { id body updatedAt } } }'"
}
```

---

## Delete Discussion

**Warning:** This permanently deletes the discussion and all comments.

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='mutation { deleteDiscussion(input: { id: \"DISCUSSION_ID\" }) { discussion { id title } } }'"
}
```

---

## Delete Comment

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='mutation { deleteDiscussionComment(input: { id: \"COMMENT_ID\" }) { comment { id } } }'"
}
```

---

## Get Pinned Discussions

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='query { repository(owner: \"OWNER\", name: \"REPO\") { pinnedDiscussions(first: 10) { nodes { discussion { number title url } } } } }'"
}
```

---

## Search Discussions

Search discussions with a query string:

```json
{
  "tool": "exec",
  "command": "gh api graphql -f query='query { search(query: \"repo:OWNER/REPO is:discussion keyword\", type: DISCUSSION, first: 10) { discussionCount nodes { ... on Discussion { number title url author { login } } } } }'"
}
```

---

## ID Formats

GitHub GraphQL uses Base64-encoded global node IDs:

| Entity | ID Format Example |
|--------|-------------------|
| Repository | `R_kgDOxxxxxx` |
| Discussion | `D_kwDOxxxxxx` |
| Discussion Category | `DIC_kwDOxxxxxx` |
| Comment | `DC_kwDOxxxxxx` |

---

## Example: Full Discussion Workflow

**Task:** Create a feature request discussion on `ethereumdegen/stark-bot`

```json
// Step 1: Get repo ID and categories
{"tool": "exec", "command": "gh api graphql -f query='query { repository(owner: \"ethereumdegen\", name: \"stark-bot\") { id discussionCategories(first: 25) { nodes { id name emoji } } } }'"}

// Step 2: Create discussion (using IDs from step 1)
{"tool": "exec", "command": "gh api graphql -f query='mutation { createDiscussion(input: { repositoryId: \"R_xxx\", categoryId: \"DIC_xxx\", title: \"Feature: Add dark mode\", body: \"## Description\\n\\nIt would be great to have a dark mode option.\\n\\n## Use Case\\n\\nBetter for nighttime use.\" }) { discussion { number url } } }'"}

// Step 3: View the created discussion
{"tool": "exec", "command": "gh api graphql -f query='query { repository(owner: \"ethereumdegen\", name: \"stark-bot\") { discussion(number: NEW_NUMBER) { title body url comments { totalCount } } } }'"}
```

---

## Escaping Special Characters

When using GraphQL via command line, escape:
- Double quotes: `\"`
- Newlines: `\\n`
- Backslashes: `\\`

Example with markdown body:
```json
{"tool": "exec", "command": "gh api graphql -f query='mutation { createDiscussion(input: { repositoryId: \"R_xxx\", categoryId: \"DIC_xxx\", title: \"Test\", body: \"## Header\\n\\n- Item 1\\n- Item 2\\n\\n**Bold text**\" }) { discussion { url } } }'"}
```

---

## Common Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `Could not resolve to a Repository` | Wrong owner/repo | Check spelling, case-sensitive |
| `Could not resolve to a node` | Invalid ID | Get fresh ID with query |
| `Resource not accessible` | Missing permissions | Check token scopes |
| `Category does not allow discussions` | Wrong category type | Use a discussion-enabled category |
| `Discussion is not answerable` | Not Q&A category | Only Q&A categories support answers |

---

## Tips

1. **Always get IDs first** - GraphQL mutations need node IDs, not numbers
2. **Use `--jq` for clean output** - Extract just what you need from JSON
3. **Check category type** - Q&A categories are "answerable", others are not
4. **Markdown is supported** - Body fields accept full GitHub-flavored markdown
5. **Pagination cursor** - Use `endCursor` from `pageInfo` for next page
