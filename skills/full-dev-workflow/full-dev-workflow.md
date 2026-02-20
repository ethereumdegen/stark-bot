---
name: full-dev-workflow
description: Complete software development workflow from idea to deployment. Covers planning, coding, testing, committing, and deploying to GitHub.
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"⚡"}}
tags: [development, git, github, code, workflow, deployment]
requires_tools: [git, committer, deploy, exec, pr_quality, read_file, write_file, edit_file, glob, grep]
---

# Full Development Workflow

Complete workflow for developing software from idea to deployed code.

## Overview

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   PLAN      │ -> │   BUILD     │ -> │   TEST      │ -> │   DEPLOY    │
│             │    │             │    │             │    │             │
│ - Understand│    │ - Create    │    │ - Run tests │    │ - Commit    │
│ - Explore   │    │ - Edit      │    │ - Fix bugs  │    │ - Push      │
│ - Design    │    │ - Refactor  │    │ - Quality   │    │ - Create PR │
└─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘
```

---

## Phase 1: PLAN

### 1.1 Understand the Request

Parse what the user wants:
- Feature type (new app, feature, bugfix, refactor)
- Technology stack (React, Rust, Python, etc.)
- Specific requirements

### 1.2 Explore Existing Code (If Applicable)

**Find relevant files:**
```tool:glob
pattern: "**/*.{ts,tsx,js,jsx}"
```

**Search for related code:**
```tool:grep
pattern: "function.*Todo"
glob: "*.ts"
output_mode: content
context: 5
```

**Read key files:**
```tool:read_file
path: src/App.tsx
```

### 1.3 Create Implementation Plan

Document:
- Files to create/modify
- Dependencies needed
- Implementation steps
- Testing strategy

---

## Phase 2: BUILD

### 2.1 Set Up Project (If New)

**For new projects, use scaffolding:**
```tool:exec
command: npx create-next-app@latest my-app --typescript --tailwind
timeout: 180000
```

Or manual setup:
```tool:write_file
path: my-app/package.json
content: |
  {
    "name": "my-app",
    "version": "1.0.0",
    "scripts": {
      "dev": "next dev",
      "build": "next build",
      "test": "jest"
    }
  }
```

### 2.2 Create/Edit Code Files

**Create new file:**
```tool:write_file
path: src/components/TodoList.tsx
content: |
  import React, { useState } from 'react';

  interface Todo {
    id: string;
    title: string;
    completed: boolean;
  }

  export function TodoList() {
    const [todos, setTodos] = useState<Todo[]>([]);
    const [input, setInput] = useState('');

    const addTodo = () => {
      if (!input.trim()) return;
      setTodos([...todos, {
        id: crypto.randomUUID(),
        title: input,
        completed: false
      }]);
      setInput('');
    };

    const toggleTodo = (id: string) => {
      setTodos(todos.map(todo =>
        todo.id === id ? {...todo, completed: !todo.completed} : todo
      ));
    };

    return (
      <div className="p-4 max-w-md mx-auto">
        <h1 className="text-2xl font-bold mb-4">Todo List</h1>
        <div className="flex gap-2 mb-4">
          <input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            className="flex-1 border rounded px-2 py-1"
            placeholder="Add a todo..."
          />
          <button onClick={addTodo} className="bg-blue-500 text-white px-4 py-1 rounded">
            Add
          </button>
        </div>
        <ul className="space-y-2">
          {todos.map(todo => (
            <li
              key={todo.id}
              onClick={() => toggleTodo(todo.id)}
              className={`cursor-pointer p-2 border rounded ${todo.completed ? 'line-through text-gray-500' : ''}`}
            >
              {todo.title}
            </li>
          ))}
        </ul>
      </div>
    );
  }
```

**Edit existing file:**
```tool:edit_file
path: src/App.tsx
old_string: export default function Home() {
new_string: import { TodoList } from './components/TodoList';

export default function Home() {
```

### 2.3 Install Dependencies

```tool:exec
command: npm install uuid
timeout: 60000
```

---

## Phase 3: TEST

### 3.1 Run Linter/Type Check

```tool:exec
command: npm run lint
timeout: 60000
```

```tool:exec
command: npx tsc --noEmit
timeout: 60000
```

### 3.2 Run Tests

```tool:exec
command: npm test
timeout: 120000
```

### 3.3 Manual Verification

```tool:exec
command: npm run dev
timeout: 10000
```

Check that the app starts without errors.

### 3.4 Debug Any Issues

If there are errors, use the debug skill:
- Read error messages
- Find the source
- Fix the issue
- Re-test

---

## Phase 4: DEPLOY

### 4.1 Quality Check

**Run full quality check:**
```tool:pr_quality
operation: full_check
base_branch: main
```

**Check for debug code:**
```tool:pr_quality
operation: debug_scan
```

### 4.2 Review Changes

```tool:git
operation: status
```

```tool:git
operation: diff
```

### 4.3 Safe Commit

**ALWAYS use committer for safety:**
```tool:committer
message: "feat(todo): add TodoList component with add/toggle functionality"
files: [
  "src/components/TodoList.tsx",
  "src/App.tsx",
  "package.json"
]
```

### 4.4 Push to GitHub

```tool:deploy
operation: push
set_upstream: true
```

### 4.5 Create Pull Request

```tool:deploy
operation: create_pr
title: "feat(todo): Add TodoList component"
body: |
  ## Summary
  Adds a fully functional TodoList component with:
  - Add new todos
  - Toggle completion status
  - Styled with Tailwind CSS

  ## Changes
  - `src/components/TodoList.tsx` - New component
  - `src/App.tsx` - Import and use TodoList
  - `package.json` - Added uuid dependency

  ## Test Plan
  - [x] Component renders correctly
  - [x] Can add new todos
  - [x] Can toggle todos
  - [x] TypeScript compiles without errors
  - [x] Linting passes

  ## Screenshots
  N/A (can be tested locally)
base_branch: main
```

### 4.6 Monitor CI

```tool:deploy
operation: workflow_status
```

### 4.7 Merge When Ready

```tool:deploy
operation: merge_pr
pr_number: 123
```

---

## Quick Reference: Tool Cheat Sheet

| Task | Tool | Example |
|------|------|---------|
| GitHub username | `github_user` | `{}` |
| Read file | `read_file` | `{"path": "src/App.tsx"}` |
| Write file | `write_file` | `{"path": "src/new.ts", "content": "..."}` |
| Edit file | `edit_file` | `{"path": "src/App.tsx", "old_string": "...", "new_string": "..."}` |
| Find files | `glob` | `{"pattern": "**/*.tsx"}` |
| Search code | `grep` | `{"pattern": "function", "glob": "*.ts"}` |
| Run command | `exec` | `{"command": "npm install", "timeout": 60000}` |
| Git status | `git` | `{"operation": "status"}` |
| Git diff | `git` | `{"operation": "diff"}` |
| Safe commit | `committer` | `{"message": "feat: add feature", "files": ["src/x.ts"]}` |
| Quality check | `pr_quality` | `{"operation": "full_check"}` |
| Push code | `deploy` | `{"operation": "push"}` |
| Create PR | `deploy` | `{"operation": "create_pr", "title": "...", "body": "..."}` |
| PR status | `deploy` | `{"operation": "pr_status", "pr_number": 123}` |
| Merge PR | `deploy` | `{"operation": "merge_pr", "pr_number": 123}` |

---

## Common Patterns

### Creating a New Feature

1. `glob` - Find related files
2. `read_file` - Understand existing code
3. `write_file` - Create new files
4. `edit_file` - Modify existing files
5. `exec` - Install deps, run tests
6. `pr_quality` - Check for issues
7. `committer` - Safe commit
8. `deploy` - Push and create PR

### Fixing a Bug

1. `grep` - Find where the bug is
2. `read_file` - Read the problematic code
3. `edit_file` - Fix the issue
4. `exec` - Run tests
5. `committer` - Commit the fix
6. `deploy` - Push and create PR

### Refactoring

1. `glob` - Find all files to refactor
2. `grep` - Find all usages
3. `edit_file` - Apply changes (multiple files)
4. `exec` - Run tests to verify no breakage
5. `pr_quality` - Ensure quality
6. `committer` - Commit refactor
7. `deploy` - Push and create PR

---

## Safety Reminders

1. **Never commit secrets** - committer tool will block them
2. **Always run quality checks** before creating PRs
3. **Use feature branches** - never commit to main directly
4. **Write meaningful commit messages** - follow conventional commits
5. **Test before deploying** - run tests and linter
6. **Monitor CI** after pushing
