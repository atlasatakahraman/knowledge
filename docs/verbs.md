# Verbs

Every command is a verb of one binary, `knowledge`; the code verbs are handed to `knowledge-code`
beside it. `knowledge` with no verb prints the full usage. `knowledge <verb> --help` prints one
verb's usage, when to use it and why, and runs nothing: `--help` or `-h` anywhere in the arguments
stops the verb. The outputs below are from a vault made by `setup --from-template` (Windows; the
vault shown as `~/knowledge`).

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

| Verb | What it does | When, and why |
|---|---|---|
| `knowledge find <words> [--limit n] [--json]` | ranked notes, each with its best passage | what the vault says about X, once the MOCs did not answer: one call instead of files read into context |
| `knowledge find <words> --captures \| --repo [<name>]` | what `keep` and `fetch` stored, or the checkouts' docs | re-querying output you kept, or a repo's docs, without reading them again |
| `knowledge graph <note> [<note>] \| <note> --text` | a note's links in and out, the shortest path between two, or the note's text | how two notes connect, before reading them one by one: ~25 ms, no file read |
| `knowledge find <symbol> --code [--repo <name>]` | where a symbol is defined, one line each: `repo path:line kind signature` | where a function or type lives, at zero tokens, instead of a grep across the checkout |
| `knowledge graph <symbol> [<symbol>] --code [--repo <name>]` | what uses a symbol and what it uses, or the chain of uses between two (at most 4 hops) | what a change would touch, before you make it |
| `knowledge code index [<repo>\|--all] [--rebuild]` | brings a checkout's code map level by hand: only changed files are parsed | to level every map at once or rebuild a broken one; `find` and `graph --code` level a map on their own |
| `knowledge code duplicate [--staged] [<repo>]` | a new or changed definition whose body matches another's, or whose name a sibling app under `apps/` defines; warns, exits 0 | before a commit in a monorepo: a copy is caught before it is two things to maintain |
| `knowledge keep <label> [--q <words>]…` | stores stdin and prints only the sections the queries match, 2,000 chars at most | output over ~2,000 chars when you need part of it; shorter output is cheaper read whole |
| `knowledge fetch <url> [--q <words>]…` | one page as Markdown, stored and answered the way `keep` answers; what it refuses: [security.md](security.md) | a page whose address you have: Markdown cut to your query, and no request to your own network |
| `knowledge inject <prompt>` | what the prompt hook would add to a prompt | a prompt brought the wrong notes: the hook's own path, run by hand |
| `knowledge events <transcript.jsonl> [--json]` | a session's decisions, constraints, plans and refusals | what a past session decided, without opening a transcript of megabytes |
| `knowledge mcp` | an MCP server on stdio with four tools: `find`, `graph`, `read` (read-only) and `note`; `find` and `graph` take `code: true` for the code map | an app that speaks MCP but runs no hooks; `setup` registers it |

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

| Verb | What it does | When, and why |
|---|---|---|
| `knowledge note log <text>` | one bullet in today's session note (the day rolls over at `rolloverHour`), indexed at once | a harness that runs no hooks: the same write-back the hooks do |
| `knowledge note decision <text> [--question <q>] [--promoted <path>]` | one line in the decision ledger: the write-back for a harness with no hooks | a decision the capture missed, or one made where no hook runs |
| `knowledge ledger list` | the captured decisions not yet promoted to a note | the session start says decisions are waiting |
| `knowledge ledger promote <src> <path\|not-a-decision>` | marks one promoted, to the note that records it, or as not a decision | after writing its decision record, so the ledger does not pile up unread |

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

| Verb | What it does | When, and why |
|---|---|---|
| `knowledge doctor [--pretty] [--repair] [--record] [--only <id,…>]` | retrieval health, zero tokens: the index, the vault's own questions, links, pointers, priming, rules, sync; `--repair` rebuilds what is derived | first, whenever something seems off, and after setup or an update: a pipeline like this fails by going quiet |
| `knowledge doctor --only portable [--json]` | names and paths that break on the other OS | before a commit meant for both boots |
| `knowledge doctor budget [<transcript>] [--json]` | what a session's start cost, by source, and listings unused in 30 days | a session start feels heavy: it names what to trim |
| `knowledge doctor profile <note>` | the per-project settings that leave unlisted connectors off | a project loads connectors it never uses |
| `knowledge index [--rebuild] [<note>…]` | brings the index level with the vault: only changed notes are read | notes changed outside a session, or `find` misses a note you know is there |
| `knowledge index --check [--json]` | notes on disk against rows: ghosts, missing, stale | to see the drift without writing anything |
| `knowledge index --repo <checkout>` | a checkout's `docs/`, `CLAUDE.md` and `README.md` into the repo-docs index | a checkout changed outside a session; the post-edit hook keeps it level otherwise |

## Rules

| Verb | What it does | When, and why |
|---|---|---|
| `knowledge rules list [--json] \| kernel` | the packs, rules and guards in the lock; the rules that are code, not text | what is enforced, and by what |
| `knowledge rules check [--staged] [--restore]` | Hard-Rules against the lock; the staged copy and the guards' backstops; a changed rule back from `HEAD` | the doctor's rules check fails: the lock shows any edit that did not go through you |
| `knowledge rules lock [--seed <file>]` | re-stamps the lock after you edit Hard-Rules | after every Hard-Rules edit you meant |
| `knowledge rules enable\|disable <pack>` | turns a pack's guards on or off | a pack that does not fit this machine or project |
| `knowledge rules guard add --rule <id> --action deny\|ask --reason <text> …` | adds a guard to the lock: `--words "git push --force; git push -f"`, `--paths`, `--tools`, `--backstop` (staged names that fail a commit), each list comma-separated, and `--unless`, `--repo`, `--branch`, `--os` to narrow it. Add-only: no verb removes or weakens a guard, and the `rules-files` guard asks first | a rule the agent keeps breaking: a rule in a file is advice, a guard on every tool call is enforcement |

## Machine and session

| Verb | What it does | When, and why |
|---|---|---|
| `knowledge setup --plan \| --apply <id> \| --remove --plan` | this machine's setup across every harness found, planned before anything is written ([install.md](install.md), [remove.md](remove.md)) | a new machine or harness, an update, a rules change: nothing is written before you read the plan |
| `knowledge update --plan` | the latest release and this platform's steps; downloads nothing ([update.md](update.md)) | when you choose to check: nothing checks on its own |
| `knowledge config [<key>]` | the paths and settings the hooks see; a folder key prints the name in effect | a script needs the vault path, or a hook acts on the wrong folder |
| `knowledge hook session\|prompt\|pre-tool\|post-edit\|post-bash\|capture --harness <id> [--pin <v>]` | the hooks themselves: the harness's JSON on stdin, always exit 0; with `--pin`, a binary of another version stands down | not by hand: `setup` wires each harness to them. By hand only to debug, with a saved payload |
| `knowledge flush <session-id>` | a session's new turns into the daily log, once (a headless run of the harness's model) | the end-of-session hook runs it; by hand when the flush canary says a session never reached the log |
| `knowledge compile [--force] [--dry-run]` | a finished daily log into notes, once | the session hook runs it; by hand after a missed day |
| `knowledge records` | commits the records the hooks wrote, and pushes when every unpushed commit is a record | the hooks run it; by hand when the doctor's `auth` check names a failed run |
| `knowledge hubs` | regenerates the hub MOCs | a new note is not yet reachable from `00-MOC-Root`; never edit a hub by hand |
| `knowledge usage [--month <YYYY-MM>] [--days N]` | the monthly token, model and cost digest | what a month cost, by project (`--projects`) or 5-hour window (`--blocks`); read locally, sent nowhere |
| `knowledge post-bash [--backfill <since>]` | one note per commit, for commits made outside a session | you committed in a terminal with no agent |
| `knowledge --version` | the version, and whether SQLite's FTS5 is on | after an install or update, and in a bug report |
| `knowledge <verb> --help` | the verb's usage, when to use it and why; runs nothing | before running a verb you have not used, and instead of guessing its flags |
