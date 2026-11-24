# Update

Nothing updates on its own. No hook, session start or background task checks for a new version:
an updater that phones home is a telemetry surface, so the check runs only when you ask for it.

## Ask what is new

```
$ knowledge update --plan
knowledge 0.12.0, x86_64-pc-windows-msvc
No release is published yet at https://github.com/atlasatakahraman/knowledge-skeleton/releases.
Update from the checkout instead:
  git -C ~/dev/knowledge-skeleton pull --ff-only
  cargo install --path ~/dev/knowledge-skeleton --locked
  knowledge setup --plan     (then --apply <id>)
```

It makes one HTTPS call, to GitHub's API for the latest release, and sends nothing about you or
your vault. Once a release exists, it names it and prints this platform's steps: the archive,
`SHA256SUMS` and `SHA256SUMS.sig` to download, how to verify them, the folder to extract the two
binaries into, and the plan to run after. It downloads nothing and replaces nothing.
`--apply` is not built: on Windows a running `knowledge mcp` (the Claude desktop app, Codex,
opencode) holds the binary, so the replace is a step you take with those apps quit.

## From the checkout

The skeleton checkout fast-forwards itself at each session start when its tree is clean. After a
pull that changed `src/`, rebuild the binary, then plan:

```bash
cargo install --path <skeleton> --locked
knowledge setup --plan
knowledge setup --apply <id>
```

The hooks run the installed binary, never the build folder, so until the rebuild they run the old
one. The plan moves the Codex and opencode plugins to the new version's pin, and Codex asks for its
hook review again (`/hooks` in the Codex CLI).

## The Claude Code plugin

The marketplace is added with no `autoUpdate`. Once that flag is set, Claude Code copies it into
`~/.claude/plugins/known_marketplaces.json`, where it stays even after it is removed from
`settings.json`. The plugin's version is the binary's, and Claude Code replaces a cached plugin
only when that version changes. It compares against its own clone of the marketplace, which
nothing refreshes on its own, so `claude plugin update` alone reports the old version as the
latest. When a pull changes `claude/`, the session start names the three steps:

```bash
claude plugin marketplace update knowledge-skeleton
claude plugin update knowledge@knowledge-skeleton
```

then `/reload-plugins` in a running session.

## The vault's pin

When the skeleton's version moves past the vault's `.skeleton.json`, the session start says which
side moves. Read what changed since the pin (`git -C <skeleton> log v<pin>..`), adapt the vault,
set `.skeleton.json` to the new version, and commit it.

## From before 0.12.0

The code map replaced graphify in 0.12.0. On a machine set up before it, the next plan removes the
graphify server `setup` registered, in Claude Code and in Codex, and leaves a graphify server of
your own as it is. graphify itself is yours to uninstall: `uv tool uninstall graphifyy`.

How each step was first done: [install.md](install.md). When a step fails:
[troubleshooting.md](troubleshooting.md).
