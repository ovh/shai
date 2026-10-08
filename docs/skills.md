# Skills

Skills are comable, on-demand procedural instructions that extend shai's capabilities. They provide a token-efficient way to give shai domain-specific knowledge and workflows without bloating the system prompt.

## How It Works

1. **Discovery** — Shai scans skill directories at startup and loads each skill's metadata (name + description) into a catalog.
2. **Catalog Injection** — The catalog is injected into the system prompt so the model knows which skills are available.
3. **On-Demand Loading** — When the model decides a skill is relevant, it calls the `skill` tool with the skill name to load the full `SKILL.md` body.

This progressive disclosure pattern keeps the system prompt lean while making detailed instructions available when needed.

## Directory Structure

Skills are discovered from two locations:

| Location | Scope | Priority |
|----------|-------|----------|
| `.shai/skills/` | Project-local | Higher |
| `~/.config/shai/skills/` | Global (user-wide) | Lower |

Project-local skills take precedence — if a project-local and global skill share the same name, the project-local one shadows the global.

## Creating a Skill

Each skill lives in its own directory containing a `SKILL.md` file:

```
.shai/skills/
├── code-review/
│   └── SKILL.md
├── testing/
│   └── SKILL.md
└── debugging/
    └── SKILL.md
```

### SKILL.md Format

A `SKILL.md` file consists of YAML frontmatter followed by markdown content:

```markdown
---
name: my-skill
description: A short description of what this skill does
---

# My Skill

## Purpose
Detailed instructions for the model to follow when this skill is loaded.

## Procedure
1. Step one...
2. Step two...
```

#### Frontmatter Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Unique identifier for the skill (lowercase kebab-case recommended). |
| `description` | Yes | Short summary shown in the skill catalog. Should help the model decide when to use the skill. |

#### Body

The body is standard markdown and can contain any instructions, procedures, code examples, or guidelines you want the model to follow when the skill is loaded. The frontmatter is stripped before the body is returned to the model.

## Using Skills

### From the Interactive Mode

When chatting with shai, the model will automatically detect when a skill is relevant based on the catalog in the system prompt and load it using the `skill` tool.

### From Headless Mode

```bash
echo "Review my code changes" | shai
```

The model will load the `code-review` skill if it's available and relevant.

## Bundled Skills

The following skills ship with shai:

### code-review
Review uncommitted or branch-diff changes for bugs, security issues, style violations, and potential improvements.

### git-workflow
Manage git branches, commits, rebases, and pull requests following project conventions.

### testing
Write and run tests, ensure adequate test coverage, and fix failing tests.

### debugging
Systematically diagnose and fix bugs using logs, stack traces, and targeted experiments.

### release
Prepare and publish a new release of the SHAI project, including version bumps and tagging.

### refactoring
Restructure existing code to improve readability, maintainability, and performance without changing behavior.

## Skills vs MCP vs AGENTS.md

| Feature | Skills | MCP | AGENTS.md |
|---------|--------|-----|------------|
| **Purpose** | Procedural instructions | External tool integration | Project context |
| **Loaded** | On-demand | Always available | Always loaded |
| **Token cost** | Low (catalog only) | Medium (tool schemas) | Medium (full content) |
| **Example** | "How to do a code review" | "Call an external API" | "This project uses X architecture" |

## Writing Good Skills

### Do
- **Be specific** — Reference exact commands, file paths, and patterns relevant to your project.
- **Keep it actionable** — Skills should guide the model through a clear procedure.
- **Use examples** — Include code snippets and command examples.
- **Set boundaries** — Clearly state what the skill should and shouldn't do.

### Don't
- **Don't duplicate AGENTS.md** — Skills are for procedures, not static project context.
- **Don't make skills too large** — If a skill is very long, consider splitting it into multiple smaller skills.
- **Don't state the obvious** — The model already knows how to program; focus on project-specific conventions and workflows.

## Example: Custom Skill

```bash
mkdir -p .shai/skills/deploy
cat > .shai/skills/deploy/SKILL.md << 'EOF'
---
name: deploy
description: Deploy the application to staging or production environments
---

# Deploy

## Staging
Run `make deploy-staging` and verify the deployment at https://staging.example.com/health.

## Production
1. Confirm the branch is `main` and tests pass.
2. Run `make deploy-production`.
3. Check the health endpoint and monitoring dashboard.
EOF
```
