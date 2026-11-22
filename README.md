# knowledge-skeleton

The engine behind a Claude Code knowledge vault: an Obsidian vault of plain Markdown notes that
every session reads before it answers and writes back to as it works. Hooks log sessions, record
commits, inject relevant note names into each prompt, promote decisions, and keep a search index
and a link graph current. The vault itself holds notes only and stays private.

```
src/            *.rs          the `knowledge` binary: the vault index, search, prompt and edit hooks,
                              link graph, portability check (Rust, one SQLite file per machine)
hooks/          brain-*.mjs   Claude Code hooks and their tools, run with `bun --bun`
skills/brain/                 /brain new|edit <kind> - writes notes in the vault's conventions
commands/       brain-*.md    /brain-commits /brain-log /brain-sync /brain-usage
template/                     the empty vault: folders, MOCs, conventions, system docs, Identity and
                              Hard-Rules stubs (`FILL:` marks each placeholder)
install.ts                    sets a machine up: prerequisites, the binary (`cargo install`), graphify,
                              context-mode, vault, config, links, hook block, CLAUDE.md imports, first index
INSTALL.md                    the same steps by hand, for a person or an agent
settings.hooks.json           the hook block install.ts merges into ~/.claude/settings.json
```

## Install

```bash
git clone https://github.com/atlasatakahraman/knowledge-skeleton ~/dev/knowledge-skeleton
bun ~/dev/knowledge-skeleton/install.ts --dry-run && bun ~/dev/knowledge-skeleton/install.ts
```

A new vault comes from `template/` at `~/knowledge`; an existing one is adopted. Safe to run again:
a step that is done reports `ok`. Flags, Windows, and every step by hand: [INSTALL.md](INSTALL.md).

## After a new vault

Fill `00-Meta/Identity.md` and `00-Meta/Hard-Rules.md` (`grep -rn "FILL:" ~/knowledge/00-Meta`
lists what is left) and replace the questions in `00-Meta/retrieval-cases.json` with ones the vault
must always answer: `brain-doctor`'s `retrieval` check runs them. The template's own `README.md`
has the rest.

## Requirements

`bun`, `git`, Claude Code (`claude`), `gitleaks`, `uv` for graphify, and the Rust toolchain
(`rustup`; `rust-toolchain.toml` pins 1.100.0 and rustup fetches it on the first build).

## The knowledge binary

One native program owns the vault's index and answers the hooks that run on every prompt and edit,
so neither starts a JavaScript runtime. It is built from `src/` by `install.ts`
(`cargo install --path . --locked`) into `~/.cargo/bin/knowledge`. Three crates: `rusqlite` (SQLite
built in, with FTS5), `serde`, `serde_json`; no network code at all.

| Verb | What it does |
|---|---|
| `knowledge index [--rebuild] [<note>…]` | brings the index level with the vault: only changed notes are read |
| `knowledge index --check [--json]` | notes on disk against rows: ghosts, missing, stale |
| `knowledge find <words> [--limit n] [--json]` | ranked notes, each with its best passage |
| `knowledge inject <prompt>` | what the prompt hook would add to a prompt |
| `knowledge graph <note> [<note>]` | a note's links in and out, or the shortest path between two |
| `knowledge hook prompt\|post-edit --harness claude` | the hooks themselves: JSON on stdin, always exit 0 |
| `knowledge doctor --only portable [--json]` | names and paths that break on the other OS |

The index is `vault-index.db` in `$KNOWLEDGE_HOME`, else the config's folder: per machine, never in
git, rebuilt from the notes in about a second. Every target in CI builds the binary and runs
`checks/stage-3.sh` on the template vault (x64 and ARM64: Windows, macOS, Linux glibc and musl). A
target counts as verified once that run passes; until then it is built, not verified.

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
time. `install.ts` writes it. Check what the hooks see with `bun --bun hooks/skeleton-config.mjs`.

## Sync

Each session start fast-forwards the vault and this checkout, each only when its tree is clean.
A checkout on a detached HEAD (a tag) is pinned on purpose and left alone.

The vault pushes one thing on its own: the records the hooks write (daily logs, commit notes, the
decision ledger, health and usage). After the SessionEnd flush, `brain-records` commits them alone
and pushes when every unpushed commit is a record, so another machine pulls the session it did not
run. Your own commits never push on their own and hold the records push while unpushed. The next
session start retries what a shutdown cut off; `.gitattributes` merges the records by union.

The vault's `.skeleton.json` (`{"version":"0.3.0"}`) pins the skeleton version its notes are
written for; `install.ts` writes it into a new vault. When the running checkout is another
version, the session opens with one line saying which side moves, and `brain-doctor`'s
`skeleton-version` fails. A vault without the file is not checked.

Every version bump in `package.json` is tagged `v<version>` on the commit that makes it, so
`git log v<pin>..` lists what changed since the vault's pin.

## Third-party tools

Installed from their own upstreams, never vendored here:

- [graphify](https://github.com/safishamsi/graphify) (`graphifyy[mcp]` on PyPI) - MIT. Builds each
  repository's code graph and serves their merge over MCP. The vault's own links are the binary's.
- [context-mode](https://github.com/mksglu/context-mode) - Elastic License 2.0. Sandboxed tools and a
  full-text index of repo docs, and once a day of the vault, for `ctx_search`. The vault's retrieval
  does not depend on it.
- [gitleaks](https://github.com/gitleaks/gitleaks) - MIT. Scans every commit (`githooks/pre-commit`)
  and the records before they are pushed. Without it, nothing is committed.

## License

MIT. See `LICENSE`. The third-party tools above keep their own licences.

Versions up to 0.4.0 were released under GPL-3.0-only and stay available under it; from 0.5.0 on
the skeleton takes no GPL dependency, so the MIT grant stays whole.
