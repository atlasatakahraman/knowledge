---
type: doc
project: knowledge
tags: [hooks, lifecycle, brain, operations]
created: 2026-11-02
updated: 2026-11-02
status: active
---

# Hook lifecycle

Every hook that touches this vault: its event, what it does, what it writes, what it costs, and
what notices when it breaks. The rationale is in [[Why-This-Exists]].

The hooks live in the knowledge-skeleton checkout's `hooks/`; `~/.claude/hooks` is a link to it,
and `brain-doctor`'s `skeleton-version` check fails when it is not. Every hook runs as
`bun --bun ~/.claude/hooks/<hook>` and reads its paths from the machine's `config.json`.
`.skeleton.json` at the vault root pins the skeleton version the vault is written for:
`brain-session` says so at session start when the checkout is another version, and
`skeleton-version` fails.

Every brain hook **fails silent at runtime**: it exits 0 and never blocks the session. The last
column says what notices a failure.

## Registered hooks

| Event | Matcher | Hook | What it does | Writes | Cost | When it fails |
|---|---|---|---|---|---|---|
| `SessionStart` | — | `brain-session.mjs` | Commits and pushes records an earlier session left (`syncRecords()`, one line when it pushed or held), then fast-forwards the vault and the skeleton checkout, each only when clean (a checkout on a detached HEAD is pinned on purpose and left alone), and says in one line when the skeleton is not the version `.skeleton.json` pins. Then injects what changes per session: the current OS, the retrieval order, the flush canary, any recorded `claude -p` failure, the health alarm, and how many captured decisions wait for promotion. Identity and Hard-Rules are **not** injected here: `~/.claude/CLAUDE.md` imports both whole, because a hook's context past 10,000 chars is cut to a preview. Re-runs a flush that was killed when the app closed | — | ~2,000 chars, once per session; must stay ≤ 10,000 | `brain-doctor` § priming, § skeleton-version |
| `UserPromptSubmit` | — | `knowledge hook prompt` | Up to six vault notes matching the prompt's rarest words, one line each, and the best passage when it is short and strong. A native process, no bun start | — | ≤ 1,800 chars on a hit, 0 otherwise; ≤ 35 ms | `brain-doctor` § retrieval, which asks `knowledge find` the same way |
| `PreCompact`, `SessionEnd` | — | `brain-capture.mjs` | Logs the event, then detaches compile, usage and flush (below). Timeout 10 s | `50-Ops/hook-events.jsonl` | free itself | the flush canary |
| `Stop`, `SubagentStop` | — | `brain-capture.mjs` | The same, **async**, at most once per 20 min per session, flush only | same | same | same |
| ↳ detached | | `brain-flush.mjs` | Takes the session's lock, appends every `AskUserQuestion` answer to the decision ledger, then sends what is new since the last flush to `claude -p --model haiku` and appends what is worth keeping to the daily note | `30-Sessions/YYYY/MM/YYYY-MM-DD.md`, `50-Ops/decisions.jsonl` | **one haiku call** per capture with new content | `!! BRAIN FLUSH FAILING` at the next session start; the flush canary after 3 days |
| ↳ detached | | `brain-records.mjs` | SessionEnd only. Waits for that session's flush lock to clear (≤ 200 s), then `syncRecords()`: commits the hook-written records alone (`30-Sessions/`, `50-Ops/Commits/`, `decisions.jsonl`, `Health/`, `Usage/`, `00-MOC-Ops`), gitleaks first (none installed → nothing committed), and pushes when every unpushed commit is a record. Behind origin it rebases; `.gitattributes` merges the records by union. Your own unpushed commits hold the push. By hand with no argument: sync now | a `chore(brain): records from the <os> boot` commit, pushed | free | `!! BRAIN RECORDS FAILING` at the next start, which also retries the sync |
| ↳ detached | | `brain-compile.mjs` | Promotes a **finished** daily log into ADRs, knowledge and rules, links each new note from its MOC, and commits only its own changes. Does nothing while `~/.claude/hooks/.state/compile.disabled` exists | `60-Decisions/`, `20-Knowledge/`, `00-Meta/`, `50-Ops/brain-compile-state.json` | **one sonnet call**, once per finished day | `!! BRAIN COMPILE FAILING` at the next session start |
| ↳ detached | | `brain-usage.mjs` | The month's token/cost digest, once a day | `50-Ops/Usage/usage-YYYY-MM-<os>.md` | free | nothing: the file's date goes stale |
| `PostToolUse` | `Edit\|Write` | `knowledge hook post-edit` | On a vault note, at once: reindexes that note, then the write-time check (frontmatter, links, `[[Note#Heading]]`), reported back, never blocking | the vault index | free; ≤ 35 ms | `brain-doctor` § vault-index |
| `PostToolUse`, `PostToolUseFailure` | `Bash` (and `PowerShell` on Windows) | `brain-commit.mjs` | **Async**, on any command naming `git` or `commit`. Writes one note per commit of the last 3 days that has none, in the vault and every repo under `devRoot`, then reindexes the vault and those repos' docs | `50-Ops/Commits/YYYY-MM/…`, `commits-YYYY-MM.md` | free | `brain-doctor` § commits; `--repair` backfills |

A background hook whose `claude -p` fails records the reason in the temp directory and clears it
on its next success; `brain-session` turns each record into a `!! BRAIN <TAG> FAILING` line.
`brain-capture` and `brain-compile` exit when `CLAUDE_INVOKED_BY` is set, so a flush's own
`claude -p` cannot re-fire capture.

## CLI-only tools

Not registered on any event. Run them by hand, or from `/brain-sync`. All run as
`bun --bun ~/.claude/hooks/<tool>`.

| Tool | Flags | Does |
|---|---|---|
| `brain-doctor.mjs` | none → JSON · `--pretty` · `--repair` · `--record` · `--only <ids>` | Zero-token health check, fourteen checks. **Fail:** `vault-index`, `retrieval` (the questions in `00-Meta/retrieval-cases.json` return their notes), `sessions`, `auth`, `frontmatter`, `links`, `reach`, `ignores`, `commits`, `priming`, `skeleton-version`, `portable`. **Warn:** `hygiene`, `pull-trust`. `--repair` rebuilds derived artifacts only and never edits a note; `--record` writes today's line in `50-Ops/Health/`. Always exits 0 |
| `brain-graph.mjs` | none | Merge the repo graphs into the served graph by hand. The vault's links are `knowledge graph`'s |
| `brain-hubs.mjs` | none | Regenerate the generated hubs from the folders. Never edit a hub by hand |
| `brain-verify.mjs` | none | Every `[[wikilink]]` resolves, and every `checkout:`/`docs:` pointer exists under `devRoot` (relative; a repo not cloned here is listed, an absolute pointer fails). Exits 1 on a break |
| `brain-commit.mjs` | `--backfill YYYY-MM-DD` · `--dry-run` | The commit recorder, from a date, idempotent |
| `brain-flush.mjs` | `<session-id> <cwd> [--date YYYY-MM-DD] [--dry-run]` | A flush by hand; `--date` backfills a missed note |
| `brain-compile.mjs` | `--force` · `--dry-run` | `--force` compiles today's still-open log |
| `brain-usage.mjs` | `--days N` · `--month YYYY-MM` · `--projects` · `--blocks` | Token/model/cost digest from the transcripts |
| `brain-ledger.mjs` | `list` · `add` · `promote` | The decision ledger; the `/brain` skill drives it |
| `skeleton-config.mjs` | none · `<key>` | Print the config the hooks resolved |

## Change log

- 2026-11-02: the vault starts from the knowledge-skeleton template.
- 2026-11-22: context-mode removed (vault audit Stage 4): `brain-reindex` and `brain-prune` are gone, repo docs are `knowledge index --repo`, and `brain-flush` reads `knowledge events`.

## Related

[[Why-This-Exists]] · [[README]] · [[Retrieval-Protocol]] · [[Hard-Rules]] · [[00-MOC-Root]]
