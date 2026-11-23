---
type: doc
project: knowledge
tags: [hooks, lifecycle, brain, operations]
created: 2026-11-02
updated: 2026-11-23
status: active
---

# Hook lifecycle

Every hook that touches this vault: its event, what it does, what it writes, what it costs, and
what notices when it breaks. The rationale is in [[Why-This-Exists]].

Every hook is the `knowledge` binary's `hook` verb, registered by `knowledge setup` from the
skeleton's `settings.hooks.json`, and reads its paths from the machine's `config.json`. What a hook
starts in the background is the same binary again, detached, so no hook starts a JavaScript
runtime. `~/.claude/hooks` links the skeleton checkout's `hooks/` (its `.state/` holds the
watermarks and locks), and the doctor's `skeleton-version` check fails when it does not.
`.skeleton.json` at the vault root pins the skeleton version the vault is written for: the session
start says so when the checkout is another version, and `skeleton-version` fails.

Every hook **fails silent at runtime**: it exits 0 and never blocks the session. The last column
says what notices a failure.

## Registered hooks

| Event | Matcher | Hook | What it does | Writes | Cost | When it fails |
|---|---|---|---|---|---|---|
| `SessionStart` | — | `knowledge hook session` | Commits and pushes records an earlier session left (one line when it pushed or held), then fast-forwards the vault and the skeleton checkout at the same time, each only when clean (a checkout on a detached HEAD is pinned on purpose and left alone), and says in one line when the skeleton is not the version `.skeleton.json` pins. Runs `knowledge rules check --restore`. Then injects what changes per session: the current OS, the retrieval order, the flush canary, any recorded `claude -p` failure, the health alarm, and how many captured decisions wait for promotion. Identity and Hard-Rules are **not** injected here: `~/.claude/CLAUDE.md` imports both whole, because a hook's context past 10,000 chars is cut to a preview. Re-runs a flush that was killed when the app closed, and once per logical day starts the health sweep (below) | — | ~2,000 chars, once per session; must stay ≤ 10,000 | the doctor's `priming`, `priming-size`, `skeleton-version` |
| `PreToolUse` | `Bash\|PowerShell\|Edit\|Write\|MultiEdit\|NotebookEdit\|mcp__.*` | `knowledge hook pre-tool` | **The guard**: matches each guard in `00-Meta/rules.lock` on command words, a written path or an MCP tool name, and answers `deny` or `ask` with the rule's id. Prints nothing when it allows | — | ~20 ms | the doctor's `rules`; the pre-commit backstop |
| `UserPromptSubmit` | — | `knowledge hook prompt` | Up to six vault notes matching the prompt's rarest words, one line each, and the best passage when it is short and strong | — | ≤ 1,800 chars on a hit, 0 otherwise; ≤ 35 ms | the doctor's `retrieval`, which asks `knowledge find` the same way |
| `PreCompact`, `SessionEnd` | — | `knowledge hook capture` | Logs the event, then detaches compile, usage, flush and (SessionEnd) records (below). Timeout 10 s | `50-Ops/hook-events.jsonl` | free itself | the flush canary |
| `Stop`, `SubagentStop` | — | `knowledge hook capture` | The same, **async**, at most once per 20 min per session, flush only | same | same | same |
| ↳ detached | | `knowledge flush` | Takes the session's lock, appends every `AskUserQuestion` answer to the decision ledger, then sends what is new since the last flush to `claude -p --model haiku` and appends what is worth keeping to the daily note | `30-Sessions/YYYY/MM/YYYY-MM-DD.md`, `50-Ops/decisions.jsonl` | **one haiku call** per capture with new content | `!! BRAIN FLUSH FAILING` at the next session start; the flush canary after 3 days |
| ↳ detached | | `knowledge records` | SessionEnd only. Waits for that session's flush, then commits the hook-written records alone (`30-Sessions/`, `50-Ops/Commits/`, `decisions.jsonl`, `Health/`, `Usage/`, `00-MOC-Ops`), gitleaks first (none installed → nothing committed), and pushes when every unpushed commit is a record. Your own unpushed commits hold the push | a `chore(brain): records from the <os> boot` commit, pushed | free | `!! BRAIN RECORDS FAILING` at the next start, which also retries the sync |
| ↳ detached | | `knowledge compile` | Promotes a **finished** daily log into ADRs, knowledge and rules, links each new note from its MOC, and commits only its own changes. Does nothing while `~/.claude/hooks/.state/compile.disabled` exists | `60-Decisions/`, `20-Knowledge/`, `00-Meta/`, `50-Ops/brain-compile-state.json` | **one sonnet call**, once per finished day | `!! BRAIN COMPILE FAILING` at the next session start |
| ↳ detached | | `knowledge usage` | The month's token/cost digest, once a day | `50-Ops/Usage/usage-YYYY-MM-<os>.md` | free | nothing: the file's date goes stale |
| ↳ detached | | `knowledge doctor --repair --record` | The health sweep, from the session start, once per logical day | `50-Ops/Health/health-YYYY-MM.md`, derived artifacts | free | `!! VAULT HEALTH` at the next session start when the newest line is FAIL or stale |
| `PostToolUse` | `Edit\|Write` | `knowledge hook post-edit` | On a vault note, at once: reindexes that note, then the write-time check (frontmatter, links, `[[Note#Heading]]`), reported back, never blocking | the vault index | free; ≤ 35 ms | the doctor's `vault-index` |
| `PostToolUse`, `PostToolUseFailure` | `Bash` | `knowledge hook post-bash` | **Async**, on any command naming `git` or `commit`. Writes one note per commit of the last 3 days that has none, in the vault and every repo under `devRoot`, then reindexes the vault and those repos' docs | `50-Ops/Commits/YYYY-MM/…`, `commits-YYYY-MM.md` | free | the doctor's `commits`; `--repair` backfills |

A background run whose `claude -p` fails records the reason in the temp directory and clears it
on its next success; the session start turns each record into a `!! BRAIN <TAG> FAILING` line. A
detached child carries `CLAUDE_INVOKED_BY`, and every hook verb exits at once when it is set, so a
flush's own `claude -p` cannot re-fire capture.

## By hand

| Command | Does |
|---|---|
| `knowledge doctor [--pretty] [--repair] [--record] [--only <ids>]` | Zero-token health check, seventeen checks. **Fail:** `vault-index`, `retrieval` (the questions in `00-Meta/retrieval-cases.json` return their notes), `sessions`, `auth`, `frontmatter`, `links`, `reach`, `ignores`, `commits`, `priming`, `priming-size`, `rules`, `budget`, `skeleton-version`, `portable`. **Warn:** `hygiene`, `pull-trust`. `--repair` rebuilds derived artifacts only and never edits a note; `--record` writes today's line in `50-Ops/Health/`. Always exits 0 |
| `knowledge hubs` | Regenerate the generated hubs from the folders. Never edit a hub by hand |
| `knowledge post-bash [--backfill YYYY-MM-DD] [--dry-run]` | The commit recorder, from a date, idempotent |
| `knowledge flush <session-id> [<cwd>] [--date YYYY-MM-DD] [--dry-run]` | A flush by hand; `--date` backfills a missed note |
| `knowledge compile [--force] [--dry-run]` | `--force` compiles today's still-open log |
| `knowledge usage [--days N] [--month YYYY-MM] [--projects] [--blocks]` | Token/model/cost digest from the transcripts |
| `knowledge records` | Commit and push the records now |
| `knowledge setup --plan` | What setup would change on this machine; nothing is written |

The skills and commands still run four bun tools from `~/.claude/hooks`: `brain-ledger.mjs` (the
decision ledger; the `/brain` skill drives it), `brain-verify.mjs` (every `[[wikilink]]` and every
`checkout:`/`docs:` pointer resolves), `brain-graph.mjs` (merge the repo graphs into the served
graph) and `skeleton-config.mjs` (print the config the hooks resolved).

## Change log

- 2026-11-02: the vault starts from the knowledge-skeleton template.
- 2026-11-22: context-mode removed (vault audit Stage 4): `brain-reindex` and `brain-prune` are gone, repo docs are `knowledge index --repo`, and `brain-flush` reads `knowledge events`.
- 2026-11-23 (skeleton 0.10.0): every hook is the `knowledge` binary; the doctor, compile, flush, records, usage, hubs and the commit recorder are its verbs; the session start runs the daily health sweep, so no scheduled task is needed; `knowledge setup` replaces `install.ts`.

## Related

[[Why-This-Exists]] · [[README]] · [[Retrieval-Protocol]] · [[Hard-Rules]] · [[00-MOC-Root]]
