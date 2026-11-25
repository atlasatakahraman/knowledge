# Architecture

How the engine is built, for someone working on it. What it does for a user is in the
[README](../README.md); every verb, with real output, in [verbs.md](verbs.md).

## The tree

```
src/            *.rs          the `knowledge` binary: every hook, the vault index, search, link graph,
                              the doctor, `setup` and the MCP server (Rust, one SQLite file per machine)
src/harness/    <id>.rs       one translator per harness with hooks: its payload in, its reply shape out
adapters/       <id>.json     one data file per harness: detection, instructions target, events,
                              headless runner; built into the binary with include_str!
claude/                       the Claude Code plugin: the hooks, in exec form
codex/                        the Codex plugin: the hooks, pinned to the binary's version
opencode/       knowledge.ts  the opencode plugin: a shim that spawns the binary
skills/knowledge/             /knowledge new|edit <kind> - writes notes in the vault's conventions
skills/knowledge-*/           /knowledge-commits /knowledge-log /knowledge-sync /knowledge-usage,
                              slash-only (disable-model-invocation)
template/                     the empty vault: folders, MOCs, conventions, system docs, Identity and
                              Hard-Rules with the generic rule packs (`FILL:` marks each placeholder)
bench/                        the token benchmark (benchmark.md)
assets/                       the README's images, text outlined: banner, hero, pills, logo; build.ts
                              draws them in every language of i18n.mjs, live.mjs draws the live
                              pills (see "The README's images"). Dev tooling run by bun, with its own
                              package.json; the engine has none
i18n/           README.<id>.md  the README in tr, es, pt, fr, de
.github/                      CI, issue forms, SECURITY.md, CONTRIBUTING.md (GitHub reads both here)
AGENTS.md                     the install, step by step, for an agent; then how to work on this repo
docs/           <task>.md     install, update, remove, verbs, security, troubleshooting, benchmark
```

## Harnesses

The engine targets what a harness can do, not a list of names: T0 reads an instructions file, T1
runs a shell, T2 loads skills, T3 runs hooks, T4 speaks MCP. A harness is one data file in
`adapters/`, one translator in `src/harness/` if it has hooks, and one setup plan if it installs a
plugin; no verb branches on a harness.

## The knowledge binary

One native program owns the vault's index and answers every hook a harness runs, so no hook starts
a JavaScript runtime (opencode's shim runs inside opencode's own). It is built from `src/`
(`cargo install --path . --locked`) into `~/.cargo/bin/knowledge`, and what it starts in the
background (the flush, compile, the records push, the usage digest, the reindex) is the same binary
again, detached. Its crates: `rusqlite` (SQLite built in, with FTS5), `serde`, `serde_json`, `jiff`
(local time for the 05:00 rollover), `ring` (SHA-256 for compile's state), and for `fetch` only
`ureq` with `rustls` on `ring`, the OS trust store (`rustls-platform-verifier`, no bundled root
list) and `htmd` (HTML to Markdown). `checks/crates.txt` lists every crate in the tree; CI fails on
any other.

The code map is a second binary from the same crate, `knowledge-code`, installed beside it by the
same `cargo install`. Every hook spawns `knowledge`, so the tree-sitter grammars stay out of it:
`knowledge` hands its code verbs to `knowledge-code` (8.4 MB beside its 6.3 MB, a Windows release
build). Its crates: `tree-sitter` (the parser and query engine, C compiled by the same compiler
SQLite needs) and one grammar per language: `tree-sitter-typescript` (`.ts`, `.tsx`),
`tree-sitter-javascript` (`.js`, `.mjs`, `.cjs`, `.jsx`), `tree-sitter-rust` and
`tree-sitter-python`. Other languages are not mapped; `find --repo` still searches a checkout's
docs as text.

Three files in `$KNOWLEDGE_HOME`, else the config's folder, all per machine and never in git:
`vault-index.db` (rebuilt from the notes in about a second), `repos-index.db` (repo docs, kept apart
so a repo doc never reaches the prompt hook or the vault's link checks) and `captures.db` (what
`keep` and `fetch` stored; a capture older than 14 days is deleted at the next write). Beside them,
`code/<checkout>.db` holds one checkout's code map. Each is built by the first code query that names
the checkout and brought level before every one after: a file whose size and mtime are unchanged
is not read, and git's tracked files are the only ones mapped.

What `fetch` refuses before it connects: [security.md](security.md#what-fetch-refuses).

## Per-machine config

The hooks read their paths from one untracked file, written once per machine:

- Every OS: `~/.config/knowledge/config.json` (or `$XDG_CONFIG_HOME/knowledge/config.json`);
  on Windows that is `%USERPROFILE%\.config\knowledge\`, never `%APPDATA%` ([install.md](install.md))
- Any OS: `$KNOWLEDGE_HOME/config.json` when `KNOWLEDGE_HOME` is set, and `$KNOWLEDGE_CONFIG`
  over both

```json
{ "vault": "/home/me/knowledge", "devRoot": "/home/me/dev", "os": "linux",
  "machine": "Free text the SessionStart banner opens with.", "rolloverHour": 5 }
```

`vault` and `devRoot` are required. `"pullTrust": "signed"` makes each session start pull only a
tip signed by a key in `allowed_signers` beside the config ([security.md](security.md)).
`rolloverHour` (default 5) is when a daily note starts, local time. `folders` renames the vault's
folders. `setup` writes it. Check what the hooks see with `knowledge config`.

## Sync

Each session start fast-forwards the vault and this checkout. Uncommitted changes block a pull
only when origin changed one of the same files (`state::incoming_overlaps`); then nothing is
merged and the start says so. A checkout on a detached HEAD (a tag) is pinned on purpose and left
alone.

The vault pushes one thing on its own: the records the hooks write (daily logs, commit notes, the
decision ledger, health and usage). After the SessionEnd flush, `knowledge records` commits them
alone and pushes when every unpushed commit is a record, so another machine pulls the session it
did not run. Your own commits never push on their own and hold the records push while unpushed. The
next session start retries what a shutdown cut off; `.gitattributes` merges the records by union.

The vault's `.skeleton.json` (`{"version":"0.13.1"}`) pins the skeleton version its notes are
written for; `setup` writes it into a new vault. When the running checkout is another version, the
session opens with one line saying which side moves, and the doctor's `skeleton-version` fails. A
vault without the file is not checked.

The checkout's version is `Cargo.toml`'s. Every bump is tagged `v<version>` on the commit that
makes it, so `git log v<pin>..` lists what changed since the vault's pin.

## CI

Every target in CI builds the binary and runs `checks/stage-3.sh` to `checks/stage-7.sh` on the
template vault (x64 and ARM64: Windows, macOS, Linux glibc and musl), and every adapter's code path
on fixture installs (`checks/stage-7.sh`). A target counts as verified once that run passes; until
then it is built, not verified.

`badges.yml` redraws the README's live pills daily and on every tag push, and force-pushes them as
the one commit on the orphan `badges` branch; unchanged pills push nothing. Its token can write
contents for that push and nothing else. The runner has no bun, so the workflow fetches one bun
release and checks it against the SHA-256 GitHub records for that asset before it runs; moving bun
means changing `BUN_VERSION` and `BUN_SHA256` together.

## The README's images

Every image in the README is an SVG with its text as outlines, so it renders the same with none of
its fonts installed, in light and dark (`<picture>` follows GitHub's theme). `assets/build.ts` draws
them all, following the CSS of the design previews to the pixel: Chrome's metrics, kerning,
ligatures and wrapping, checked glyph by glyph against the browser.

```bash
bun install --cwd assets
bun --bun assets/build.ts <fonts-dir>
```

`<fonts-dir>` holds static instances of the three variable fonts (Google Fonts, OFL 1.1); the
header of `build.ts` gives the `fonttools varLib.instancer` line for each. The live pills (stars,
the newest tag) cannot be drawn ahead of time: `build.ts` writes `assets/badges/live.json`, the
glyphs and keys they need, and the badges workflow draws them from it with `assets/live.mjs`, with
no fonts and no dependencies.

GitHub takes the social preview and an avatar only as PNG, uploaded by hand (the repo's Settings,
Social preview). `build.ts` writes `social.svg` (1280 x 640) and `avatar.svg` (512 x 512), and any
Chromium browser renders them; with Edge on Windows, from the repo root:

```bash
msedge --headless=new --hide-scrollbars --force-device-scale-factor=1 --window-size=1280,640 --screenshot="$PWD/assets/social.png" "file:///$PWD/assets/social.svg"
msedge --headless=new --hide-scrollbars --force-device-scale-factor=1 --window-size=512,512 --screenshot="$PWD/assets/avatar.png" "file:///$PWD/assets/avatar.svg"
```

### Languages

The README comes in six languages: `README.md` (English) and `i18n/README.<id>.md` for `tr`, `es`,
`pt` (Brazilian), `fr` and `de`, kept out of the root so the file list stays short above the README. Every word drawn into an image lives in `assets/i18n.mjs`, one entry per
language: the hero's rows (`_…_` underlines, `` `…` `` is a code chip), its footer, the pills'
labels and values, and the decimal mark. Names that are commands or features (`prompt hook`,
`guard`, `knowledge find`) stay English in every language.

Each image is written twice over:

| File | Holds | Used by |
|---|---|---|
| `<name>-<theme>.svg` | every language, in an SVG `<switch>` | `README.md` |
| `<name>-<id>-<theme>.svg` | that language only | `i18n/README.<id>.md` |

A README cannot see its reader's language, but an image can: in a `<switch>`, the browser draws the
first child whose `systemLanguage` matches any language in its list, by exact tag or by prefix
(`pt` matches `pt-BR`, `pt-BR` does not match `pt`). So English, with no tags, goes last as the
fallback, and a reader with Turkish anywhere in their list sees Turkish even when English comes
first. That is why each translated README names its own fixed files. The all-language file is as
tall as its tallest language, so a shorter one leaves empty space under the hero card.

The fonts are Latin only, so a language in another script needs fonts that cover it first; the
build stops on any letter a font lacks. To add a language: add its entry to `i18n.mjs` (`tags`
lists every regional tag to match, Spanish's `es-419` and `es-MX` among them), write
`i18n/README.<id>.md` with README.md's code blocks and links unchanged (a `../` before each repo
path), add the id to the language row
of every README and to the list in `checks/stage-9.sh`, and run the build. The check fails a
translation whose code blocks, links or images drift from README.md's.
