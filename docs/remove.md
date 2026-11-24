# Remove

```bash
knowledge setup --remove --plan
knowledge setup --remove --apply <id>
```

Like the install, the plan writes nothing and the apply refuses if anything moved in between. A
plan for Claude Code alone (`--harness claude`), with the home folder shown as `~`:

```
knowledge 0.12.0 remove plan: nothing is written

  remove  settings.json    ~/.claude/settings.json: our 0 hooks and the plugin's option removed, every other hook kept (the old file kept as ~/.claude/settings.json.pre-skeleton-f3876bf0)
  remove  plugin           claude plugin uninstall knowledge@knowledge-skeleton (0.12.0), then marketplace remove knowledge-skeleton
  remove  CLAUDE.md        ~/.claude/CLAUDE.md: the imports section setup wrote
  remove  link             ~/.claude/hooks
  remove  link             ~/.claude/skills/brain
  remove  link             ~/.agents/skills/brain
  remove  link             ~/.claude/commands/brain-commits.md
  remove  link             ~/.claude/commands/brain-log.md
  remove  link             ~/.claude/commands/brain-sync.md
  remove  link             ~/.claude/commands/brain-usage.md
  info    kept             the vault, config.json, the compile state and any signing key: --remove deletes no data

10 change(s). Apply exactly these: knowledge setup --remove --apply 356a39e9fbfa4955 --harness claude
plan 356a39e9fbfa4955
```

It takes out the links, the Claude plugin, its marketplace and its option in `settings.json`
(every other key and hook stays), the block `setup` wrote in `CLAUDE.md`, the githooks path (a
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
key: delete `~/.config/knowledge` by hand if you want them gone. The binary stays too:
`cargo uninstall knowledge`. The vault stays a folder of Markdown notes. Obsidian's graph colours
stay too: they are Obsidian's view state.
