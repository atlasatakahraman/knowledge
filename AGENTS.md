# AGENTS.md

Two parts: installing the engine for a person, and working on this repository.

## Install, for an agent

Run each step, show the person its output, and go on only when the output matches. Every command
is one line; the output shown is from a real run on Windows (paths shortened to `~`). The flags and
every step by hand: [docs/install.md](docs/install.md).

1. **Check the prerequisites.** Each prints a version; a missing one is the person's to install
   (the commands are in [docs/install.md](docs/install.md)), and you ask before running any.

   ```
   $ git --version
   git version 2.55.0.windows.4
   $ cargo --version
   cargo 1.100.0 (3acc79c0c 2026-11-05)
   $ gitleaks version
   8.30.1
   $ claude --version
   2.1.330 (Claude Code)
   ```

2. **Clone the skeleton** into the person's checkouts folder (`~/dev` unless they keep another):

   ```
   $ git clone https://github.com/atlasatakahraman/knowledge ~/dev/knowledge
   ```

3. **Build and install the binary** (two to three minutes):

   ```
   $ cargo install --path ~/dev/knowledge --locked
      Installed package `knowledge v0.13.0 (~/dev/knowledge)` (executables `knowledge-code.exe`, `knowledge.exe`)
   $ knowledge --version
   knowledge 0.13.0
   SQLite 3.54.1, FTS5 on
   ```

   `FTS5 on` is required: without it the index cannot search.

4. **Plan the setup, and show the whole plan to the person:**

   ```
   $ knowledge setup --plan
   knowledge 0.13.0 setup plan: nothing is written
   ...
   14 change(s). Apply exactly these: knowledge setup --apply 1a5be68145b5b7a2 --harness claude
   plan 1a5be68145b5b7a2
   ```

   Each line is `ok`, `create`, `change`, `WARN` or `FAIL`. A FAIL is the answer to report: quote it
   and stop, do not work around it. A WARN that names a missing tool carries the command that
   installs it; installing is the person's call.

5. **Apply the plan, only on the person's yes,** with the id from its last line:

   ```
   $ knowledge setup --apply <id>
   ...
   Done.
   ```

   The id refuses to apply if anything changed since the plan; then plan again and show it again.

6. **Build the index and check the vault:**

   ```
   $ knowledge doctor --repair --pretty
   knowledge doctor  PASS  2026-11-24T10:49:50.767Z
   ...
     WARN  hygiene
     WARN  pull-trust
   ```

   PASS with those two WARNs is a finished install: `hygiene` is uncommitted work, `pull-trust` is
   opt-in ([docs/security.md](docs/security.md)).

7. **The rules interview.** `knowledge rules list --json` lists the packs. Ask the person which to
   turn on, one question with the packs as options, then `knowledge rules enable <pack>` for each
   one they chose.

8. **For a new vault,** tell the person what is left for them: the `FILL:` placeholders in
   `00-Meta/Identity.md` and `00-Meta/Hard-Rules.md`, and the questions in
   `00-Meta/retrieval-cases.json`. Then a new session, in the vault, picks up the hooks.

**What you are never asked to do, and must not do:**

- read, enter or copy a credential, a token or a private key, or sign in for the person;
- turn off a protection: a permission rule, a sandbox, the secret scan, a hook;
- approve a permission prompt, or write Codex's hook trust (the person reviews hooks in `/hooks`);
- turn on automatic updates;
- commit or push the person's vault;
- edit a hook in `~/.claude/settings.json` that is not this engine's.

If a step seems to need one of these, stop and say which step and why.

## Working on this repository

The engine is one Rust crate with two binaries: `knowledge` (every hook, the index, the doctor,
`setup`, the MCP server) and `knowledge-code` (the code map, so no hook loads the tree-sitter
grammars). Layout:

| Path | |
|---|---|
| `src/` | the crate; `src/harness/<id>.rs` translates one harness's hook payloads |
| `adapters/<id>.json` | one data file per harness, built into the binary |
| `claude/`, `codex/`, `opencode/` | the harnesses' plugins; each hook runs the binary |
| `skills/`, `commands/` | the vault-writing skill and the slash commands; they run only `knowledge` |
| `template/` | the empty vault `setup --from-template` copies, with its generic rules and lock |
| `githooks/` | the pre-commit backstop `setup` points every clone at |
| `checks/stage-N.sh` | the runnable checks, one per stage; CI runs them on every target |
| `docs/` | one file per task: install, verify, update, remove, verbs, security, troubleshooting, release |
| `allowed_signers` | the release key; `update --plan` builds it in (docs/release.md) |

**Build and check.** `rust-toolchain.toml` pins the compiler. Before a commit, all of these pass:

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
cargo deny check
for n in 3 4 5 6 7 8 9; do bash checks/stage-$n.sh target/release/knowledge || break; done
gitleaks git --staged
```

- **Never run a build against the real home.** The checks set `HOME`, `USERPROFILE` and
  `KNOWLEDGE_HOME` to a fixture. Do the same for any probe: a new binary run against a real
  `~/.config/knowledge` moves real state.
- **No new crate** without the maintainer's pick; `checks/crates.txt` lists every crate in the tree
  and CI fails on any other. The licence must fit MIT (no GPL).
- **A folder name** (`00-Meta`, `30-Sessions`, …) is written only in `src/config.rs`; everything
  else reads `config::META` and its siblings, since a vault may rename them.
- **A doc's command output is pasted from a real run,** on a fixture vault, never written from
  memory. No doc runs bun.
- **Commits** are by area, each building on its own, with a body that says why.
