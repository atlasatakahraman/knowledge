# Remove

```bash
knowledge setup --remove --plan
knowledge setup --remove --apply <id>
```

Like the install, the plan writes nothing and the apply refuses if anything moved in between. A
plan for Claude Code alone (`--harness claude`), with the home folder shown as `~`:

```
knowledge 0.13.2 remove plan: nothing is written

  remove  settings.json    ~/.claude/settings.json: our 0 hooks and the plugin's option removed, every other hook kept (the old file kept as ~/.claude/settings.json.pre-skeleton-0588b9aa)
  remove  plugin           claude plugin uninstall knowledge@knowledge (0.13.2), then marketplace remove knowledge
  remove  CLAUDE.md        ~/.claude/CLAUDE.md: only the imports section setup wrote, so the file goes
  remove  link             ~/.claude/skills/knowledge
  remove  link             ~/.agents/skills/knowledge
  remove  link             ~/.claude/skills/knowledge-commits
  remove  link             ~/.agents/skills/knowledge-commits
  remove  link             ~/.claude/skills/knowledge-log
  remove  link             ~/.agents/skills/knowledge-log
  remove  link             ~/.claude/skills/knowledge-sync
  remove  link             ~/.agents/skills/knowledge-sync
  remove  link             ~/.claude/skills/knowledge-usage
  remove  link             ~/.agents/skills/knowledge-usage
  remove  hooksPath        ~/knowledge/.git/config: core.hooksPath unset
  remove  hooksPath        ~/dev/knowledge/.git/config: core.hooksPath unset
  info    kept             the vault, config.json, the compile state and any signing key: --remove deletes no data

15 change(s). Apply exactly these: knowledge setup --remove --apply 45483e7fe037b649 --harness claude
plan 45483e7fe037b649
```

It takes out the links, the Claude plugin, its marketplace and its option in `settings.json`
(every other key and hook stays), the block `setup` wrote in `CLAUDE.md` (the file itself when
nothing else is in it), the githooks path (a
checkout's own comes back), and a graphify MCP entry an earlier `setup` registered.

For each other harness found it takes out what [install.md](install.md) § Other harnesses added:

- Codex: the plugin, its marketplace, the `knowledge` server (and an earlier graphify one) setup
  registered, and the trust tables `codex plugin remove` leaves in `config.toml` (kept beside it
  first);
- opencode: `knowledge.ts`;
- the desktop app: `mcpServers.knowledge`;
- each `AGENTS.md` block.

A `generic` block goes with `knowledge setup --harness generic --instructions <file> --remove
--plan`, then `--apply <id>`.

It never touches the vault, and keeps `config.json`, the indexes, the compile state and any signing
key: delete `~/.config/knowledge` by hand if you want them gone. The binaries stay too: delete
`knowledge` and `knowledge-code` from `~/.local/bin` (a release install), or
`cargo uninstall knowledge` (a source build). The vault stays a folder of Markdown notes. Obsidian's graph colours
stay too: they are Obsidian's view state.
