---
name: knowledge-sync
disable-model-invocation: true
description: Refresh the knowledge indexes - re-index the vault and repo docs, verify links
---

Bring the knowledge indexes back in step with what is on disk. The vault and checkout roots come from this machine's config:

```bash
VAULT=$(knowledge config vault); DEV=$(knowledge config devRoot)
```

Then run in order:

1. **Bring the vault index level** — the binary's, what the prompt hook and `knowledge find` read:

```bash
knowledge index && knowledge index --check
```

2. **The code map needs no step.** Each checkout's map (`$DEV/<remote-name>/`,
   [[0012-repos-leave-the-vault-index-stays]]) is brought level with its tracked files by every
   `knowledge find <symbol> --code` and `graph <symbol> --code` before it answers. To level them
   all now, or to rebuild one from nothing:

```bash
knowledge code index --all
knowledge code index <Repo> --rebuild
```

3. **Re-index a repo's docs** (`docs/**`, `CLAUDE.md`, `README.md`) for `knowledge find --repo`;
   the post-edit hook keeps them current, so this is for a checkout changed outside a session:

```bash
knowledge index --repo "$DEV/<Repo>"
```

4. **Verify referential integrity** — every `[[wikilink]]` resolves, every `checkout:`/`docs:`
   pointer exists under `devRoot` (a repo not cloned on this machine is listed, not failed).
   Exits non-zero if anything is broken:

```bash
knowledge doctor --only links,pointers --pretty
```

**Do not re-derive this check as a shell one-liner.** It was got wrong twice in one run that way —
once from case-sensitivity (Obsidian resolves `[[My-App]]` to `my-app.md`), once
from shell escaping mangling a regex so it skipped three files and printed a confident wrong
count. The doctor resolves names case-insensitively, skips code fences (bash `[[ ... ]]`
conditionals are indistinguishable from wikilinks), and treats the assets folder's copies as link
targets but not as vault notes. It does not resolve frontmatter `aliases:`: link by the note's name.

The deliberate-dangling allowlist lives in the vault — any MOC's **"Known gaps — don't chase
these"** section — not in the binary. Add a link there to silence it, with the reason.

5. Report only what changed: notes reindexed and any newly broken links. (`knowledge index` drops
   rows for files that are gone, so there is no prune step.)
