# Verbs

Every command is a verb of one binary, `knowledge`; the code verbs are handed to `knowledge-code`
beside it. `knowledge` with no verb prints the full usage. The outputs below are from a vault made
by `setup --from-template` (Windows; the vault shown as `~/knowledge`).

## Find and read

```
$ knowledge find "retrieval tiers"
Retrieval-Protocol — ~/knowledge/00-Meta/Retrieval-Protocol.md
  …follow a name into tier 1 or 4 | free | | 1 | `00-MOC-Root.md` → the `00-MOC-*.md` it points to → the note | "where is X…
CLAUDE — ~/knowledge/CLAUDE.md
  ``` injected notes -> 00-MOC-Root.md -> knowledge find -> knowledge graph -> raw Read (last resort) ``` Tier 0 is the notes the `UserPromptSubmit` hook injects. They are vault…
00-MOC-Root — ~/knowledge/00-Meta/00-MOC-Root.md
  **Start here.** Tier 1 of the [[Retrieval-Protocol]]. If the answer isn't reachable from this page in two hops, the vault has a gap: fix…

$ knowledge graph Hard-Rules
Hard-Rules — ~/knowledge/00-Meta/Hard-Rules.md
links to (4): 00-MOC-Root, Conventions, Identity, Retrieval-Protocol
linked from (7): 00-MOC-Root, Conventions, Hook-Lifecycle, Identity, Retrieval-Protocol, Why-This-Exists, CLAUDE
```

| Verb | What it does |
|---|---|
| `knowledge find <words> [--limit n] [--json]` | ranked notes, each with its best passage |
| `knowledge find <words> --captures \| --repo [<name>]` | what `keep` and `fetch` stored, or the checkouts' docs |
| `knowledge graph <note> [<note>] \| <note> --text` | a note's links in and out, the shortest path between two, or the note's text |
| `knowledge find <symbol> --code [--repo <name>]` | where a symbol is defined, one line each: `repo path:line kind signature` |
| `knowledge graph <symbol> [<symbol>] --code [--repo <name>]` | what uses a symbol and what it uses, or the chain of uses between two (at most 4 hops) |
| `knowledge code index [<repo>\|--all] [--rebuild]` | brings a checkout's code map level by hand: only changed files are parsed |
| `knowledge code duplicate [--staged] [<repo>]` | a new or changed definition whose body matches another's, or whose name a sibling app under `apps/` defines; warns, exits 0 |
| `knowledge keep <label> [--q <words>]…` | stores stdin and prints only the sections the queries match, 2,000 chars at most |
| `knowledge fetch <url> [--q <words>]…` | one page as Markdown, stored and answered the way `keep` answers; what it refuses: [security.md](security.md) |
| `knowledge inject <prompt>` | what the prompt hook would add to a prompt |
| `knowledge events <transcript.jsonl> [--json]` | a session's decisions, constraints, plans and refusals |
| `knowledge mcp` | an MCP server on stdio with four tools: `find`, `graph`, `read` (read-only) and `note`; `find` and `graph` take `code: true` for the code map |

## Write back

```
$ knowledge note log "tried the fixture"
added to ~/knowledge/30-Sessions/2026/11/2026-11-24.md

$ knowledge note decision "keep the vault private" --question "public vault?"
added to ~/knowledge/50-Ops/decisions.jsonl

$ knowledge ledger list
manual:1795517391312
  Q: public vault?
  A: keep the vault private

1 unpromoted
```

| Verb | What it does |
|---|---|
| `knowledge note log <text>` | one bullet in today's session note (the day rolls over at `rolloverHour`), indexed at once |
| `knowledge note decision <text> [--question <q>] [--promoted <path>]` | one line in the decision ledger: the write-back for a harness with no hooks |
| `knowledge ledger list` | the captured decisions not yet promoted to a note |
| `knowledge ledger promote <src> <path\|not-a-decision>` | marks one promoted, to the note that records it, or as not a decision |

## Health

```
$ knowledge doctor --only links,pointers --pretty
knowledge doctor  PASS  2026-11-24T10:49:51.520Z

  PASS  links
          broken: 0
          allowedForward: 0
  PASS  pointers
          pointers: 0
          dead: 0
```

| Verb | What it does |
|---|---|
| `knowledge doctor [--pretty] [--repair] [--record] [--only <id,…>]` | retrieval health, zero tokens: the index, the vault's own questions, links, pointers, priming, rules, sync; `--repair` rebuilds what is derived |
| `knowledge doctor --only portable [--json]` | names and paths that break on the other OS |
| `knowledge doctor budget [<transcript>] [--json]` | what a session's start cost, by source, and listings unused in 30 days |
| `knowledge doctor profile <note>` | the per-project settings that leave unlisted connectors off |
| `knowledge index [--rebuild] [<note>…]` | brings the index level with the vault: only changed notes are read |
| `knowledge index --check [--json]` | notes on disk against rows: ghosts, missing, stale |
| `knowledge index --repo <checkout>` | a checkout's `docs/`, `CLAUDE.md` and `README.md` into the repo-docs index |

## Rules

| Verb | What it does |
|---|---|
| `knowledge rules list [--json] \| kernel` | the packs, rules and guards in the lock; the rules that are code, not text |
| `knowledge rules check [--staged] [--restore]` | Hard-Rules against the lock; the staged copy and the guards' backstops; a changed rule back from `HEAD` |
| `knowledge rules lock [--seed <file>]` | re-stamps the lock after you edit Hard-Rules |
| `knowledge rules enable\|disable <pack>` | turns a pack's guards on or off |

## Machine and session

| Verb | What it does |
|---|---|
| `knowledge setup --plan \| --apply <id> \| --remove --plan` | this machine's setup across every harness found, planned before anything is written ([install.md](install.md), [remove.md](remove.md)) |
| `knowledge update --plan` | the latest release and this platform's steps; downloads nothing ([update.md](update.md)) |
| `knowledge config [<key>]` | the paths and settings the hooks see; a folder key prints the name in effect |
| `knowledge hook session\|prompt\|pre-tool\|post-edit\|post-bash\|capture --harness <id> [--pin <v>]` | the hooks themselves: the harness's JSON on stdin, always exit 0; with `--pin`, a binary of another version stands down |
| `knowledge flush <session-id>` | a session's new turns into the daily log, once (a headless run of the harness's model) |
| `knowledge compile [--force] [--dry-run]` | a finished daily log into notes, once |
| `knowledge records` | commits the records the hooks wrote, and pushes when every unpushed commit is a record |
| `knowledge hubs` | regenerates the hub MOCs |
| `knowledge usage [--month <YYYY-MM>] [--days N]` | the monthly token, model and cost digest |
| `knowledge post-bash [--backfill <since>]` | one note per commit, for commits made outside a session |
| `knowledge --version` | the version, and whether SQLite's FTS5 is on |
