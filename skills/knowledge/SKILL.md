---
name: knowledge
description: Use when creating or editing anything in the knowledge vault - a new folder or structure, a repo/project page, a documentation note, a slash command, a commit, a task or todo, a hook, a plan, or a decision (ADR), including promoting captured decisions from the ledger. Also use when asked to record a decision, log a session, or write something into the vault. Triggers on "/knowledge new", "/knowledge edit", "add this to the knowledge vault", "record this decision", "document this".
---

# knowledge

Router for writing to the knowledge vault. Its path is `vault` in the machine's config:
`knowledge config vault`.

```
/knowledge new  <kind> [name]
/knowledge edit <kind> [name]
```

## Before writing anything

1. Read `00-Meta/Conventions.md` — frontmatter schema, naming, where things live.
2. Read `00-Meta/Hard-Rules.md` if the change touches code, deps, components, or licensing.
3. Check the target doesn't already exist: `knowledge find "<q>"`.

**Never duplicate a repo's own docs into the vault.** A repo's `docs/` *is* its documentation;
vault pages link inward.

## Kinds

Load **only** the reference file for the kind being invoked. Do not read the others.

| kind | reference |
|---|---|
| `structure` | `references/structure.md` |
| `repo` | `references/repo.md` |
| `docs` | `references/docs.md` |
| `command` | `references/command.md` |
| `commit` | `references/commit.md` |
| `task` | `references/task.md` |
| `hook` | `references/hook.md` |
| `plan` | `references/plan.md` |
| `decision` | `references/decision.md` — also `/knowledge new decision --ledger` |

If the kind is ambiguous, ask — do not guess between `docs` and `repo`.

## Always, for every kind

- Frontmatter on every note. `type` and `created` required; `updated` on every edit.
- Wikilinks `[[name]]` for cross-references. A markdown path link is invisible to the graph.
- Link the new note from its folder MOC, or it is unreachable at retrieval tier 1.
- After a batch of changes, `knowledge index --check`: the post-edit hook reindexes each note, and this confirms the index `knowledge find` and `knowledge graph` read is level.
- Commit the way `00-Meta/Hard-Rules.md` says to branch and commit.

## A new or changed Hard-Rule

`00-Meta/Hard-Rules.md`, `Identity.md` and `rules.lock` change only on the owner's yes: show the
edit as one diff first. After it, `knowledge rules lock` re-stamps the lock. Then, when the rule
names something a machine can see (a command, a path an edit or a shell writer touches, an MCP
tool, a file name a commit stages), propose the guard that enforces it: the
`knowledge rules guard add --rule <id> --action ask|deny --reason "…" --words "…" | --paths … |
--tools … | --backstop …` line, `ask` unless the owner wants it refused outright, and run it on a
yes. `knowledge rules list` prints the rule ids. Guards are add-only: none is removed or weakened
by a verb.
