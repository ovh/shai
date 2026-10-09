---
name: memory
description: Save, organize, and curate persistent memories (MEMORY.md index plus topic files) across sessions. Load before writing detailed notes, topic files, or when asked to remember context that should outlive this session.
---

# Memory

Shai keeps persistent memory as plain markdown you manage with the standard
file tools plus two index tools. Memories are loaded into every future session,
so they must stay small, durable, and curated.

## Layout

Two scopes exist, each with the same shape:

```
~/.config/shai/memory/        (global — personal, cross-project)
├── MEMORY.md                 # index: one line per memory
└── <topic>.md                # detail files, one topic each

.shai/memory/                 (project — knowledge shared for this repo)
├── MEMORY.md
└── <topic>.md
```

- The **index** (`MEMORY.md`) is the only thing auto-injected into the prompt.
  One bullet per memory: `- [YYYY-MM-DD HH:MM:SS] short fact`, optionally
  ending with a pointer like `(see testing.md)`.
- **Topic files** hold detail. They are never auto-loaded; read them with the
  `read` tool when the index line is relevant.
- Project scope = facts about this repo (conventions, decisions, architecture
  notes that are not derivable from the code). Global scope = facts about the
  user (preferences, working style) that apply everywhere.

## What to save

Save when the information would be useful in a *future* session and cannot be
derived from the codebase, git history, or AGENTS.md:

- Corrections the user gives you ("no, we use pnpm here", "never force-push")
- Confirmed preferences (answer style, workflow choices)
- Decisions and their rationale ("chose X over Y because Z")
- External references (issue tracker, dashboards, deploy targets)
- Ongoing work context that is not in git yet

Do **not** save: anything readable from the code or AGENTS.md, file paths or
architecture the repo already shows, one-off debugging fixes, or session task
state (that belongs in the todo list).

## How to save

1. **One-line fact** → `memory_write` (add `scope` only for global facts).
   Keep it under ~120 characters. Deduplicate first: if the index already says
   it, do not write it again.
2. **Detailed note** → create or update a topic file with the `write`/`edit`
   tools. Name it after the topic (`testing.md`, `deploy.md`,
   `user_preferences.md`). Then add or update a single index line pointing at
   it: `memory_write {"content": "Testing conventions and commands (see testing.md)"}`.
3. Never write multi-paragraph content through `memory_write` — it rejects
   newlines by design.

## Curation

The index is capped (200 lines / 25KB per scope). If `memory_write` reports the
cap, or the injected index says it is truncated, curate before adding more:

- Remove stale or superseded facts with `memory_remove`.
- Merge near-duplicates: `memory_remove` the variants, `memory_write` one
  consolidated line.
- Move growing detail out of index lines into topic files, leaving a pointer.
- Topic files can be rewritten freely with `edit` — keep each focused on one
  topic and short enough to read on demand.

Prefer updating an existing memory over stacking a new one that contradicts it.

## Examples

User says "we deploy with the cds pipeline, not kubectl":

```json
{"name": "memory_write"}
{"content": "Deployment goes through CDS pipelines, not kubectl (see deploy.md)"}
```

then write the pipeline names/links into `.shai/memory/deploy.md`.

User says "stop adding emojis to commit messages" (personal preference):

```json
{"name": "memory_write"}
{"content": "No emojis in commit messages or answers", "scope": "global"}
```

An index line is now wrong:

```json
{"name": "memory_remove"}
{"content": "Deployment goes through CDS pipelines"}
```
