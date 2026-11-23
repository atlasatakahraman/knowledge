# Installing knowledge-skeleton

Build the binary, then let it plan the rest; every step the plan takes is also written out below,
so a person or an agent can do it by hand and get the same machine. One plan covers Claude Code,
then every other harness it finds installed (Codex, opencode, the Claude desktop app), each under
its own `found` line; § Other harnesses has each one's steps.

```bash
git clone https://github.com/atlasatakahraman/knowledge-skeleton ~/dev/knowledge-skeleton
cargo install --path ~/dev/knowledge-skeleton --locked
knowledge setup --plan            # what it would write; nothing is written
knowledge setup --apply <id>      # exactly that, where <id> is the plan's last line
```

```powershell
git clone https://github.com/atlasatakahraman/knowledge-skeleton C:\dev\knowledge-skeleton
cargo install --path C:\dev\knowledge-skeleton --locked
knowledge setup --plan
knowledge setup --apply <id>
```

| Flag | Default | |
|---|---|---|
| `--vault <path>` | `config.json`'s, else `~/knowledge` | The vault. No spaces: `CLAUDE.md` imports cannot hold one |
| `--dev <path>` | `config.json`'s, else `~/dev` | Where checkouts live, one folder per GitHub remote |
| `--machine "<text>"` | empty | Free text the SessionStart banner opens with: hardware, dual-boot, dotfiles |
| `--from-template` / `--adopt` | adopt a non-empty path, else template | A new vault from `template/`, or an existing one (it must have `00-Meta/00-MOC-Root.md`) |
| `--signing` | off | Also this boot's commit-signing key and both clones signing (§ Signing and pull trust) |
| `--skeleton <checkout>` | the one `~/.claude/hooks` links, else the one the binary was built in | The skeleton the links point into |
| `--harness <id>` | every harness found | One harness alone: `claude`, `codex`, `opencode`, `claude-desktop` or `generic` |
| `--instructions <file>` | none | With `--harness generic`: the file the vault block is written into |

Each line of the plan is `ok` (already so), `create`, `change`, `WARN` or `FAIL`, with the file it
names. Its last line is the plan id: a hash of the plan and of each target's content now.
`--apply <id>` takes the same flags, recomputes the plan and writes nothing if the id differs, so a
file that changed after the plan was read is never overwritten unseen. A plan with a FAIL has no id
to apply: fix what it says and plan again. Nothing is deleted: a real file where a link belongs moves
to `~/.claude/.pre-skeleton/`, and `settings.json` is copied to `settings.json.pre-skeleton-<hash>`
before a change. A second `--plan` on a set-up machine says `Nothing to change.`

## For an agent

- Run `--plan` and show the output to the person; apply its id only on their yes.
- Never edit a hook in `~/.claude/settings.json` that is not this engine's, and never commit the
  vault for them. The first commit of a template vault is setup's, and only when git has an
  identity.
- A FAIL line is the answer to report: quote it, do not work around it.
- A WARN that names a missing tool (`claude`, `uv`, `gitleaks`) carries the command that installs
  it. Installing one is the person's call: ask first.

## What it does, by hand

Paths below are Linux; the Windows form follows each block where it differs. `<skeleton>` is the
checkout, `<vault>` the vault, `<dev>` the checkouts folder.

### 1. Prerequisites

`git`, `uv` (for graphify), `claude` (Claude Code), `gitleaks`, `cargo` (the Rust toolchain, for
the knowledge binary) and `bun` (the skills' and commands' tools), all on `PATH`. The hooks commit
and push their records on their own and scan them with gitleaks first. Without it they commit
nothing.

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
mkdir -p ~/.agents/skills && ln -s <skeleton>/skills/brain ~/.agents/skills/brain
```

```powershell
New-Item -ItemType Junction -Path $HOME\.claude\hooks -Target C:\dev\knowledge-skeleton\hooks
New-Item -ItemType Junction -Path $HOME\.claude\skills\brain -Target C:\dev\knowledge-skeleton\skills\brain
Get-ChildItem C:\dev\knowledge-skeleton\commands\brain-*.md | ForEach-Object {
  New-Item -ItemType SymbolicLink -Path "$HOME\.claude\commands\$($_.Name)" -Target $_.FullName }
New-Item -ItemType Directory -Force $HOME\.agents\skills | Out-Null
New-Item -ItemType Junction -Path $HOME\.agents\skills\brain -Target C:\dev\knowledge-skeleton\skills\brain
```

A file symlink on Windows needs Developer Mode (Settings > For developers) or an elevated shell; a
junction needs neither. Move an existing `~/.claude/hooks` folder aside first rather than deleting it.
`~/.agents/skills` is the skills folder Codex, opencode, Gemini CLI, Cursor, Amp and Zed read, so
`/brain` reaches them through the same link; opencode reads both folders and loads the skill once.

### 4b. The knowledge binary

```bash
cargo install --path <skeleton> --locked
knowledge --version    # knowledge 0.11.2 / SQLite 3.x, FTS5 on
```

It lands in `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows). A release build takes about a
minute; the C compiler SQLite needs also builds `ring` for `fetch`'s TLS (no CMake, no NASM). Rerun
it after pulling a skeleton whose `src/` changed: the hooks run the installed copy, never the build
folder, and `setup` WARNs when the binary it would register is a build folder's. Its three files (`vault-index.db`, `repos-index.db`, `captures.db`) live beside
`config.json` unless `KNOWLEDGE_HOME` says otherwise. `fetch` trusts the OS certificate store, so a
machine whose store is out of date fails a TLS handshake rather than falling back to a bundled list.

### 5. The Claude Code plugin

The hooks ship as a plugin, `claude/`, served by this repo's own marketplace
(`.claude-plugin/marketplace.json`). Every hook is exec form: the command is
`${user_config.knowledge_bin}` with `["hook", <verb>, "--harness", "claude"]` as its arguments, so
no shell starts per hook and no other `knowledge` earlier on `PATH` can be picked.

```bash
claude plugin marketplace add https://github.com/atlasatakahraman/knowledge-skeleton
claude plugin install knowledge@knowledge-skeleton --scope user
```

Then the plugin's one option, in `~/.claude/settings.json`, beside every other key:

```json
{ "pluginConfigs": { "knowledge@knowledge-skeleton": {
    "options": { "knowledge_bin": "/home/me/.cargo/bin/knowledge" } } } }
```

On Windows the path keeps forward slashes: `"C:/Users/me/.cargo/bin/knowledge.exe"`. Claude Code
reads hooks at session start, so open a new session after.

- **Updates are never automatic.** The marketplace is added with no `autoUpdate`: once that flag is
  set, Claude Code copies it into `~/.claude/plugins/known_marketplaces.json`, where it stays even
  after it is removed from `settings.json`. The plugin's version is the binary's, and Claude Code
  replaces a cached plugin only when that version changes. When a pull changes `claude/`, the
  session start names `claude plugin update knowledge@knowledge-skeleton`, then `/reload-plugins`.
- **From 0.10.0's hook block.** `setup` takes every hook of ours out of `settings.json` (a
  `knowledge … hook` command, or a `brain-*.mjs` under `~/.claude/hooks`) after the plugin installs,
  and keeps every other hook. If the install fails, the apply stops and the old hooks keep running.
  `settings.hooks.json` is that old block; only `install.ts` still reads it, for a boot not yet
  moved to the binary.

### 6. Identity and Hard-Rules in `~/.claude/CLAUDE.md`

Both load whole into every session. Add, one import per line:

```markdown
@/home/me/knowledge/00-Meta/Identity.md
@/home/me/knowledge/00-Meta/Hard-Rules.md
```

Never move either behind a link or into a hook: a hook's context is cut to a 2,000-char preview past
10,000 chars. The doctor's `priming` check guards it. `setup` writes the two lines between
`<!-- knowledge-skeleton:begin -->` and `<!-- knowledge-skeleton:end -->`, so `--remove` can take
out exactly that; text outside the markers is never touched. The same generator writes the other
harnesses' blocks (§ Other harnesses), and each session start restores a block that went stale.

### 6b. The rules lock and the guard

The first lock is written once, by the owner, in their own terminal. A seed supplies what the text
cannot (each rule's kind, enforcement points and the guards):

```bash
cd <vault> && knowledge rules lock --seed <seed.json>   # later edits: knowledge rules lock
knowledge rules check                                   # rules: N units match 00-Meta/rules.lock
```

Commit `00-Meta/rules.lock` with Hard-Rules: the session-start restore takes a changed rule back
from `HEAD` only when `HEAD` matches the lock. The plugin's PreToolUse hook (step 5) is the guard.
Without a lock it allows everything, with one stderr line. `setup` points `core.hooksPath` at
`githooks/` in the vault, this clone and each checkout a `10-Repos` page names. A checkout's own
`core.hooksPath` is kept as `knowledge.ownHooksPath` and runs first. The hook needs `gitleaks` and
`knowledge` on `PATH`, and refuses the commit without either.

### 7. Compile state

`knowledge compile` turns each finished daily log into notes once, remembered by hash in
`<vault>/50-Ops/brain-compile-state.json` (untracked, per machine). An adopted vault has no state on
a new machine, so the first session would recompile every log it ever had. `setup` marks every log
but today's done: each log's absolute path, as the OS writes it, maps to the SHA-256 of its text.
By hand, the same with `bun`:

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

This one marks today's log too; delete its line if today's session should still be compiled.

### 8. graphify (MIT)

```bash
uv tool install "graphifyy[mcp]"
```

The `[mcp]` extra is required: without it `graphify-mcp` dies on import and Claude Code lists the
server as failed, while every doctor check still passes. `setup` does not install it: its plan WARNs
and names this command, and registers the server (step 10) once `graphify-mcp` is on `PATH`.

### 9. First index and graph

```bash
knowledge index
knowledge doctor --repair --pretty
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
knowledge doctor --pretty
```

Every check PASS (`hygiene` may WARN). Then, for a new vault, fill the `<!-- FILL: … -->` placeholders
in `00-Meta/Identity.md` and `00-Meta/Hard-Rules.md` and replace the questions in
`00-Meta/retrieval-cases.json` with ones the vault must always answer. Last, the rules interview:
`knowledge rules list` shows the packs, and `knowledge rules enable <pack>` turns one's guards on.

Each session start runs the full doctor once per logical day, detached, with `--repair --record`;
a FAIL opens the next session with a `!! VAULT HEALTH FAIL` line. No scheduled task is needed.

## Other harnesses

`setup --plan` reaches each of these when its adapter (`adapters/<id>.json`, built into the binary)
detects it, after the Claude part has written `config.json`; `--harness <id>` plans one alone. Each
one's instructions block comes from the same generator as step 6. Codex and opencode cannot import
a file, so their block holds the retrieval steps, then Identity and Hard-Rules written whole. It has
no by-hand form: `setup` writes it, and each session start restores it.

### Codex

Detected by `~/.codex` with `codex` on `PATH`. A Codex plugin has no option to carry an absolute
path, so its hooks run the `knowledge` on `PATH`: `setup` FAILs without one, and WARNs when it is
not the binary that is planning.

```bash
codex plugin marketplace add <skeleton>
codex plugin add knowledge@knowledge-skeleton
codex mcp add knowledge -- "$(command -v knowledge)" mcp
codex mcp add graphify -- "$(command -v graphify-mcp)" <vault>/graphify-out/merged-graph.json
```

```powershell
codex plugin marketplace add C:\dev\knowledge-skeleton
codex plugin add knowledge@knowledge-skeleton
codex mcp add knowledge -- (Get-Command knowledge).Source mcp
codex mcp add graphify -- (Get-Command graphify-mcp).Source <vault>\graphify-out\merged-graph.json
```

The block goes into `~/.codex/AGENTS.md` (`%USERPROFILE%\.codex\AGENTS.md`). That global file does
not count toward Codex's 32 KiB `project_doc_max_bytes`, so the cap stays as it is.

- **Hook trust is yours.** Codex runs a plugin hook only after you review it: `/hooks` in the Codex
  CLI. Trust is kept against each hook's hash, so every release asks again. `setup` never writes
  trust: the plan's `hook trust` line and the doctor's `codex-trust` read Codex's own count
  (`codex app-server`, `hooks/list`), and WARN while a hook waits for review. The ChatGPT app's
  Codex runs only hooks already trusted in the CLI and skips the rest without a word.
- **The pin.** Each hook runs `knowledge hook <verb> --harness codex --pin <version>`. A binary of
  another version stands down, and the session start says so in one line. After a new binary,
  `knowledge setup --plan` and `--apply` add the plugin again (a local marketplace has no upgrade
  command); then review the hooks again.
- **The guard denies; it cannot ask.** Codex has no ask, so an ask rule is a deny there and its
  reason says so. An `apply_patch` call is judged by the paths in its patch text.
- **The sandbox.** Codex's elevated Windows sandbox cannot read `~/.config/knowledge`, which also
  holds the signing key, so that folder gets no read grant. Retrieval inside a session goes through
  the `knowledge` MCP server instead: `mcp__knowledge__find` and `mcp__knowledge__graph`, read-only,
  so Codex asks nothing before a call. The block names those tools first.
- **The flush** runs `codex exec --ignore-user-config -m gpt-6-luna -s read-only
  --skip-git-repo-check --json -`, which loads no plugin hook and no MCP server. The global
  `AGENTS.md` still loads: about 30k input tokens a flush.

### opencode

Detected by `~/.config/opencode` with `opencode` on `PATH`. Two files: the plugin, a shim that only
spawns `knowledge hook <verb> --harness opencode` and acts on the reply, and the block in
`~/.config/opencode/AGENTS.md`. opencode loads every `.ts` in its plugins folder, with no config
entry, in its background service, so restart the service after.

```bash
mkdir -p ~/.config/opencode/plugins && cp <skeleton>/opencode/knowledge.ts ~/.config/opencode/plugins/
opencode service restart
```

```powershell
New-Item -ItemType Directory -Force $HOME\.config\opencode\plugins | Out-Null
Copy-Item C:\dev\knowledge-skeleton\opencode\knowledge.ts $HOME\.config\opencode\plugins\
opencode service restart
```

- `setup` writes the copy built into the binary, pinned to its version, and never overwrites or
  removes a `knowledge.ts` it did not write.
- **The guard** runs in `execute.before` on `shell`, `edit`, `write` and `patch`, and blocks by
  throwing. An ask rule is a deny, as in Codex.
- **The flush** has no transcript file to read: it exports the session (`opencode session export
  --standalone`) and runs `opencode run --standalone -m opencode/tidepool-alpha-free --format json`.
  The model is free, and `--standalone` keeps the flush's marker out of the shared service, where it
  would stand the plugin down in every live session.

### Claude desktop app (Chat)

The Chat has no shell and no hooks, so it gets the vault through `knowledge mcp`: `find`, `graph`
and `read` (one note by name), all read-only, and `note`, which writes and so asks first. Detected by
`~/AppData/Roaming/Claude` or `~/Library/Application Support/Claude`. One key goes into
`claude_desktop_config.json`, every other key kept:

```json
{ "mcpServers": { "knowledge": { "command": "/home/me/.cargo/bin/knowledge", "args": ["mcp"] } } }
```

| Install | The file |
|---|---|
| macOS | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| Windows, installer | `%APPDATA%\Claude\claude_desktop_config.json` |
| Windows, Store (MSIX) | `%LOCALAPPDATA%\Claude-Data\<package>\claude_desktop_config.json` |

A Store install leaves only a `.user-data-left.<package>` marker in `%APPDATA%\Claude` and reads the
moved folder; `setup` follows the marker, and WARNs "start the app once" while that folder does not
exist. Quit the app fully and start it again after. A `knowledge` server `setup` did not write makes
the plan FAIL rather than replace it. The desktop app's Code tab passes the same server into its
sessions as `mcp__knowledge__*`, so the Claude plugin carries no server of its own.

### Any other harness (`generic`)

```bash
knowledge setup --harness generic --instructions <file> --plan
knowledge setup --harness generic --instructions <file> --apply <id>
```

The block goes into any file the harness reads, with one more line: no hook records this session,
so write back with `knowledge note decision "<text>"` or `knowledge note log "<text>"`. A harness
with a shell then has `knowledge find` and `knowledge graph`; one that speaks MCP and has no shell
takes `knowledge mcp` as the desktop app does. Nothing restores this block at session start: plan
again to check it.

## Signing and pull trust

Commits from every machine are signed with an SSH key, and each session start pulls only a tip
signed by a key you listed. The key has no passphrase, because the hooks commit with nobody present;
it signs commits and nothing else. In this order, because step 4 is the one that can refuse a pull:

1. **Each machine:** `knowledge setup --plan --signing`, then `--apply` its id. It writes `~/.config/knowledge/signing_ed25519`,
   sets `gpg.format ssh`, `user.signingkey`, `commit.gpgsign` and `tag.gpgsign` in the vault and in
   the skeleton clone, and adds this machine's key to `~/.config/knowledge/allowed_signers`.
   `ssh-keygen -lf ~/.config/knowledge/signing_ed25519.pub` prints its fingerprint. Add the public key on GitHub as a **signing** key
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
config. The doctor's `pull-trust` check WARNs until step 4 and FAILs if a piece goes missing.
Undo, at any step: `git config --unset commit.gpgsign` in a clone, or remove `pullTrust`.

## Credential fences

Recommended for `~/.claude/settings.json`; `setup` never writes it, since security settings stay
yours:

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

The skeleton fast-forwards itself at each session start when its tree is clean; when its `claude/`
plugin changed, that session names `claude plugin update knowledge@knowledge-skeleton`. After a
pull that changed `src/`, rebuild the binary (§ 4b), then `knowledge setup --plan` and `--apply`:
that moves the Codex and opencode plugins to the new pin, and Codex asks for its hook review again.
When the version moves past the vault's `.skeleton.json`, adapt the vault and bump the pin. The
tools never update themselves:

```bash
uv tool upgrade graphifyy
```

## Removing

```bash
knowledge setup --remove --plan
knowledge setup --remove --apply <id>
```

It takes out the links from step 4, the Claude plugin, its marketplace and its option in
`settings.json` (every other key and hook stays), the block `setup` wrote in `CLAUDE.md`, the
githooks path (a checkout's own comes back), and the graphify MCP entry. For each other harness
found it takes out what § Other harnesses added: the Codex plugin, its marketplace, the
`knowledge` and graphify servers setup registered, and the trust tables `codex plugin remove` leaves in `config.toml` (kept beside
it first); opencode's `knowledge.ts`; the desktop app's `mcpServers.knowledge`; and each `AGENTS.md`
block. A `generic` block goes with `--harness generic --instructions <file> --remove`. It never
touches the vault, and keeps `config.json`, the compile state and any signing key: delete those by
hand if you want them gone. The vault stays a folder of Markdown notes. Obsidian's graph colours
stay too: they are Obsidian's view state.

## Troubleshooting

| Symptom | Cause |
|---|---|
| `graphify: ✘ Failed to connect` in `claude mcp list` | graphify without the `[mcp]` extra: `uv tool install --reinstall "graphifyy[mcp]"` |
| `link FAIL ... Developer Mode` on Windows | A file symlink needs Developer Mode or an elevated shell |
| `setup: the plan changed since --plan` | A target changed after the plan was printed (or the flags differ): plan again and read it |
| `ignores` FAIL on a fresh clone | `.graphifyignore` names a folder git did not bring: an empty folder needs a `.gitkeep` |
| `links` FAIL on a fresh clone, pointing at a date | A record links a daily log that was never committed: commit it, or let the flush write it |
| `!! SKELETON x is behind the vault's pin y` at session start | The other machine moved the vault to a newer skeleton: `git -C <skeleton> pull --ff-only` |
| `!! SKELETON x is ahead of the vault's pin y` | The skeleton moved on: read its changes since `y` (`git -C <skeleton> log vy..`), adapt the vault, set `.skeleton.json` to `x`, commit |
| `!! VAULT PULL refused: the tip is not signed by a known key` | Pull trust is on and origin's tip is unsigned or signed by a key not in `allowed_signers`: check who pushed it (`git log --show-signature -1 @{u}`) before adding a key |
| `pre-commit: gitleaks is not on PATH; commit refused` | Install gitleaks (§ 1), or the commit stays unscanned |
| `pin FAIL` in the plan | The adopted vault pins another version: check out that one, or adapt the vault and bump the pin |
| Every hook errors with `no knowledge config` | Step 3 is missing, or `$KNOWLEDGE_CONFIG` points somewhere empty |
| `this binary is x, but the codex plugin's hooks were written for y, so they stand down` | A new binary and an old plugin: `knowledge setup --plan`, `--apply`, then the Codex hook review |
| `WARN hook trust 0 of 9 hooks trusted in Codex` | The hooks wait for your review: `/hooks` in the Codex CLI |
| `knowledge find` in Codex: `Access denied (os error 5)` on `config.json` | Codex's Windows sandbox: the model should use `mcp__knowledge__find`; `codex mcp list` must show `knowledge` |
| opencode runs none of the hooks after setup | The service loaded its plugins before the file existed: `opencode service restart` |
| The desktop Chat lists no `knowledge` tools | The config is in a folder the app left (a Store install reads `%LOCALAPPDATA%\Claude-Data\<package>`), or the app was not fully restarted |
