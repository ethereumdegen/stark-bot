---
name: create-project
description: Create a new software project from scratch. Scaffolds project structure, sets up dependencies, and initializes git repository.
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🚀"}}
tags: [development, git, code, project, scaffold]
requires_tools: [git, committer, deploy, exec, github_user, pr_quality, write_file]
---

# Create Project Skill

Create new software projects with proper structure, dependencies, and git initialization.

## Workflow Overview

```
1. Determine project type and requirements
2. Create project directory
3. Initialize with framework scaffolding (or manual setup)
4. Set up dependencies
5. Create initial code files
6. Initialize git repository
7. Make initial commit
8. (Optional) Push to GitHub
```

## Step 1: Gather Requirements

Ask the user (if not specified):
- What type of project? (web app, CLI tool, API, library)
- What language/framework? (React, Next.js, Rust, Python, Node.js)
- Any specific features needed?

## Step 2: Create Project Structure

### For Node.js / React / Next.js

**Option A: Use Create Tool (Recommended)**
```tool:exec
command: npx create-next-app@latest my-todo-app --typescript --tailwind --eslint
timeout: 120000
```

**Option B: Manual Setup**
```tool:write_file
path: my-project/package.json
content: |
  {
    "name": "my-project",
    "version": "1.0.0",
    "scripts": {
      "dev": "node index.js",
      "test": "jest"
    },
    "dependencies": {},
    "devDependencies": {}
  }
```

### For Rust

```tool:exec
command: cargo new my-project
timeout: 30000
```

### For Python

```tool:exec
command: mkdir -p my-project && cd my-project && python -m venv venv
timeout: 30000
```

Then create requirements.txt:
```tool:write_file
path: my-project/requirements.txt
content: |
  flask>=2.0
  pytest>=7.0
```

## Step 3: Create Project Files

### Example: Todo App Structure

```tool:write_file
path: todo-app/src/index.ts
content: |
  // Todo App Entry Point
  import { TodoList } from './components/TodoList';

  export function main() {
    const todoList = new TodoList();
    todoList.render();
  }

  main();
```

```tool:write_file
path: todo-app/src/types.ts
content: |
  export interface Todo {
    id: string;
    title: string;
    completed: boolean;
    createdAt: Date;
  }
```

```tool:write_file
path: todo-app/src/components/TodoList.ts
content: |
  import { Todo } from '../types';

  export class TodoList {
    private todos: Todo[] = [];

    add(title: string): Todo {
      const todo: Todo = {
        id: crypto.randomUUID(),
        title,
        completed: false,
        createdAt: new Date()
      };
      this.todos.push(todo);
      return todo;
    }

    toggle(id: string): void {
      const todo = this.todos.find(t => t.id === id);
      if (todo) {
        todo.completed = !todo.completed;
      }
    }

    remove(id: string): void {
      this.todos = this.todos.filter(t => t.id !== id);
    }

    list(): Todo[] {
      return [...this.todos];
    }

    render(): void {
      console.log('Todos:', this.todos);
    }
  }
```

## Step 4: Install Dependencies

```tool:exec
command: cd my-project && npm install
timeout: 120000
```

## Step 5: Create README

```tool:write_file
path: my-project/README.md
content: |
  # My Project

  ## Description
  A brief description of what this project does.

  ## Installation
  ```bash
  npm install
  ```

  ## Usage
  ```bash
  npm run dev
  ```

  ## Development
  ```bash
  npm run test
  ```
```

## Step 6: Initialize Git Repository

```tool:git
operation: status
```

If not a git repo:
```tool:exec
command: cd my-project && git init
timeout: 10000
```

## Step 7: Create Initial Commit

**Run quality check first:**
```tool:pr_quality
operation: debug_scan
```

**Use safe commit:**
```tool:committer
message: "feat: initial project setup"
files: ["package.json", "src/index.ts", "src/types.ts", "README.md"]
```

## Step 8: Push to GitHub (Optional)

**First, get your GitHub username:**
```tool:github_user
```

**Create repository on GitHub (using your username):**
```tool:exec
command: gh repo create <username>/my-project --public --source=. --push
timeout: 30000
```

Replace `<username>` with the result from `github_user` tool.

**Or just push to existing remote:**
```tool:deploy
operation: push
set_upstream: true
```

---

## Common Project Templates

### React + TypeScript + Tailwind
```bash
npx create-next-app@latest my-app --typescript --tailwind --eslint --app
```

### Express.js API
```bash
mkdir api && cd api && npm init -y && npm install express cors dotenv
```

### Rust CLI Tool
```bash
cargo new my-cli
# Then add clap to Cargo.toml
```

### Python Flask API
```bash
mkdir api && cd api && python -m venv venv && pip install flask
```

---

## Best Practices

1. **Always create a README** - Explain what the project does
2. **Add .gitignore** - Exclude node_modules, .env, build artifacts
3. **Set up linting** - ESLint, Prettier, rustfmt, black
4. **Add basic tests** - At least one test to start
5. **Use environment variables** - Never hardcode secrets
6. **Initialize git early** - Track changes from the start

---

## Tools Used

| Tool | Purpose |
|------|---------|
| `exec` | Run project scaffolding commands |
| `write_file` | Create project files |
| `git` | Initialize repository |
| `github_user` | Get authenticated GitHub username |
| `committer` | Safe initial commit |
| `deploy` | Push to GitHub |
| `pr_quality` | Check for issues before commit |
