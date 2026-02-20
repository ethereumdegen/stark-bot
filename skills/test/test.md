---
name: test
description: Run tests and analyze failures. Detects test framework, executes tests, and helps debug failing tests.
version: 1.0.0
author: starkbot
metadata: {"clawdbot":{"emoji":"🧪"}}
tags: [development, testing, debugging, code]
requires_tools: [exec, read_file, glob, grep]
---

# Test Runner Skill

Run tests, analyze results, and debug failures.

## Framework Detection

First, detect the test framework by checking for config files:

### Rust (Cargo)
```tool:glob
pattern: "**/Cargo.toml"
```

### Node.js (npm/yarn)
```tool:glob
pattern: "**/package.json"
```

### Python
```tool:glob
pattern: "**/pytest.ini"
```
or
```tool:glob
pattern: "**/setup.py"
```

## Running Tests

### Rust
```tool:exec
command: cargo test
timeout: 120000
```

With specific test:
```tool:exec
command: cargo test test_name
timeout: 120000
```

### Node.js
```tool:exec
command: npm test
timeout: 120000
```

Or with pattern:
```tool:exec
command: npm test -- --grep 'pattern'
timeout: 120000
```

### Python (pytest)
```tool:exec
command: pytest -v
timeout: 120000
```

Specific test:
```tool:exec
command: pytest tests/test_file.py::test_name -v
timeout: 120000
```

## Analyzing Failures

When tests fail:

### 1. Identify the failing test
Extract from output:
- Test name
- File location
- Line number

### 2. Read the test file
```tool:read_file
path: tests/test_file.rs
```

### 3. Read the code under test
```tool:read_file
path: src/module.rs
```

### 4. Search for related code
```tool:grep
pattern: function_name
glob: "*.rs"
output_mode: content
context: 5
```

## Common Failure Patterns

| Pattern | Likely Cause |
|---------|--------------|
| `assertion failed` | Logic error in code |
| `panic` | Unhandled error case |
| `timeout` | Infinite loop or slow operation |
| `connection refused` | Missing test dependency |
| `not found` | Missing file or import |

## Test Output Format

Report results clearly:

```markdown
## Test Results

**Status:** ✅ PASSED / ❌ FAILED

### Summary
- Total: X tests
- Passed: Y
- Failed: Z
- Skipped: W

### Failed Tests

1. `test_name`
   - File: `tests/test_file.rs:42`
   - Error: assertion failed
   - Expected: X
   - Got: Y
   - Likely cause: [analysis]
```

## Debugging Tips

1. **Run single test** - Isolate the failure
2. **Add verbose output** - Use `-v` or `--verbose` flags
3. **Check test setup** - Look for missing fixtures/mocks
4. **Compare with working tests** - Find what's different
5. **Check test data** - Ensure inputs are correct

## Tools Used

| Tool | Purpose |
|------|---------|
| `exec` | Run test commands |
| `read_file` | Read test files and source code |
| `grep` | Search for related code |
| `glob` | Find test files |
| `list_files` | List test directory contents |
