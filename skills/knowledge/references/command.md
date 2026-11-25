# knowledge: command

A slash command: a skill in `~/.claude/skills/<name>/SKILL.md` (user scope, works in every
project) that only you invoke. Since 0.13.1 the engine's own commands are skills of this kind: a
skill is a folder, linked as a junction on Windows, where a `~/.claude/commands/<name>.md` file
link needs Developer Mode.

## Slash-only or model-invoked?

| | Frontmatter | Context cost |
|---|---|---|
| You always type it, deterministic steps | `disable-model-invocation: true` | none until you type it |
| The agent should invoke it on intent | (default) | its description loads into every session |

Default to **slash-only**. A model-invoked skill is a standing tax; this vault deliberately has one
router skill rather than sixteen.

## new

```markdown
---
name: knowledge-<what>
disable-model-invocation: true
description: One line. It is what the / menu shows.
---

Steps, imperative. Reference absolute paths - the skill runs from any cwd.
```

- Name it for what it does: `knowledge-usage`, not `usage`. The folder name is the `/` name.
- `$ARGUMENTS` interpolates what the user typed after the name.
- Keep it short. One that needs 200 lines of instruction wants `references/` beside it.

## edit

On a slash-only skill, `description:` is what the `/` menu shows. On a model-invoked one it decides
when the agent reaches for it, the highest-leverage line in the file: test by asking for the thing
in your own words and seeing whether it fires.
