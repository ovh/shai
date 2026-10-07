# Skills

Skills are comable, on-demand procedural instructions that extend shai's capabilities. They provide a token-efficient way to give shai domain-specific knowledge and workflows without bloating the system prompt.

## How It Works

1. **Discovery** — Shai scans skill directories at startup and loads each skill's metadata (name + description) into a catalog.
2. **Catalog Injection** — The catalog is injected into the system prompt so the model knows which skills are available.
3. **On-Demand Loading** — When the model decides a skill is relevant, it calls the `skill` tool with the skill name to load the full `SKILL.md` body.

This progressive disclosure pattern keeps the system prompt lean while making detailed instructions available when needed.

## Directory Structure

Skills are discovered from three locations:

| Location | Scope | Priority |
|----------|-------|----------|
| `.shai/skills/` | Project-local | Highest |
| `~/.config/shai/skills/` | Global (user-wide) | Middle |
| Built-in (embedded in the binary) | Ships with shai | Lowest |

A skill in a higher-priority location shadows a same-named skill in a lower one. The built-in skills are compiled into every shai binary, so they work even on a fresh install with no skill directories present.

To override a built-in skill (e.g. to replace the `memory` protocol with your own), create a skill with the same name in `.shai/skills/` or `~/.config/shai/skills/` — yours takes precedence and the built-in copy is ignored.

`shai list skills` (and the `/skills` command in the TUI) shows where each skill comes from — `(project)`, `(global)`, or `(built-in)` — so you can see at a glance which skills are overriding others.

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

Only generic, project-agnostic skills are embedded in the shai binary:

### memory
Save, organize, and curate persistent memories across sessions. Defines the protocol for the `MEMORY.md` index and per-topic detail files that `memory_write`/`memory_remove` manage.

## Sample Skills

Shai/Rust-specific sample skills live in `shai-core/examples/skills/` in the repository. They are **not** bundled — copy any of them into `.shai/skills/` or `~/.config/shai/skills/` to use them:

- **code-review** — Review uncommitted or branch-diff changes for bugs, security issues, style violations, and potential improvements.
- **git-workflow** — Manage git branches, commits, rebases, and pull requests following project conventions.
- **testing** — Write and run tests, ensure adequate test coverage, and fix failing tests.
- **debugging** — Systematically diagnose and fix bugs using logs, stack traces, and targeted experiments.
- **release** — Prepare and publish a new release of the SHAI project, including version bumps and tagging.
- **refactoring** — Restructure existing code to improve readability, maintainability, and performance without changing behavior.
- **repo-analysis** — Analyze repository structure and produce an overview.
- **rust-lint** — Run and fix Rust linting (clippy/fmt) issues.

```bash
cp -r shai-core/examples/skills/code-review ~/.config/shai/skills/
```

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
