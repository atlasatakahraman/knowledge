---
type: rules
project: knowledge
tags: [retrieval, context]
created: 2026-11-02
updated: 2026-11-24
status: active
---

# Retrieval Protocol

How to find something in this vault **without** reading files. Follow the tiers in order. The point
of the vault is to cut context, and a vault you grep is just a folder.

## The tiers

| # | Tier | Use for | Cost |
|---|---|---|---|
| 0 | The notes the prompt hook adds to each prompt | Breadcrumbs: follow a name into tier 1 or 4 | free |
| 1 | `00-MOC-Root.md` → the `00-MOC-*.md` it points to → the note | "where is X". Most questions end here | ~free |
| 2 | `knowledge find "<q>"` | "what did I write about X" | very low |
| 3 | `knowledge graph <note> [<note>]` | "how does X connect to Y" | very low |
| 4 | `Read` of **one named file** | Only when 1–3 failed | unbounded |

**Stop-check:** about to `Read`, `ls`, `find` or `grep` a vault path without having run tiers 1–3?
That is the violation. Go back to the MOC. If tier 4 was needed, the vault has a gap: fix it.

### 0. Injected notes

The `knowledge` binary searches the vault index on every prompt and adds up to six matching vault
notes, each with a snippet, and the best passage whole when it is short. A snippet is not the note.

### 1. The MOCs

[[00-MOC-Root]] links every folder's `00-MOC-*.md`, and every note is at most two hops from it
([[Conventions]] § Reach). Cheap, current, and it says where things live.

### 2. `knowledge find`: full-text search

The vault index (SQLite FTS5, per machine), from any folder. The question's rarest words rank the
notes; each comes with its best passage. `--json` for scripts, `--limit n` for more than six.

```bash
knowledge find "how does a request reach the cache"
```

`knowledge find "<q>" --repo [<name>]` searches the checkouts' docs, and `--captures` what was kept
or fetched in the last 14 days. Neither is a vault tier.

### 3. `knowledge graph`: relationships

When the question is how notes **connect**, not what they are called: a note's links in and out,
or the shortest path between two, read from the links the index holds.

```bash
knowledge graph Conventions                 # links to, linked from
knowledge graph Conventions Hard-Rules      # the path, up to four links
```

The code map answers the same questions for the checkouts' code: where a symbol is defined, what
uses it, and how two connect. One SQLite file per checkout in the knowledge home, built from each
grammar's own tags query and brought level with git's tracked files before every answer.

```bash
knowledge find <symbol> --code [--repo <name>]   # the definitions, with signatures
knowledge graph <symbol> --code                  # used by, and uses
knowledge graph <symbol> <other> --code          # the path between two
```

### 4. `Read`

Once a tier has **named** the file, `Read` is right: most notes are a few KB. Never a directory
sweep. When a large output will be filtered rather than edited, `<cmd> | knowledge keep "<label>"
--q "<q>"` prints only the matching sections.

## Keeping it honest

An edit reindexes its note at once, and the doctor's `vault-index` check compares the index
with the disk. A write outside Claude (Obsidian, a shell) is picked up by `knowledge index`, which
the daily sweep runs.

| Command | Covers | Cost |
|---|---|---|
| `knowledge index` | the vault index: only changed notes are read | **free**, ~1 s for a rebuild |
| `knowledge code index --all` | every checkout's code map: only changed files are parsed | **free** |

**The code map reads git's tracked files only**: a file git ignores or has not been told about is
not in it.

## Related

[[Identity]] · [[Hard-Rules]] · [[Conventions]] · [[00-MOC-Root]]
