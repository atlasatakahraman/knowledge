# knowledge-skeleton

The engine behind a Claude Code knowledge vault: an Obsidian vault of plain Markdown notes that
every session reads before it answers and writes back to as it works. Hooks log sessions, record
commits, inject relevant note names into each prompt, promote decisions, and keep a search index
and a link graph current. The vault itself holds notes only and stays private.

```
src/            *.rs          the `knowledge` binary: every hook, the vault index, search, link graph,
                              the doctor and `setup` (Rust, one SQLite file per machine)
hooks/          brain-*.mjs   the tools the skills and commands run with `bun --bun` (ledger, verify,
                              graph merge, config); the hook entry points are the binary's
skills/brain/                 /brain new|edit <kind> - writes notes in the vault's conventions
commands/       brain-*.md    /brain-commits /brain-log /brain-sync /brain-usage
template/                     the empty vault: folders, MOCs, conventions, system docs, Identity and
                              Hard-Rules stubs (`FILL:` marks each placeholder)
INSTALL.md                    every step `setup` takes, by hand, for a person or an agent
settings.hooks.json           the hook block `setup` merges into ~/.claude/settings.json
```

## Install

```bash
git clone https://github.com/atlasatakahraman/knowledge-skeleton ~/dev/knowledge-skeleton
cargo install --path ~/dev/knowledge-skeleton --locked
knowledge setup --plan
knowledge setup --apply <id>      # the id is the plan's last line
```

`--plan` writes nothing: it lists every file it would create or change (the vault, new from
`template/` at `~/knowledge` or adopted, the config, the links into `~/.claude`, the hook block, the
`CLAUDE.md` imports, each clone's githooks path) and ends with the plan id, a hash of the plan and of
each target's current content. It covers Claude Code and then every other harness it finds installed
(Codex, opencode, the Claude desktop app), each under its own `found` line. `--apply <id>` recomputes
the plan and refuses if anything moved in between, then writes exactly that. `knowledge setup
--remove --plan` is the reverse, for the same harnesses, and never touches the vault. Flags, Windows, and every step by hand: [INSTALL.md](INSTALL.md).

## After a new vault

Fill `00-Meta/Identity.md` and `00-Meta/Hard-Rules.md` (`grep -rn "FILL:" ~/knowledge/00-Meta`
lists what is left) and replace the questions in `00-Meta/retrieval-cases.json` with ones the vault
must always answer: the doctor's `retrieval` check runs them. Then `knowledge doctor --repair
--pretty` builds the first index, hubs and graph, and `knowledge rules list` with `rules enable
<pack>` turns on the rule packs you want. The template's own `README.md` has the rest.

## Requirements

`git`, Claude Code (`claude`), `gitleaks`, `uv` for graphify, the Rust toolchain (`rustup`;
`rust-toolchain.toml` pins 1.100.0 and rustup fetches it on the first build), and `bun` for the
skills' and commands' tools.

## The knowledge binary

One native program owns the vault's index and answers every hook Claude Code runs, so no hook starts
a JavaScript runtime. It is built from `src/` (`cargo install --path . --locked`) into
`~/.cargo/bin/knowledge`, and what it starts in the background (the flush, compile, the records push,
the usage digest, the reindex) is the same binary again, detached. Its crates: `rusqlite` (SQLite
built in, with FTS5), `serde`, `serde_json`, `jiff` (local time for the 05:00 rollover), `ring`
(SHA-256 for compile's state), and for `fetch` only `ureq` with `rustls` on `ring`, the OS trust
store (`rustls-platform-verifier`, no bundled root list) and `htmd` (HTML to Markdown).
`checks/crates.txt` lists every crate in the tree; CI fails on any other.

| Verb | What it does |
|---|---|
| `knowledge index [--rebuild] [<note>…]` | brings the index level with the vault: only changed notes are read |
| `knowledge index --check [--json]` | notes on disk against rows: ghosts, missing, stale |
| `knowledge index --repo <checkout>` | a checkout's `docs/`, `CLAUDE.md` and `README.md` into the repo-docs index |
| `knowledge find <words> [--limit n] [--json]` | ranked notes, each with its best passage |
| `knowledge find <words> --captures \| --repo [<name>]` | what `keep` and `fetch` stored, or the checkouts' docs |
| `knowledge keep <label> [--q <words>]…` | stores stdin and prints only the sections the queries match, 2,000 chars at most |
| `knowledge fetch <url> [--q <words>]…` | one page as Markdown, stored and answered the way `keep` answers |
| `knowledge events <transcript.jsonl> [--json]` | a session's decisions, constraints, plans and refusals, for `brain-flush` |
| `knowledge inject <prompt>` | what the prompt hook would add to a prompt |
| `knowledge graph <note> [<note>]` | a note's links in and out, or the shortest path between two |
| `knowledge hook session\|prompt\|pre-tool\|post-edit\|post-bash\|capture --harness claude` | the hooks themselves: JSON on stdin, always exit 0 |
| `knowledge setup --plan \| --apply <id> \| --remove --plan` | this machine's setup across every harness found, planned before anything is written; `--harness <id>` for one |
| `knowledge doctor [--pretty] [--repair] [--record]` | retrieval health, zero tokens: the index, 33 questions, links, priming, sync; `--repair` rebuilds what is derived |
| `knowledge flush <session-id>` | a session's new turns into the daily log, once (`claude -p`, haiku) |
| `knowledge compile [--force]` | a finished daily log into notes, once (`claude -p`, sonnet) |
| `knowledge records \| hubs \| usage \| post-bash` | the records push, the MOC hubs, the monthly usage digest, the commit notes |
| `knowledge rules list [--json] \| kernel` | the packs, rules and guards in the lock; the rules that are code, not text |
| `knowledge rules check [--staged] [--restore]` | Hard-Rules against the lock; the staged copy and the guards' backstops; a changed rule back from `HEAD` |
| `knowledge rules lock [--seed <file>] \| enable\|disable <pack>` | re-stamps the lock after the owner edits Hard-Rules; turns a pack's guards on or off |
| `knowledge doctor --only portable [--json]` | names and paths that break on the other OS |
| `knowledge doctor budget [<transcript>] [--json]` | what a session's start cost, by source, and listings unused in 30 days |
| `knowledge doctor profile <note>` | the per-project settings that leave unlisted connectors off |

Three files in `$KNOWLEDGE_HOME`, else the config's folder, all per machine and never in git:
`vault-index.db` (rebuilt from the notes in about a second), `repos-index.db` (repo docs, kept apart
so a repo doc never reaches the prompt hook or the vault's link checks) and `captures.db` (what
`keep` and `fetch` stored; a capture older than 14 days is deleted at the next write).

`fetch` refuses, before it connects: anything but `http` and `https`, credentials in a URL, and any
host that resolves to a loopback, private (RFC 1918), link-local, unique-local, CGNAT, multicast or
unspecified address, IPv4 inside IPv6 included. It connects only to the addresses it checked, follows
at most 5 redirects and checks each one again, stops at 20 s and 5 MB, reads no proxy from the
environment, and stores only HTML (converted), other text and JSON. `--allow-private` lifts the
address check and nothing else.

### Rules and the guard

`00-Meta/Hard-Rules.md` stays one note, loaded whole. Each `##` section is a pack. Each rule is a
bullet that opens with a bold line, and that line's slug is the rule's id. `00-Meta/rules.lock`
(JSON, tracked) holds what the text cannot: each rule's pack, kind, enforcement points and the hash
of its whole text, and the guards. Nothing is added to the primed text. The owner edits the note,
then runs `knowledge rules lock` in their own terminal.

- **The guard** (`hook pre-tool`, on PreToolUse for shell, edit and MCP calls) splits a shell line
  into segments and words, and answers `deny` or `ask` with the rule's id. It reads only
  `rules.lock`. It prints nothing when it allows, and on a missing or broken lock it allows with
  one stderr line.
- **The backstop** is `githooks/pre-commit`. It runs the repo's own hook, gitleaks, then
  `rules check --staged`, which fails a commit holding a guard's staged names (`package-lock.json`)
  or a Hard-Rules that disagrees with the lock. `setup` sets it in the vault, the skeleton and
  each checkout a `10-Repos` page names.
- **The restore.** Each session start runs `rules check --restore`. A rule that differs from the
  lock comes back from `HEAD` when `HEAD` matches the lock, and one line says so. Otherwise the
  doctor FAILs until the owner relocks or reverts.

**What it cannot stop.** An agent that runs code as the user can change any file the user can. The
guard puts the plain forms to the owner: an Edit, `sed -i`, a redirect, `Set-Content`. It cannot
judge a wrapper (`bun -e`, a substitution, an escape). That case is caught afterwards, by the next
start's restore and the doctor.

Every target in CI builds the binary and runs `checks/stage-3.sh` to `checks/stage-6.sh` on the
template vault (x64 and ARM64: Windows, macOS, Linux glibc and musl). A target counts as verified
once that run passes; until then it is built, not verified.

## Per-machine config

The hooks read their paths from one untracked file, written once per machine:

- Every OS: `~/.config/knowledge/config.json` (or `$XDG_CONFIG_HOME/knowledge/config.json`);
  on Windows that is `%USERPROFILE%\.config\knowledge\`, never `%APPDATA%` (see INSTALL.md)
- Any OS: `$KNOWLEDGE_HOME/config.json` when `KNOWLEDGE_HOME` is set, and `$KNOWLEDGE_CONFIG`
  over both

```json
{ "vault": "/home/me/knowledge", "devRoot": "/home/me/dev", "os": "linux",
  "machine": "Free text the SessionStart banner opens with.", "rolloverHour": 5 }
```

`vault` and `devRoot` are required. `"pullTrust": "signed"` makes each session start pull only a
tip signed by a key in `allowed_signers` beside the config (INSTALL.md § Signing and pull trust). `rolloverHour` (default 5) is when a daily note starts, local
time. `setup` writes it. Check what the hooks see with `bun --bun hooks/skeleton-config.mjs`.

## Sync

Each session start fast-forwards the vault and this checkout, each only when its tree is clean.
A checkout on a detached HEAD (a tag) is pinned on purpose and left alone.

The vault pushes one thing on its own: the records the hooks write (daily logs, commit notes, the
decision ledger, health and usage). After the SessionEnd flush, `knowledge records` commits them alone
and pushes when every unpushed commit is a record, so another machine pulls the session it did not
run. Your own commits never push on their own and hold the records push while unpushed. The next
session start retries what a shutdown cut off; `.gitattributes` merges the records by union.

The vault's `.skeleton.json` (`{"version":"0.3.0"}`) pins the skeleton version its notes are
written for; `setup` writes it into a new vault. When the running checkout is another
version, the session opens with one line saying which side moves, and the doctor's
`skeleton-version` fails. A vault without the file is not checked.

Every version bump in `package.json` is tagged `v<version>` on the commit that makes it, so
`git log v<pin>..` lists what changed since the vault's pin.

## Third-party tools

Installed from their own upstreams, never vendored here:

- [graphify](https://github.com/safishamsi/graphify) (`graphifyy[mcp]` on PyPI) - MIT. Builds each
  repository's code graph and serves their merge over MCP. The vault's own links are the binary's.
- [gitleaks](https://github.com/gitleaks/gitleaks) - MIT. Scans every commit (`githooks/pre-commit`)
  and the records before they are pushed. Without it, nothing is committed.

## License

MIT. See `LICENSE`. The third-party tools above keep their own licences.

Versions up to 0.4.0 were released under GPL-3.0-only and stay available under it; from 0.5.0 on
the skeleton takes no GPL dependency, so the MIT grant stays whole.
