# Install

Build the binary, then let it plan the rest. Every step the plan takes is also written out below,
so a person or an agent can do it by hand and get the same machine. One plan covers Claude Code,
then every other harness it finds installed (Codex, opencode, the Claude desktop app), each under
its own `found` line. An agent installing for someone follows [AGENTS.md](../AGENTS.md) instead.

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

A plan on a new machine, for Claude Code alone (`--harness claude`), with the home folder shown as
`~`:

```
knowledge 0.12.0 setup plan: nothing is written

  ok      prerequisites    git · claude · gitleaks
  ok      skeleton         ~/dev/knowledge-skeleton (0.12.0)
  create  vault            ~/knowledge: new, from template/ (31 files), .skeleton.json pins 0.12.0, git init, a first commit when git knows your user.email
  change  core.fileMode    ~/knowledge/.git/config: false (Obsidian's mode-only rewrites would block the pull)
  change  hooksPath        ~/knowledge/.git/config: -> ~/dev/knowledge-skeleton/githooks (secret scan, rules backstops)
  change  hooksPath        ~/dev/knowledge-skeleton/.git/config: -> ~/dev/knowledge-skeleton/githooks (secret scan, rules backstops)
  create  config           ~/.config/knowledge/config.json: vault ~/knowledge, devRoot ~/dev, os windows
  create  link             ~/.claude/hooks -> ~/dev/knowledge-skeleton/hooks
  create  link             ~/.claude/skills/brain -> ~/dev/knowledge-skeleton/skills/brain
  create  link             ~/.agents/skills/brain -> ~/dev/knowledge-skeleton/skills/brain
  create  link             ~/.claude/commands/brain-commits.md -> ~/dev/knowledge-skeleton/commands/brain-commits.md
  create  link             ~/.claude/commands/brain-log.md -> ~/dev/knowledge-skeleton/commands/brain-log.md
  create  link             ~/.claude/commands/brain-sync.md -> ~/dev/knowledge-skeleton/commands/brain-sync.md
  create  link             ~/.claude/commands/brain-usage.md -> ~/dev/knowledge-skeleton/commands/brain-usage.md
  ok      knowledge        ~/.cargo/bin/knowledge.exe (0.12.0)
  ok      plugin           knowledge@knowledge-skeleton 0.12.0
  create  settings.json    ~/.claude/settings.json: our 0 hooks out (the plugin carries them), knowledge_bin = ~/.cargo/bin/knowledge.exe
  create  CLAUDE.md        ~/.claude/CLAUDE.md: imports ~/knowledge/00-Meta/Identity.md, ~/knowledge/00-Meta/Hard-Rules.md
  ok      compile state    a new vault has no finished log

After apply:
  knowledge doctor --repair --pretty: the first index, hubs and commit notes
  knowledge rules list --json, then knowledge rules enable <pack> for each pack you want: the rules interview
  fill the FILL: placeholders in ~/knowledge/00-Meta, then start Claude Code in ~/knowledge

14 change(s). Apply exactly these: knowledge setup --apply 1a5be68145b5b7a2 --harness claude
plan 1a5be68145b5b7a2
```

The apply ends with `Done.`, and a second `--plan` on the same machine says `Nothing to change.`

| Flag | Default | |
|---|---|---|
| `--vault <path>` | `config.json`'s, else `~/knowledge` | The vault. No spaces: `CLAUDE.md` imports cannot hold one |
| `--dev <path>` | `config.json`'s, else `~/dev` | Where checkouts live, one folder per GitHub remote |
| `--machine "<text>"` | empty | Free text the SessionStart banner opens with: hardware, dual-boot, dotfiles |
| `--from-template` / `--adopt` | adopt a non-empty path, else template | A new vault from `template/`, or an existing one (it must have `00-Meta/00-MOC-Root.md`) |
| `--signing` | off | Also this boot's commit-signing key and both clones signing ([security.md](security.md)) |
| `--skeleton <checkout>` | the one `~/.claude/hooks` links, else the one the binary was built in | The skeleton the links point into |
| `--harness <id>` | every harness found | One harness alone: `claude`, `codex`, `opencode`, `claude-desktop` or `generic` |
| `--instructions <file>` | none | With `--harness generic`: the file the vault block is written into |

Each line of the plan is `ok` (already so), `create`, `change`, `WARN` or `FAIL`, with the file it
names. Its last line is the plan id: a hash of the plan and of each target's content now.
`--apply <id>` takes the same flags, recomputes the plan and writes nothing if the id differs, so a
file that changed after the plan was read is never overwritten unseen. A plan with a FAIL has no id
to apply: fix what it says and plan again. Nothing is deleted: a real file where a link belongs moves
to `~/.claude/.pre-skeleton/`, and `settings.json` is copied to `settings.json.pre-skeleton-<hash>`
before a change.

Then: [update.md](update.md) to update, [remove.md](remove.md) to take it out,
[security.md](security.md) for signing, pull trust and the credential fences,
[verbs.md](verbs.md) for every command, [troubleshooting.md](troubleshooting.md) when a line FAILs.

## What it does, by hand

Paths below are Linux; the Windows form follows each block where it differs. `<skeleton>` is the
checkout, `<vault>` the vault, `<dev>` the checkouts folder.

### 1. Prerequisites

`git`, `claude` (Claude Code), `gitleaks` and `cargo` (the Rust toolchain, for the knowledge
binary), all on `PATH`. The hooks commit and push their records on their own and scan them with
gitleaks first. Without it they commit nothing.

| Tool | Linux | Windows |
|---|---|---|
| git | your package manager (`pacman -S git`, `apt install git`) | `winget install Git.Git` |
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
echo '{"version":"0.12.0"}' > <vault>/.skeleton.json     # the version `knowledge --version` prints
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
`$KNOWLEDGE_CONFIG` overrides it, and `$KNOWLEDGE_HOME` moves the whole folder. On Windows that is
`%USERPROFILE%\.config\knowledge\`, not `%APPDATA%`: a packaged (MSIX) app such as Claude desktop
has its `%APPDATA%` writes redirected to `%LOCALAPPDATA%\Packages\<app>\LocalCache\Roaming`, so a
config written from inside it never reaches a plain terminal.

```json
{ "vault": "/home/me/knowledge", "devRoot": "/home/me/dev", "os": "arch",
  "machine": "Free text the SessionStart banner opens with.", "rolloverHour": 5 }
```

`vault` and `devRoot` are required; create `devRoot` if it does not exist. `os` is what logs and
commit notes are stamped with. `rolloverHour` (default 5) is when a daily note starts, local time.
`"pullTrust": "signed"` turns on pull trust; set it last ([security.md](security.md)).

A vault laid out differently names its folders in `folders`; a key left out keeps the default:

```json
{ "folders": { "meta": "Meta", "sessions": "Journal" } }
```

The keys are `meta` (`00-Meta`), `repos` (`10-Repos`), `knowledge` (`20-Knowledge`), `sessions`
(`30-Sessions`), `plans` (`40-Plans`), `ops` (`50-Ops`), `decisions` (`60-Decisions`), `assets`
(`80-Assets`) and `inbox` (`90-Inbox`). A name is one folder at the vault's root: no `/`, `\`, `:`,
`.` or `..`, and no two keys naming one folder. A bad `folders` stops every verb with a line naming
it.

Check what the hooks see (Windows, the home folder shown as `~`):

```
$ knowledge config
{
  "devRoot": "~\\dev",
  "home": "~\\.config\\knowledge",
  "machine": "",
  "os": "windows",
  "pullTrust": "",
  "rolloverHour": 5,
  "skeleton": "~\\dev\\knowledge-skeleton",
  "source": "~\\.config\\knowledge\\config.json",
  "trustDir": "~\\.config\\knowledge",
  "vault": "~\\knowledge"
}
```

`knowledge config <key>` prints one value, and a folder key prints the name in effect.

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

### 5. The knowledge binary

```
$ cargo install --path <skeleton> --locked
$ knowledge --version
knowledge 0.12.0
SQLite 3.54.1, FTS5 on
```

It lands in `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows), with `knowledge-code`, the code
map, beside it: `knowledge` hands `find --code`, `graph --code` and `code` to it, and names this
command when it is missing. A clean release build takes two to three minutes; the C compiler SQLite
needs also builds `ring` for `fetch`'s TLS (no CMake, no NASM) and the tree-sitter grammars. Rerun
it after pulling a skeleton whose `src/` changed: the hooks run the installed copy, never the build
folder, and `setup` WARNs when the binary it would register is a build folder's.

Its files live in `~/.config/knowledge` beside `config.json`, unless `KNOWLEDGE_HOME` says
otherwise: `vault-index.db`, `repos-index.db`, `captures.db`, the code maps (`code/<checkout>.db`)
and the hooks' state (`state/`). `fetch` trusts the OS certificate store, so a machine whose store
is out of date fails a TLS handshake rather than falling back to a bundled list.

### 6. The Claude Code plugin

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

The marketplace is added with no `autoUpdate`: updates are never automatic ([update.md](update.md)).
`setup` also takes every hook of ours out of `settings.json` that an install before the plugin wrote
(a `knowledge … hook` command, or a `brain-*.mjs` under `~/.claude/hooks`), after the plugin
installs, and keeps every other hook. If the install fails, the apply stops and the old hooks keep
running.

### 7. Identity and Hard-Rules in `~/.claude/CLAUDE.md`

Both load whole into every session. Add, one import per line:

```markdown
@/home/me/knowledge/00-Meta/Identity.md
@/home/me/knowledge/00-Meta/Hard-Rules.md
```

Never move either behind a link or into a hook: a hook's context is cut to a 2,000-char preview past
10,000 chars. The doctor's `priming` check guards it. `setup` writes the two lines between
`<!-- knowledge-skeleton:begin -->` and `<!-- knowledge-skeleton:end -->`, so `--remove` can take
out exactly that; text outside the markers is never touched. The same generator writes the other
harnesses' blocks (below), and each session start restores a block that went stale.

### 8. The rules lock and the guard

A template vault ships `00-Meta/rules.lock` for its generic packs. For rules you write yourself,
the lock is re-stamped by you, or by your agent once the `rules-files` guard has asked you; a seed
supplies what the text cannot (each rule's kind, enforcement points and the guards):

```bash
cd <vault> && knowledge rules lock --seed <seed.json>   # later edits: knowledge rules lock
knowledge rules check                                   # rules: N units match 00-Meta/rules.lock
```

Commit `00-Meta/rules.lock` with Hard-Rules: the session-start restore takes a changed rule back
from `HEAD` only when `HEAD` matches the lock. The plugin's PreToolUse hook (step 6) is the guard.
Without a lock it allows everything, with one stderr line. `setup` points `core.hooksPath` at
`githooks/` in the vault, this clone and each checkout a `10-Repos` page names. A checkout's own
`core.hooksPath` is kept as `knowledge.ownHooksPath` and runs first. The hook needs `gitleaks` and
`knowledge` on `PATH`, and refuses the commit without either. What the guard can and cannot stop:
[security.md](security.md).

### 9. Compile state

`knowledge compile` turns each finished daily log into notes once, remembered by hash in
`<vault>/50-Ops/brain-compile-state.json` (untracked, per machine). An adopted vault has no state on
a new machine, so the first session would recompile every log it ever had. `setup` marks every log
but today's done: the plan's `compile state` line reads `<file>: N finished log(s) marked
compiled`. This step has no by-hand form: run the plan, even for this line alone. A new vault has
no finished log, so the line reads `ok`.

### 10. First index, and the check

```
$ knowledge doctor --repair --pretty
knowledge doctor  PASS  2026-11-24T10:49:50.767Z
  PASS  vault-index
  PASS  retrieval
  PASS  sessions
  PASS  auth
  PASS  frontmatter
  PASS  links
  PASS  pointers
  PASS  reach
  PASS  ignores
  PASS  commits
  PASS  priming
  PASS  priming-size
  PASS  rules
  PASS  budget
  PASS  skeleton-version
  WARN  hygiene
  WARN  pull-trust
  PASS  codex-trust
  PASS  portable
```

The repair builds the vault index (about a second for 2,000 notes), records commits made outside a
session, regenerates the hub MOCs and brings the index level again. All of it is derived; run it
whenever a check fails. A WARN leaves the doctor at PASS: `hygiene` WARNs while the vault has
uncommitted work, and `pull-trust` until it is set up ([security.md](security.md)). The code map needs no step: each checkout's map is
built by the first `knowledge find <symbol> --code` or `graph <symbol> --code` that names it, and
brought level by every one after.

Then, for a new vault, fill the `<!-- FILL: … -->` placeholders in `00-Meta/Identity.md` and
`00-Meta/Hard-Rules.md`, and replace the questions in `00-Meta/retrieval-cases.json` with ones the
vault must always answer. Last, the rules interview: `knowledge rules list` shows the packs, and
`knowledge rules enable <pack>` turns one's guards on.

Each session start runs the full doctor once per logical day, detached, with `--repair --record`;
a FAIL opens the next session with a `!! VAULT HEALTH FAIL` line. No scheduled task is needed.

## Other harnesses

`setup --plan` reaches each of these when its adapter (`adapters/<id>.json`, built into the binary)
detects it, after the Claude part has written `config.json`; `--harness <id>` plans one alone. Each
one's instructions block comes from the same generator as step 7. Codex and opencode cannot import
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
```

```powershell
codex plugin marketplace add C:\dev\knowledge-skeleton
codex plugin add knowledge@knowledge-skeleton
codex mcp add knowledge -- (Get-Command knowledge).Source mcp
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
