# Installing knowledge-skeleton

One command does all of it; every step it takes is also written out below, so a person or an agent
can do it by hand and get the same machine. The installer is safe to run again at any time: each
step checks first, and a step that is already done reports `ok`.

```bash
git clone https://github.com/atlasatakahraman/knowledge-skeleton ~/dev/knowledge-skeleton
bun ~/dev/knowledge-skeleton/install.ts --dry-run    # what it would do
bun ~/dev/knowledge-skeleton/install.ts              # do it
```

```powershell
git clone https://github.com/atlasatakahraman/knowledge-skeleton C:\dev\knowledge-skeleton
bun C:\dev\knowledge-skeleton\install.ts --dry-run    # what it would do
bun C:\dev\knowledge-skeleton\install.ts              # do it
```

| Flag | Default | |
|---|---|---|
| `--vault <path>` | `config.json`'s, else `~/knowledge` | The vault. No spaces: `CLAUDE.md` imports cannot hold one |
| `--dev <path>` | `config.json`'s, else `~/dev` | Where checkouts live, one folder per GitHub remote |
| `--machine "<text>"` | empty | Free text the SessionStart banner opens with: hardware, dual-boot, dotfiles |
| `--from-template` / `--adopt` | adopt a non-empty path, else template | A new vault from `template/`, or an existing one (it must have `00-Meta/00-MOC-Root.md`) |
| `--dry-run` | | Print each step, change nothing |

It exits 0 only when every step and `brain-doctor` pass. On a FAIL it says what to fix; fix it and
run it again.

## For an agent

- Run `--dry-run` first and show the output to the person before the real run.
- Never edit a hook in `~/.claude/settings.json` that is not a `brain-*` command, and never commit
  the vault for them. The first commit of a template vault is the installer's, and only when git
  has an identity.
- A FAIL line is the answer to report: quote it, do not work around it.
- If `claude`, `uv` or `git` is missing, the installer stops with the command that installs it.
  Installing one is the person's call: ask first.

## What it does, by hand

Paths below are Linux; the Windows form follows each block where it differs. `<skeleton>` is the
checkout, `<vault>` the vault, `<dev>` the checkouts folder.

### 1. Prerequisites

`bun`, `git`, `uv` (for graphify), `claude` (Claude Code), `gitleaks` and `cargo` (the Rust
toolchain, for the knowledge binary), all on `PATH`. The hooks commit and push their records on
their own and scan them with gitleaks first. Without it they commit nothing.

| Tool | Linux | Windows |
|---|---|---|
| bun | `curl -fsSL https://bun.sh/install \| bash` | `powershell -c "irm bun.sh/install.ps1 \| iex"` |
| git | your package manager (`pacman -S git`, `apt install git`) | `winget install Git.Git` |
| uv | `curl -LsSf https://astral.sh/uv/install.sh \| sh` | `powershell -c "irm https://astral.sh/uv/install.ps1 \| iex"` |
| claude | `curl -fsSL https://claude.ai/install.sh \| bash` | `irm https://claude.ai/install.ps1 \| iex` |
| gitleaks | your package manager (`pacman -S gitleaks`, `brew install gitleaks`) | `winget install Gitleaks.Gitleaks` |
| cargo | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` | `winget install Rustlang.Rustup` |

`rust-toolchain.toml` pins the compiler (1.100.0); rustup fetches it on the first build. On Windows
rustup also needs the MSVC build tools (Visual Studio Build Tools, "Desktop development with C++").

`CLAUDE_CONFIG_DIR` must be unset: the hooks read `~/.claude`.

### 2. The vault

A new one from the template, or an existing clone (then skip to `core.fileMode`):

```bash
cp -r <skeleton>/template <vault>
echo "{\"version\":\"$(jq -r .version <skeleton>/package.json)\"}" > <vault>/.skeleton.json
git -C <vault> init -b main && git -C <vault> add -A && git -C <vault> commit -m "chore: new vault from knowledge-skeleton"
git -C <vault> config core.fileMode false
```

`.skeleton.json` pins the skeleton version the vault is written for; each session start compares it
with the checkout. An adopted vault without one is not checked: commit the same file there once.

`core.fileMode false`: Obsidian on Linux rewrites `.obsidian/*.json` with mode 755, and a mode-only
change blocks the SessionStart pull. The vault tracks no executables.

The pre-commit secret scan, in the vault and in the skeleton clone:

```bash
git -C <vault> config core.hooksPath <skeleton>/githooks
git -C <skeleton> config core.hooksPath <skeleton>/githooks
```

`githooks/pre-commit` runs `gitleaks git --staged` on every commit, the hooks' own included, and
refuses the commit when gitleaks is missing. It adds about 1.3 s per commit on Windows.

### 3. config.json

`~/.config/knowledge/config.json` on every OS (`$XDG_CONFIG_HOME/knowledge/` when set);
`$KNOWLEDGE_CONFIG` overrides it. On Windows that is `%USERPROFILE%\.config\knowledge\`, not
`%APPDATA%`: a packaged (MSIX) app such as Claude desktop has its `%APPDATA%` writes redirected to
`%LOCALAPPDATA%\Packages\<app>\LocalCache\Roaming`, so a config written from inside it never reaches
a plain terminal. A Windows machine set up before 2026-11-04 moves `%APPDATA%\knowledge\config.json`
there; the hooks name both paths until it does.

```json
{ "vault": "/home/me/knowledge", "devRoot": "/home/me/dev", "os": "arch",
  "machine": "Free text the SessionStart banner opens with.", "rolloverHour": 5 }
```

`os` is what logs and commit notes are stamped with. `rolloverHour` is when a daily note starts,
local time. `"pullTrust": "signed"` turns on pull trust; set it last (§ Signing and pull trust). Create `devRoot` if it does not exist. Check what the hooks see:
`bun --bun <skeleton>/hooks/skeleton-config.mjs`.

### 4. Links into `~/.claude`

```bash
ln -s <skeleton>/hooks ~/.claude/hooks
mkdir -p ~/.claude/skills ~/.claude/commands
ln -s <skeleton>/skills/brain ~/.claude/skills/brain
ln -s <skeleton>/commands/brain-*.md ~/.claude/commands/
```

```powershell
New-Item -ItemType Junction -Path $HOME\.claude\hooks -Target C:\dev\knowledge-skeleton\hooks
New-Item -ItemType Junction -Path $HOME\.claude\skills\brain -Target C:\dev\knowledge-skeleton\skills\brain
Get-ChildItem C:\dev\knowledge-skeleton\commands\brain-*.md | ForEach-Object {
  New-Item -ItemType SymbolicLink -Path "$HOME\.claude\commands\$($_.Name)" -Target $_.FullName }
```

A file symlink on Windows needs Developer Mode (Settings > For developers) or an elevated shell; a
junction needs neither. Move an existing `~/.claude/hooks` folder aside first rather than deleting it.

### 4b. The knowledge binary

```bash
cargo install --path <skeleton> --locked
knowledge --version    # knowledge 0.9.0 / SQLite 3.x, FTS5 on
```

It lands in `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows). A release build takes about a
minute; the C compiler SQLite needs also builds `ring` for `fetch`'s TLS (no CMake, no NASM). Rerun
it after pulling a skeleton whose `src/` changed; `install.ts` does when the source is newer than the
installed binary. Its three files (`vault-index.db`, `repos-index.db`, `captures.db`) live beside
`config.json` unless `KNOWLEDGE_HOME` says otherwise. `fetch` trusts the OS certificate store, so a
machine whose store is out of date fails a TLS handshake rather than falling back to a bundled list.

### 5. The hook block in `~/.claude/settings.json`

[`settings.hooks.json`](settings.hooks.json) is the block. Merge each event's entries into the
`hooks` key, with `{BUN}` replaced by bun's absolute path (`command -v bun`,
`(Get-Command bun).Source`), `{HOOKS}` by `~/.claude/hooks` written out absolute, and `{KNOWLEDGE}`
by the binary's absolute path. Use forward slashes on Windows too. Keep every other hook; plugins
add their own. On Linux, once:

```bash
cd <skeleton> && sed "s#{BUN}#$(command -v bun)#g; s#{HOOKS}#$HOME/.claude/hooks#g; s#{KNOWLEDGE}#$HOME/.cargo/bin/knowledge#g" settings.hooks.json > /tmp/hooks.json
jq --slurpfile b /tmp/hooks.json '.hooks = reduce ($b[0] | to_entries[]) as $e (.hooks // {}; .[$e.key] = ((.[$e.key] // []) + $e.value))' \
  ~/.claude/settings.json > /tmp/settings.json && mv /tmp/settings.json ~/.claude/settings.json
```

Run that once only: it appends. Claude Code reads hooks at session start, so open a new session after.

### 6. Identity and Hard-Rules in `~/.claude/CLAUDE.md`

Both load whole into every session. Add, one import per line:

```markdown
@/home/me/knowledge/00-Meta/Identity.md
@/home/me/knowledge/00-Meta/Hard-Rules.md
```

Never move either behind a link or into a hook: a hook's context is cut to a 2,000-char preview past
10,000 chars. `brain-doctor`'s `priming` check guards it.

### 6b. The rules lock and the guard

The first lock is written once, by the owner, in their own terminal. A seed supplies what the text
cannot (each rule's kind, enforcement points and the guards):

```bash
cd <vault> && knowledge rules lock --seed <seed.json>   # later edits: knowledge rules lock
knowledge rules check                                   # rules: N units match 00-Meta/rules.lock
```

Commit `00-Meta/rules.lock` with Hard-Rules: the session-start restore takes a changed rule back
from `HEAD` only when `HEAD` matches the lock. The PreToolUse entry in step 5's block is the guard.
Without a lock it allows everything, with one stderr line. `install.ts` points `core.hooksPath` at
`githooks/` in the vault, this clone and each checkout a `10-Repos` page names. A checkout's own
`core.hooksPath` is kept as `knowledge.ownHooksPath` and runs first. The hook needs `gitleaks` and
`knowledge` on `PATH`, and refuses the commit without either.

### 7. Compile state

`brain-compile` turns each finished daily log into notes once, remembered by hash in
`<vault>/50-Ops/brain-compile-state.json` (untracked, per machine). An adopted vault has no state on
a new machine, so the first session would recompile every log it ever had. Mark them done:

```bash
cd <skeleton>/hooks && bun --bun -e '
import { createHash } from "node:crypto"; import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path"; import { CONFIG, FOLDERS } from "./skeleton-config.mjs";
const file = join(CONFIG.vault, FOLDERS.ops, "brain-compile-state.json");
const state = existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : {};
for (const log of new Bun.Glob("*/*/*.md").scanSync({ cwd: join(CONFIG.vault, FOLDERS.sessions), absolute: true }))
  state[log] ??= createHash("sha256").update(readFileSync(log, "utf8")).digest("hex");
writeFileSync(file, JSON.stringify(state, null, 1));'
```

This marks today's log too; delete its line if today's session should still be compiled.

### 8. graphify (MIT)

```bash
uv tool install "graphifyy[mcp]"
```

The `[mcp]` extra is required: without it `graphify-mcp` dies on import and Claude Code lists the
server as failed, while every doctor check still passes.

### 9. First index and graph

```bash
knowledge index
bun --bun ~/.claude/hooks/brain-doctor.mjs --repair --pretty
```

The first builds the vault index (about a second for 2,000 notes). The repair records commits made
outside Claude, regenerates the hub MOCs, brings the index level again and merges the repo graphs
into `graphify-out/merged-graph.json`. All of it is derived; run it whenever a check fails.

### 10. graphify MCP, local scope

From inside the vault, so only sessions launched there see it:

```bash
cd <vault> && claude mcp add --scope local graphify -- "$(command -v graphify-mcp)" <vault>/graphify-out/merged-graph.json
claude mcp list    # graphify: ... ✔ Connected
```

### 11. Check

```bash
bun --bun ~/.claude/hooks/brain-doctor.mjs --pretty
```

Every check PASS (`hygiene` may WARN). Then, for a new vault, fill the `<!-- FILL: … -->` placeholders
in `00-Meta/Identity.md` and `00-Meta/Hard-Rules.md` and replace the questions in
`00-Meta/retrieval-cases.json` with ones the vault must always answer.

## Signing and pull trust

Commits from every machine are signed with an SSH key, and each session start pulls only a tip
signed by a key you listed. The key has no passphrase, because the hooks commit with nobody present;
it signs commits and nothing else. In this order, because step 4 is the one that can refuse a pull:

1. **Each machine:** `bun install.ts --signing`. It writes `~/.config/knowledge/signing_ed25519`,
   sets `gpg.format ssh`, `user.signingkey`, `commit.gpgsign` and `tag.gpgsign` in the vault and in
   the skeleton clone, adds this machine's key to `~/.config/knowledge/allowed_signers`, and prints
   the key's fingerprint. Add the public key on GitHub as a **signing** key
   (`gh ssh-key add <key>.pub --type signing`; needs `gh auth refresh -s admin:ssh_signing_key`).
2. **Each machine:** add the other machine's public key to `allowed_signers`, one line each:
   `<email> namespaces="git" ssh-ed25519 AAAA…`. Fetch it from
   `https://api.github.com/users/<you>/ssh_signing_keys`, and compare its fingerprint
   (`ssh-keygen -lf <file>`) with the one the other machine printed before trusting it.
3. **Both machines at the same tip** of the vault and the skeleton, so the first checked pull is a
   signed commit.
4. **Each machine:** `"pullTrust": "signed"` in `config.json`. Then, on GitHub, a ruleset that
   requires signed commits on `main`, once every machine signs.

Only the tip of a pull is checked: a signed tip vouches for the unsigned commits under it. `%G?`
must be `G`; `U` (a good signature from a key not in `allowed_signers`) is refused, and so is a
missing `allowed_signers`. `allowed_signers` is passed on each call, never read from the repo's
config. `brain-doctor`'s `pull-trust` check WARNs until step 4 and FAILs if a piece goes missing.
Undo, at any step: `git config --unset commit.gpgsign` in a clone, or remove `pullTrust`.

## Credential fences

Recommended for `~/.claude/settings.json`; the installer never writes it, since security settings
stay yours:

```json
{
  "permissions": {
    "deny": [
      "Read(~/.ssh/**)",
      "Read(~/.codex/auth.json)",
      "Read(~/.claude/.credentials.json)",
      "Read(**/.env*)",
      "Read(~/.config/knowledge/signing_ed25519)"
    ]
  }
}
```

Merge it into an existing `permissions.deny`. These rules cover the Read tool; a shell command can
still read the files, so keep shell commands on approval.

## Updating

The skeleton fast-forwards itself at each session start when its tree is clean; when its
`settings.hooks.json` changed, that session says to rerun `install.ts`. When its version moves past
the vault's `.skeleton.json`, adapt the vault and bump the pin. The tools never update themselves:

```bash
uv tool upgrade graphifyy
```

## Removing

Delete the links from step 4, the `brain-*` entries from `settings.json`, the two imports from
`CLAUDE.md`, `config.json`, and the graphify MCP (`claude mcp remove --scope local graphify`, from
the vault). The vault stays a folder of Markdown notes.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `graphify: ✘ Failed to connect` in `claude mcp list` | graphify without the `[mcp]` extra: `uv tool install --reinstall "graphifyy[mcp]"` |
| `link FAIL ... Developer Mode` on Windows | A file symlink needs Developer Mode or an elevated shell |
| `ignores` FAIL on a fresh clone | `.graphifyignore` names a folder git did not bring: an empty folder needs a `.gitkeep` |
| `links` FAIL on a fresh clone, pointing at a date | A record links a daily log that was never committed: commit it, or let the flush write it |
| `!! SKELETON x is behind the vault's pin y` at session start | The other machine moved the vault to a newer skeleton: `git -C <skeleton> pull --ff-only` |
| `!! SKELETON x is ahead of the vault's pin y` | The skeleton moved on: read its changes since `y` (`git -C <skeleton> log vy..`), adapt the vault, set `.skeleton.json` to `x`, commit |
| `!! VAULT PULL refused: the tip is not signed by a known key` | Pull trust is on and origin's tip is unsigned or signed by a key not in `allowed_signers`: check who pushed it (`git log --show-signature -1 @{u}`) before adding a key |
| `pre-commit: gitleaks is not on PATH; commit refused` | Install gitleaks (§ 1), or the commit stays unscanned |
| `pin FAIL` from the installer | The adopted vault pins another version: check out that one, or adapt the vault and bump the pin |
| Every hook errors with `no knowledge config` | Step 3 is missing, or `$KNOWLEDGE_CONFIG` points somewhere empty |
