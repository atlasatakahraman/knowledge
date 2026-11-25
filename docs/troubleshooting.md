# Troubleshooting

Start with `knowledge doctor --pretty`: each FAIL names what it found. `knowledge doctor --repair`
rebuilds everything derived (the index, the hubs, the commit notes) and is safe to run any time.

| Symptom | Cause, and the fix |
|---|---|
| `link FAIL ... Developer Mode` on Windows | A file symlink needs Developer Mode (Settings > For developers) or an elevated shell |
| `setup: the plan changed since --plan` | A target changed after the plan was printed, or the flags differ: plan again and read it |
| A plan line `FAIL` | It names what to fix; a plan with a FAIL has no id to apply |
| `ignores` FAIL on a fresh clone | `.obsidian/app.json`'s `userIgnoreFilters` names a folder git did not bring: an empty folder needs a `.gitkeep` |
| `links` FAIL on a fresh clone, pointing at a date | A record links a daily log that was never committed: commit it, or let the flush write it |
| `vault-index` FAIL, `stale:` or `missing:` | A note changed outside a session and outside the hooks: `knowledge doctor --repair` |
| `!! SKELETON x is behind the vault's pin y` at session start | The other machine moved the vault to a newer skeleton: `git -C <skeleton> pull --ff-only`, then rebuild ([update.md](update.md)) |
| `!! SKELETON x is ahead of the vault's pin y` | The skeleton moved on: read its changes since `y` (`git -C <skeleton> log vy..`), adapt the vault, set `.skeleton.json` to `x`, commit |
| `!! VAULT PULL refused: the tip is not signed by a known key` | Pull trust is on and origin's tip is unsigned or signed by a key not in `allowed_signers`: check who pushed it (`git log --show-signature -1 @{u}`) before adding a key ([security.md](security.md)) |
| `!! VAULT PULL skipped: incoming commits touch files with uncommitted changes` | Origin changed files you also changed and have not committed; git merged nothing. Commit or stash those files, then `git pull --ff-only`. Uncommitted changes elsewhere never block the pull |
| `pre-commit: gitleaks is not on PATH; commit refused` | Install gitleaks ([install.md](install.md)), or the commit stays unscanned |
| `pin FAIL` in the plan | The adopted vault pins another version: check out that one, or adapt the vault and bump the pin |
| Every hook errors with `no knowledge config` | `config.json` is missing, or `$KNOWLEDGE_CONFIG` points somewhere empty ([install.md](install.md)) |
| Every verb stops with a line naming `folders` | `config.json`'s `folders` holds a bad name: one folder per key, no `/`, `\`, `:`, `.` or `..` |
| A commit fails in a hook right after a skeleton pull | The hook calls a verb the installed binary does not have yet: rebuild it ([update.md](update.md)) |
| `this binary is x, but the codex plugin's hooks were written for y, so they stand down` | A new binary and an old plugin: `knowledge setup --plan`, `--apply`, then the Codex hook review |
| `WARN hook trust 0 of 9 hooks trusted in Codex` | The hooks wait for your review: `/hooks` in the Codex CLI |
| `knowledge find` in Codex: `Access denied (os error 5)` on `config.json` | Codex's Windows sandbox: the model should use `mcp__knowledge__find`; `codex mcp list` must show `knowledge` |
| `cargo install` on Windows cannot replace `knowledge.exe` (access denied) | A running `knowledge mcp` holds the file: the Claude desktop app's server, or Codex's or opencode's. Quit the app fully (the tray too), install, start it again; a session inside that app stops with it, its background tasks included |
| opencode runs none of the hooks after setup | The service loaded its plugins before the file existed: `opencode service restart` |
| The desktop Chat lists no `knowledge` tools | The config is in a folder the app left (a Store install reads `%LOCALAPPDATA%\Claude-Data\<package>`), or the app was not fully restarted |
| `auth` FAIL, or the flush times out after 120 s | The harness's CLI is signed out, and a signed-out CLI waits: sign in to it in a terminal (`claude`, then `/login`) |
