- When recommending what a git commit should be always specify a description detailed and related to the files changed, output using commitizen formatting
- After completing a set of work recommend the git commit, then afterwards update the Cargo.toml version number and create and push a git tag
- include the package version numbers in the recommended commit message
- overwrite a file called updateMessage.txt with all the latest changes use the Obi persona

## GitHub Issue Creation

When you identify bugs, feature opportunities, or have ideas during development, you can programmatically create GitHub issues using the scripts in `.github/scripts/`. These scripts support JSON input, making them ideal for automation.

### When to Create Issues

Create issues when you discover:
- **Bugs**: Problems, crashes, or unexpected behavior
- **Features**: New functionality or enhancements that would improve the project
- **Ideas**: Quick thoughts, TODOs, or future improvements
- **Technical debt**: Code that needs refactoring or improvement

### How to Create Issues Programmatically

**Bug Reports:**
```bash
echo '{
  "title": "Brief bug description",
  "description": "Detailed explanation of the bug",
  "steps": "1. Step one\n2. Step two\n3. Bug occurs",
  "expected": "What should happen",
  "actual": "What actually happens"
}' | .github/scripts/new-bug.sh --json --silent
```

**Feature Requests:**
```bash
echo '{
  "title": "Feature name",
  "summary": "Brief feature summary",
  "problem": "Problem this solves",
  "solution": "Proposed solution",
  "priority": "medium"
}' | .github/scripts/new-feature.sh --json --silent
```

**Quick Ideas:**
```bash
echo '{
  "title": "Idea title",
  "notes": "Optional additional context"
}' | .github/scripts/new-idea.sh --json --silent
```

### Examples

**Example 1: Bug found during code review**
```bash
# You notice a potential race condition
echo '{
  "title": "Potential race condition in parallel exec",
  "description": "The exec plugin may have a race condition when running commands in parallel mode",
  "steps": "Run meta exec --parallel with multiple projects simultaneously",
  "expected": "Commands execute safely in parallel",
  "actual": "Occasionally see output corruption or crashes"
}' | .github/scripts/new-bug.sh --json --silent
```

**Example 2: Feature idea during development**
```bash
# You realize a feature would be useful
echo '{
  "title": "Add dry-run mode to all commands",
  "summary": "Allow users to preview what would happen before executing",
  "problem": "Users are hesitant to run destructive commands without knowing what will happen",
  "solution": "Add --dry-run flag to all commands that shows planned actions without executing",
  "priority": "medium"
}' | .github/scripts/new-feature.sh --json --silent
```

**Example 3: Quick idea capture**
```bash
# Quick thought during implementation
echo '{
  "title": "Add progress bar for git clone operations"
}' | .github/scripts/new-idea.sh --json --silent
```

### Best Practices

1. **Use `--silent` flag**: Returns only the issue URL for clean output
2. **Create issues proactively**: Don't wait to be asked - if you spot something worth tracking, create an issue
3. **Be descriptive**: Provide enough context for others to understand and act on
4. **Set appropriate priorities**: Use "critical", "high", "medium", or "low" for features
5. **Include reproduction steps**: For bugs, always include clear steps to reproduce

### Output

All scripts return the created issue URL, which you can capture:
```bash
ISSUE_URL=$(echo '{"title":"..."}' | .github/scripts/new-idea.sh --json --silent)
echo "Created issue: $ISSUE_URL"
```

### Full Documentation

See `.github/scripts/README.md` for complete documentation including:
- All input modes (JSON, env vars, command-line args)
- Integration examples
- Error handling
- CI/CD usage patterns
- After completing a feature or major milestones generate a commitizen style commit message for the work done and wait for me to commit the changes to the git repo.

## Feature Version Maintenance

When modifying any feature module, follow these rules:

### Feature Header Requirements
Every feature module (`src/*.rs` that implements a distinct feature) must have a header comment:

```rust
//! # Feature: Feature Name
//!
//! Brief description of the feature.
//!
//! - **Version**: 1.0.0
//! - **Since**: 0.1.0
//! - **Toggleable**: true/false
//!
//! ## Changelog
//! - 1.0.0: Initial release
```

### Version Update Rules
- **Patch** (x.x.+1): Bug fixes, internal refactoring
- **Minor** (x.+1.0): New options, settings, or non-breaking enhancements
- **Major** (+1.0.0): Breaking changes, API changes, major behavior changes

### When Adding Features
1. Create the feature module with proper header comment
2. Register the feature in `src/features.rs`
3. Update `docs/feature-organization.md` implementation checklist
4. Update `README.md` if user-facing

### When Modifying Features
1. Update the feature header version
2. Add changelog entry in the header
3. Update `src/features.rs` version
4. Include version in commit message

See `docs/feature-organization.md` for complete feature organization specification.
