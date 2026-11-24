# knowledge-skeleton

The engine behind a Claude Code knowledge vault: an Obsidian vault of plain Markdown notes that
every session reads before it answers and writes back to as it works. Hooks log sessions, record
commits, inject relevant note names into each prompt, promote decisions, and keep a search index
and a link graph current. The vault itself holds notes only and stays private.

```
src/            *.rs          the `knowledge` binary: every hook, the vault index, search, link graph,
                              the doctor, `setup` and the MCP server (Rust, one SQLite file per machine)
src/harness/    <id>.rs       one translator per harness with hooks: its payload in, its reply shape out
adapters/       <id>.json     one data file per harness: detection, instructions target, events,
                              headless runner; built into the binary with include_str!
claude/                       the Claude Code plugin: the hooks, in exec form
codex/                        the Codex plugin: the hooks, pinned to the binary's version
opencode/       knowledge.ts  the opencode plugin: a shim that spawns the binary
hooks/          brain-*.mjs   the hooks from before the binary, kept until every machine runs it;
                              nothing on the binary's path runs them
skills/brain/                 /brain new|edit <kind> - writes notes in the vault's conventions
commands/       brain-*.md    /brain-commits /brain-log /brain-sync /brain-usage
template/                     the empty vault: folders, MOCs, conventions, system docs, Identity and
                              Hard-Rules with the generic rule packs (`FILL:` marks each placeholder)
AGENTS.md                     the install, step by step, for an agent; then how to work on this repo
docs/           <task>.md     install, update, remove, verbs, security, troubleshooting
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
--remove --plan` is the reverse, for the same harnesses, and never touches the vault. Flags, Windows, and every step by hand: [docs/install.md](docs/install.md). An agent
installing for you follows [AGENTS.md](AGENTS.md). Then [update](docs/update.md),
[remove](docs/remove.md), [every verb](docs/verbs.md), [security](docs/security.md) and
[troubleshooting](docs/troubleshooting.md).

## Harnesses

The engine targets what a harness can do, not a list of names: T0 reads an instructions file, T1
runs a shell, T2 loads skills, T3 runs hooks, T4 speaks MCP. A harness is one data file in
`adapters/`, one translator in `src/harness/` if it has hooks, and one setup plan if it installs a
plugin; no verb branches on a harness. "Verified" means a live session on a real install
(Windows, 2026-11-23); the same checks on Arch wait for its pass.

| Harness | Tiers used | What it gets | Enforcement points | Status |
|---|---|---|---|---|
| Claude Code | T0–T4 | Identity and Hard-Rules imported whole into `~/.claude/CLAUDE.md`; the plugin's hooks on nine events; `/brain`; the code map (`knowledge find --code`, `graph --code`) | The guard on `PreToolUse` (shell, edit and MCP calls) with deny and ask; the session-start restore; the pre-commit backstop | Verified |
| Codex CLI | T0–T4 | The block written whole into `~/.codex/AGENTS.md`; the plugin's hooks on eight events, pinned to the binary's version; `/brain` through `~/.agents/skills`; `knowledge` over MCP, the code map included | The guard on `PreToolUse` (`Bash`, `apply_patch`, MCP calls), deny only: an ask rule is a deny, since Codex cannot ask; hooks run only once you review them; restore and backstop | Verified |
| opencode | T0–T3 | The block in `~/.config/opencode/AGENTS.md`; the shim plugin on eight events, seven of which run the binary; `/brain` | The guard in `execute.before` (`shell`, `edit`, `write`, `patch`), deny only; restore and backstop | Verified; its compaction and session-delete events only run in the interactive app and were not reached |
| Claude desktop app, Chat | T4 | `knowledge mcp`: `find`, `graph`, `read`, and `note`, which asks before it writes | None: no shell and no hooks, so no tool call of ours to guard | Verified |
| ChatGPT app's Codex | T0–T4 | The Codex plugin's hooks, once trusted in the CLI | As Codex; hooks not yet trusted are skipped silently | Built, not verified |
| Any other (`generic`) | T0, T1 (T4 with `knowledge mcp`) | The block in a file you name, `knowledge find` and `graph`, and `knowledge note` to write back | None at run time: the rules are text only. The pre-commit backstop still runs in every clone `setup` set | Built, not verified |

Every adapter's code path also runs in CI on fixture installs (`checks/stage-7.sh`).

## After a new vault

Fill `00-Meta/Identity.md` and `00-Meta/Hard-Rules.md` (`grep -rn "FILL:" ~/knowledge/00-Meta`
lists what is left) and replace the questions in `00-Meta/retrieval-cases.json` with ones the vault
must always answer: the doctor's `retrieval` check runs them. Then `knowledge doctor --repair
--pretty` builds the first index and hubs, and `knowledge rules list` with `rules enable
<pack>` turns on the rule packs you want. The template's own `README.md` has the rest.

## Requirements

`git`, Claude Code (`claude`), `gitleaks`, and the Rust toolchain (`rustup`;
`rust-toolchain.toml` pins 1.100.0 and rustup fetches it on the first build).

## The knowledge binary

One native program owns the vault's index and answers every hook a harness runs, so no hook starts
a JavaScript runtime (opencode's shim runs inside opencode's own). It is built from `src/` (`cargo install --path . --locked`) into
`~/.cargo/bin/knowledge`, and what it starts in the background (the flush, compile, the records push,
the usage digest, the reindex) is the same binary again, detached. Its crates: `rusqlite` (SQLite
built in, with FTS5), `serde`, `serde_json`, `jiff` (local time for the 05:00 rollover), `ring`
(SHA-256 for compile's state), and for `fetch` only `ureq` with `rustls` on `ring`, the OS trust
store (`rustls-platform-verifier`, no bundled root list) and `htmd` (HTML to Markdown).
`checks/crates.txt` lists every crate in the tree; CI fails on any other.

The code map is a second binary from the same crate, `knowledge-code`, installed beside it by the
same `cargo install`. Every hook spawns `knowledge`, so the tree-sitter grammars stay out of it:
`knowledge` hands its code verbs to `knowledge-code` (8.4 MB beside its 6.3 MB, a Windows release
build). Its crates:
`tree-sitter` (the parser and query engine, C compiled by the same compiler SQLite needs) and one
grammar per language: `tree-sitter-typescript` (`.ts`, `.tsx`), `tree-sitter-javascript` (`.js`,
`.mjs`, `.cjs`, `.jsx`), `tree-sitter-rust` and `tree-sitter-python`. Other languages are not
mapped; `find --repo` still searches a checkout's docs as text.

Every verb, with real output: [docs/verbs.md](docs/verbs.md).

Three files in `$KNOWLEDGE_HOME`, else the config's folder, all per machine and never in git:
`vault-index.db` (rebuilt from the notes in about a second), `repos-index.db` (repo docs, kept apart
so a repo doc never reaches the prompt hook or the vault's link checks) and `captures.db` (what
`keep` and `fetch` stored; a capture older than 14 days is deleted at the next write). Beside them,
`code/<checkout>.db` holds one checkout's code map. Each is built by the first code query that names
the checkout and brought level before every one after: a file whose size and mtime are unchanged
is not read, and git's tracked files are the only ones mapped.

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

- **The guard** (`hook pre-tool`, on each harness's pre-tool event for shell, edit and MCP calls) splits a shell line
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
judge a wrapper (an interpreter's `-e`, a substitution, an escape). That case is caught afterwards, by the next
start's restore and the doctor.

Every target in CI builds the binary and runs `checks/stage-3.sh` to `checks/stage-7.sh` on the
template vault (x64 and ARM64: Windows, macOS, Linux glibc and musl). A target counts as verified
once that run passes; until then it is built, not verified.

## Per-machine config

The hooks read their paths from one untracked file, written once per machine:

- Every OS: `~/.config/knowledge/config.json` (or `$XDG_CONFIG_HOME/knowledge/config.json`);
  on Windows that is `%USERPROFILE%\.config\knowledge\`, never `%APPDATA%` ([docs/install.md](docs/install.md))
- Any OS: `$KNOWLEDGE_HOME/config.json` when `KNOWLEDGE_HOME` is set, and `$KNOWLEDGE_CONFIG`
  over both

```json
{ "vault": "/home/me/knowledge", "devRoot": "/home/me/dev", "os": "linux",
  "machine": "Free text the SessionStart banner opens with.", "rolloverHour": 5 }
```

`vault` and `devRoot` are required. `"pullTrust": "signed"` makes each session start pull only a
tip signed by a key in `allowed_signers` beside the config ([docs/security.md](docs/security.md)). `rolloverHour` (default 5) is when a daily note starts, local
time. `folders` renames the vault's folders. `setup` writes it. Check what the hooks see with
`knowledge config`.

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

- [gitleaks](https://github.com/gitleaks/gitleaks) - MIT. Scans every commit (`githooks/pre-commit`)
  and the records before they are pushed. Without it, nothing is committed.

## License

MIT. See `LICENSE`. The third-party tools above keep their own licences.

Versions up to 0.4.0 were released under GPL-3.0-only and stay available under it; from 0.5.0 on
the skeleton takes no GPL dependency, so the MIT grant stays whole.
