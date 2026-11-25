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

Every hook is the `knowledge` binary's `hook` verb, and reads its paths from the machine's
`config.json`. Claude Code gets the hooks from the skeleton's plugin, `knowledge@knowledge`,
which `knowledge setup` installs: each hook is exec form, `${user_config.knowledge_bin}` with
`["hook", <verb>, "--harness", "claude"]`, so no shell starts and no other `knowledge` on PATH is
picked. Other harnesses run the same verbs (§ Other harnesses). What a hook starts in the background
is the same binary again, detached, so no hook starts a JavaScript runtime. The watermarks, locks
and exports live in `state/` beside the config (`~/.config/knowledge/state`, or `$KNOWLEDGE_HOME/state`);
the first run moves an older `~/.claude/hooks/.state` there. The `/knowledge` skill and the
slash-only `knowledge-*` skills link into the checkout, and the doctor's `skeleton-version` check fails when they do not.
`.skeleton.json` at the vault root pins the skeleton version the vault is written for: the session
start says so when the checkout is another version, and `skeleton-version` fails.

Every hook **fails silent at runtime**: it exits 0 and never blocks the session. The last column
says what notices a failure.

## Registered hooks

| Event | Matcher | Hook | What it does | Writes | Cost | When it fails |
|---|---|---|---|---|---|---|
| `SessionStart` | — | `knowledge hook session` | Commits and pushes records an earlier session left (one line when it pushed or held), then fast-forwards the vault and the skeleton checkout at the same time, each only when clean (a checkout on a detached HEAD is pinned on purpose and left alone), and says in one line when the skeleton is not the version `.skeleton.json` pins. Runs `knowledge rules check --restore`. Then injects what changes per session: the current OS, the retrieval order, the flush canary, any recorded `claude -p` failure, the health alarm, and how many captured decisions wait for promotion. Identity and Hard-Rules are **not** injected here: `~/.claude/CLAUDE.md` imports both whole, because a hook's context past 10,000 chars is cut to a preview. Re-runs a flush that was killed when the app closed, and once per logical day starts the health sweep (below) | — | ~2,000 chars, once per session; must stay ≤ 10,000 | the doctor's `priming`, `priming-size`, `skeleton-version` |
| `PreToolUse` | `Bash\|PowerShell\|Edit\|Write\|MultiEdit\|NotebookEdit\|mcp__.*` | `knowledge hook pre-tool --harness claude` | **The guard**: matches each guard in `00-Meta/rules.lock` on command words, a written path or an MCP tool name, and answers `deny` or `ask` with the rule's id. Prints nothing when it allows | — | ~20 ms | the doctor's `rules`; the pre-commit backstop |
| `UserPromptSubmit` | — | `knowledge hook prompt` | Up to six vault notes matching the prompt's rarest words, one line each, and the best passage when it is short and strong | — | ≤ 1,800 chars on a hit, 0 otherwise; ≤ 35 ms | the doctor's `retrieval`, which asks `knowledge find` the same way |
| `PreCompact`, `SessionEnd` | — | `knowledge hook capture` | Logs the event, then detaches compile, usage, flush and (SessionEnd) records (below). Timeout 10 s | `50-Ops/hook-events.jsonl` | free itself | the flush canary |
| `Stop`, `SubagentStop` | — | `knowledge hook capture` | The same, **async**, at most once per 20 min per session, flush only | same | same | same |
| ↳ detached | | `knowledge flush` | Takes the session's lock, appends every `AskUserQuestion` answer to the decision ledger, then sends what is new since the last flush to `claude -p --model haiku` and appends what is worth keeping to the daily note | `30-Sessions/YYYY/MM/YYYY-MM-DD.md`, `50-Ops/decisions.jsonl` | **one haiku call** per capture with new content | `!! KNOWLEDGE FLUSH FAILING` at the next session start; the flush canary after 3 days |
| ↳ detached | | `knowledge records` | SessionEnd only. Waits for that session's flush, then commits the hook-written records alone (`30-Sessions/`, `50-Ops/Commits/`, `decisions.jsonl`, `Health/`, `Usage/`, `00-MOC-Ops`), gitleaks first (none installed → nothing committed), and pushes when every unpushed commit is a record. Your own unpushed commits hold the push | a `chore(brain): records from the <os> boot` commit, pushed | free | `!! KNOWLEDGE RECORDS FAILING` at the next start, which also retries the sync |
| ↳ detached | | `knowledge compile` | Promotes a **finished** daily log into ADRs, knowledge and rules, links each new note from its MOC, and commits only its own changes. Does nothing while `~/.config/knowledge/state/compile.disabled` exists | `60-Decisions/`, `20-Knowledge/`, `00-Meta/`, `50-Ops/brain-compile-state.json` | **one sonnet call**, once per finished day | `!! KNOWLEDGE COMPILE FAILING` at the next session start |
| ↳ detached | | `knowledge usage` | The month's token/cost digest, once a day | `50-Ops/Usage/usage-YYYY-MM-<os>.md` | free | nothing: the file's date goes stale |
| ↳ detached | | `knowledge doctor --repair --record` | The health sweep, from the session start, once per logical day | `50-Ops/Health/health-YYYY-MM.md`, derived artifacts | free | `!! VAULT HEALTH` at the next session start when the newest line is FAIL or stale |
| `PostToolUse` | `Edit\|Write` | `knowledge hook post-edit` | On a vault note, at once: reindexes that note, then the write-time check (frontmatter, links, `[[Note#Heading]]`), reported back, never blocking | the vault index | free; ≤ 35 ms | the doctor's `vault-index` |
| `PostToolUse`, `PostToolUseFailure` | `Bash` | `knowledge hook post-bash` | **Async**, on any command naming `git` or `commit`. Writes one note per commit of the last 3 days that has none, in the vault and every repo under `devRoot`, then reindexes the vault and those repos' docs | `50-Ops/Commits/YYYY-MM/…`, `commits-YYYY-MM.md` | free | the doctor's `commits`; `--repair` backfills |

A background run whose `claude -p` fails records the reason in the temp directory and clears it
on its next success; the session start turns each record into a `!! BRAIN <TAG> FAILING` line. A
detached child carries `CLAUDE_INVOKED_BY`, and every hook verb exits at once when it is set, so a
flush's own `claude -p` cannot re-fire capture.

## Other harnesses

Each harness is an adapter (`adapters/<id>.json`, built into the binary) that maps its own event
names onto the same verbs, plus a translator that reads its payload and words the reply. The rows
above hold for each; what differs is below. Setup and the by-hand steps: the skeleton's
`docs/install.md` § Other harnesses.

| Harness | Registered by | Differs from Claude Code |
|---|---|---|
| Codex CLI | The Codex plugin `knowledge@knowledge`: `knowledge hook <verb> --harness codex --pin <version>`, from PATH | A hook runs only once you review it in `/hooks`; the doctor's `codex-trust` warns until then. The pin: a binary of another version stands every hook down, with one session-start line. An ask rule is a deny. The instructions block is inline in `~/.codex/AGENTS.md` |
| opencode | The shim `~/.config/opencode/plugins/knowledge.ts`, written by setup and pinned the same way | The guard runs in `execute.before` and a deny throws; an ask is a deny. The flush exports the session and runs `opencode run`. The block is inline in `~/.config/opencode/AGENTS.md` |
| Claude desktop app, Chat | No hooks: `mcpServers.knowledge` runs `knowledge mcp` | No capture and no guard. `note` writes the ledger or the session note, and asks first |
| Any other (`generic`) | No hooks: the block goes into the file you name | The rules are text only; `knowledge find`, `graph` and `note` from its shell |

The session start's rules restore also rewrites a stale block in each `AGENTS.md` it finds, and
leaves a file with no block alone.

## By hand

| Command | Does |
|---|---|
| `knowledge doctor [--pretty] [--repair] [--record] [--only <ids>]` | Zero-token health check, nineteen checks. **Fail:** `vault-index`, `retrieval` (the questions in `00-Meta/retrieval-cases.json` return their notes), `sessions`, `auth`, `frontmatter`, `links`, `pointers` (every `checkout:`/`docs:` pointer in `10-Repos/` exists under `devRoot`), `reach`, `ignores`, `commits`, `priming` (also each harness's instructions block), `priming-size`, `rules`, `budget`, `skeleton-version`, `portable`. **Warn:** `hygiene`, `pull-trust`, `codex-trust`. `--repair` rebuilds derived artifacts only and never edits a note; `--record` writes today's line in `50-Ops/Health/`. Always exits 0 |
| `knowledge hubs` | Regenerate the generated hubs from the folders. Never edit a hub by hand |
| `knowledge ledger list \| promote <src> <path\|not-a-decision>` | The decision ledger: the unpromoted entries, or mark one promoted to an ADR (the path must be a note) or `not-a-decision`. Takes the same lock as the flush, so a promote never drops a decision being added |
| `knowledge post-bash [--backfill YYYY-MM-DD] [--dry-run]` | The commit recorder, from a date, idempotent |
| `knowledge flush <session-id> [<cwd>] [--date YYYY-MM-DD] [--dry-run]` | A flush by hand; `--date` backfills a missed note |
| `knowledge compile [--force] [--dry-run]` | `--force` compiles today's still-open log |
| `knowledge usage [--days N] [--month YYYY-MM] [--projects] [--blocks]` | Token/model/cost digest from the transcripts |
| `knowledge records` | Commit and push the records now |
| `knowledge setup --plan [--harness <id>]` | What setup would change on this machine, for every harness found or one; nothing is written |
| `knowledge mcp` | An MCP server on stdio: `find`, `graph`, `read` (read-only) and `note`. The desktop Chat and Codex start it themselves |
| `knowledge note decision\|log <text>` | One line in the decision ledger, or in the day's session note: the write-back where no hook captures |
| `knowledge config [<key>]` | The config the hooks see; the skills and commands read their paths here |
| `knowledge update --plan` | On request only: the latest release and this platform's steps; downloads nothing |

The skills and commands run only the binary: `knowledge ledger list|promote` (the decision ledger;
the `/knowledge` skill drives it) and `knowledge doctor --only links,pointers` (every `[[wikilink]]`
and every `checkout:`/`docs:` pointer resolves).

## Change log

- 2026-11-02: the vault starts from the knowledge-skeleton template.
- 2026-11-22: context-mode removed (vault audit Stage 4): `brain-reindex` and `brain-prune` are gone, repo docs are `knowledge index --repo`, and `brain-flush` reads `knowledge events`.
- 2026-11-23 (skeleton 0.10.0): every hook is the `knowledge` binary; the doctor, compile, flush, records, usage, hubs and the commit recorder are its verbs; the session start runs the daily health sweep, so no scheduled task is needed; `knowledge setup` replaces `install.ts`.
- 2026-11-23 (skeleton 0.11.2): Claude Code's hooks come from the skeleton's plugin, not `settings.json`; Codex runs them through its own plugin, pinned to the binary's version, and opencode through a shim; `knowledge mcp` serves the desktop Chat; `note` and `config` are new verbs; the doctor gains `codex-trust`.
- 2026-11-24 (skeleton 0.12.0): the code map replaces graphify: `knowledge` hands `find --code`, `graph --code` and `code index|duplicate` to a second binary, `knowledge-code`, so no hook loads the tree-sitter grammars; Claude Code's `post-bash` matches the PowerShell tool as well as Bash.
- 2026-11-25 (knowledge 0.13.0): the engine is `knowledge` everywhere: the repo, the plugin (`knowledge@knowledge`) and the `/knowledge` skill, with no `brain-*` name left; machine state lives in the home, not the vault; `knowledge update --plan` names a newer release on request only; releases ship eight archives with `SHA256SUMS`, a build attestation each and the maintainer's signature; the session pull stops only where origin touches an uncommitted file.

## Related

[[Why-This-Exists]] · [[README]] · [[Retrieval-Protocol]] · [[Hard-Rules]] · [[00-MOC-Root]]
