# Update

Nothing updates on its own. No hook, session start or background task checks for a new version:
an updater that phones home is a telemetry surface, so the check runs only when you ask for it.

## Ask what is new

```
$ knowledge update --plan
knowledge 0.13.1, x86_64-pc-windows-msvc
latest release: v0.13.1  https://github.com/atlasatakahraman/knowledge/releases/tag/v0.13.1
This binary is the latest release. Nothing to do.
```

Before the first release it named the checkout's steps instead (`pull --ff-only`, `cargo install`,
then the plan).

It makes one HTTPS call, to GitHub's API for the latest release, and sends nothing about you or
your vault. Once a release exists, it names it and prints this platform's steps: the archive,
`SHA256SUMS` and `SHA256SUMS.sig` to download, [verify.md](verify.md)'s three checks, the folder to
extract the two binaries into, the checkout's move to the new tag (a checkout installed from a
release sits on its tag, and its plugins must carry the binary's version or the hooks stand down),
and the plan to run after. It downloads nothing and replaces nothing. 0.13.0 prints no checkout
step; from it, run `git -C <checkout> fetch --tags` and `git -C <checkout> checkout v<new>` before
the plan.
The release key it prints is the one built into your binary, so the new release is checked against
a key you already trust rather than one fetched beside the archive.
`--apply` is not built: on Windows a running `knowledge mcp` (the Claude desktop app, Codex,
opencode) holds the binary, so the replace is a step you take with those apps quit.

## From the checkout

The skeleton checkout fast-forwards itself at each session start, unless origin changed a file you
have uncommitted changes in. After a
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
claude plugin marketplace update knowledge
claude plugin update knowledge@knowledge
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
