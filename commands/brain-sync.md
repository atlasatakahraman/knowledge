---
description: Refresh the brain - re-index, merge repo graphs, verify links
---

Bring the brain's indexes back in step with what is on disk. The vault and checkout roots come from this machine's config:

```bash
CFG=~/.claude/hooks/skeleton-config.mjs; VAULT=$(bun --bun $CFG vault); DEV=$(bun --bun $CFG devRoot)
```

Then run in order:

1. **Bring the vault index level** — the binary's, what the prompt hook and `knowledge find` read:

```bash
knowledge index && knowledge index --check
```

2. **Rebuild any repo graph you have touched, then re-merge.** The checkouts live at
   `$DEV/<remote-name>/` ([[0012-repos-leave-the-vault-index-stays]]) and each
   carries its own `graphify-out/`, built on demand. The vault is not in graphify: its links are
   `knowledge graph`'s.

```bash
cd "$DEV/<Repo>" && graphify update .
bun --bun ~/.claude/hooks/brain-graph.mjs
```

**`merged-graph.json` is what the graphify MCP serves.** It is the repo graphs only.

3. **Re-index a repo's docs** (`docs/**`, `CLAUDE.md`, `README.md`) for `knowledge find --repo`;
   the post-edit hook keeps them current, so this is for a checkout changed outside a session:

```bash
knowledge index --repo "$DEV/<Repo>"
```

4. **Verify referential integrity** — every `[[wikilink]]` resolves, every `checkout:`/`docs:`
   pointer exists under `devRoot` (a repo not cloned on this machine is listed, not failed).
   Exits non-zero if anything is broken:

```bash
bun --bun ~/.claude/hooks/brain-verify.mjs
```

**Do not re-derive this check as a shell one-liner.** It was got wrong twice in one run that way —
once from case-sensitivity (Obsidian resolves `[[My-App]]` to `my-app.md`), once
from shell escaping mangling a regex so it skipped three files and printed a confident wrong
count. The script handles case-insensitivity, frontmatter `aliases:`, code-fence stripping (bash
`[[ ... ]]` conditionals are indistinguishable from wikilinks), and treats `80-Assets/` copies as
link targets but not as vault notes.

The deliberate-dangling allowlist lives in the vault — any MOC's **"Known gaps — don't chase
these"** section — not in the script. Add a link there to silence it, with the reason.

5. Report only what changed: notes reindexed and any newly broken links. (`knowledge index` drops
   rows for files that are gone, so there is no prune step.)

**Do not run `/graphify . --update`** unless the user explicitly asks — that is the semantic pass
over docs and images, it dispatches subagents, and it costs real tokens. `graphify update .` is
the routine one.
