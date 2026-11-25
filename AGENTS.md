# AGENTS.md

Two parts: installing the engine for a person, and working on this repository.

## Install, for an agent

Run each step, show the person its output, and go on only when the output matches. Every command
is one line; the output shown is from a real run on Windows (paths shortened to `~`). The flags and
every step by hand: [docs/install.md](docs/install.md).

The person decides; you do the work. Where a step needs something from them (an install, a name, a
choice), ask one question with a concrete suggestion, then run the command yourself on their yes.
Two kinds of step stay theirs to run: a sign-in (`gh auth login`, the harness's own), and anything
that lowers a protection (see the list at the end). An installer that asks for elevation shows its
own prompt; that click is theirs too.

1. **Check the prerequisites.** Each prints a version. For a missing one, ask whether to install
   it, naming the command from [docs/install.md](docs/install.md) § Prerequisites for this OS, and
   run it on a yes; a new install may need a new shell before it is on `PATH`. `gh` checks the
   build provenance and needs the person signed in: `gh auth status` says whether they are, and
   `gh auth login` is theirs to run.

   ```
   $ git --version
   git version 2.55.0.windows.4
   $ gitleaks version
   8.30.1
   $ claude --version
   2.1.330 (Claude Code)
   $ gh --version
   gh version 2.97.0 (2026-07-31)
   ```

   Then git's identity, which the vault's commits carry. When `git config user.email` prints
   nothing, ask for the name and email to use, and whether for all of this machine's git or only
   the vault. All of it: `git config --global user.name "<name>"` and `user.email` now, on a yes.
   Only the vault: the same with `git -C <vault>` once step 7 has made it, then step 7's last
   paragraph.

2. **Download the latest release** into a new, empty folder. `SHA256SUMS` comes first: it names
   one archive per platform, `knowledge-<version>-<target>.zip` on Windows and `.tar.gz` elsewhere.
   Pick this machine's target (`x86_64-pc-windows-msvc`, `aarch64-apple-darwin`,
   `x86_64-unknown-linux-gnu`, …), then fetch that archive and `SHA256SUMS.sig` from the same
   place. On Windows the command is `curl.exe`, since PowerShell's `curl` is another command.

   ```
   $ curl -fsSLO https://github.com/atlasatakahraman/knowledge/releases/latest/download/SHA256SUMS
   $ curl -fsSLO https://github.com/atlasatakahraman/knowledge/releases/latest/download/SHA256SUMS.sig
   $ curl -fsSLO https://github.com/atlasatakahraman/knowledge/releases/latest/download/knowledge-0.13.1-x86_64-pc-windows-msvc.zip
   ```

   No archive for this target: build from source instead (step 5's last paragraph).

3. **Clone the skeleton at that release's tag** into the person's checkouts folder (`~/dev` unless
   they keep another). The checkout stays on the release, so its plugins match the binary; a
   session start leaves a checkout on a tag alone.

   ```
   $ git -c advice.detachedHead=false clone --branch v0.13.1 https://github.com/atlasatakahraman/knowledge ~/dev/knowledge
   Cloning into '~/dev/knowledge'...
   ```

4. **Verify the archive before extracting it,** with all three checks of
   [docs/verify.md](docs/verify.md), in its order, run in the download folder. Copy
   `~/dev/knowledge/allowed_signers` there first. Its key's fingerprint must be
   `SHA256:4lk0qVkGnFfrnKNselpgVf9e55AvCRSy5OkdhcCq6uc`. Any check that fails is the answer to
   report: stop, and do not extract.

   ```
   $ cmd /c "ssh-keygen -Y verify -f allowed_signers -I atlasatakahraman -n file -s SHA256SUMS.sig < SHA256SUMS"
   Good "file" signature for atlasatakahraman with ED25519 key SHA256:4lk0qVkGnFfrnKNselpgVf9e55AvCRSy5OkdhcCq6uc
   $ $f = "knowledge-0.13.1-x86_64-pc-windows-msvc.zip"; ((Get-FileHash $f -Algorithm SHA256).Hash + "  $f") -in (Get-Content SHA256SUMS)
   True
   $ gh attestation verify knowledge-0.13.1-x86_64-pc-windows-msvc.zip --repo atlasatakahraman/knowledge --signer-workflow atlasatakahraman/knowledge/.github/workflows/release.yml --source-ref refs/tags/v0.13.1 --deny-self-hosted-runners
   ```

   The last one exits 0 on a match.

5. **Put the two binaries in `~/.local/bin`.** The folder must be named `bin`: `setup` takes any
   other folder for a build folder and WARNs. `tar` reads the zip on Windows too (Windows 10 1803
   and later). If `~/.local/bin` is not on `PATH`, ask, then add it for the person's user: on
   Windows `[Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ";$HOME\.local\bin", 'User')`,
   elsewhere one line in their shell's profile. A new shell sees it.

   ```
   $ mkdir -p ~/.local/bin
   $ tar -xf knowledge-0.13.1-x86_64-pc-windows-msvc.zip -C ~/.local/bin --strip-components 1 knowledge-0.13.1-x86_64-pc-windows-msvc/knowledge.exe knowledge-0.13.1-x86_64-pc-windows-msvc/knowledge-code.exe
   $ knowledge --version
   knowledge 0.13.1
   SQLite 3.54.1, FTS5 on
   ```

   Outside Windows the members have no `.exe`. `FTS5 on` is required: without it the index cannot
   search. To build from source instead (no archive for the target), the Rust toolchain is a
   prerequisite too, and `cargo install --path ~/dev/knowledge --locked` puts both binaries in
   `~/.cargo/bin` in two to three minutes.

6. **Plan the setup, and show the whole plan to the person:**

   ```
   $ knowledge setup --plan
   knowledge 0.13.1 setup plan: nothing is written
   ...
   18 change(s). Apply exactly these: knowledge setup --apply d9305150d8d7ceea --harness claude
   plan d9305150d8d7ceea
   ```

   Each line is `ok`, `create`, `change`, `WARN` or `FAIL`. A FAIL is the answer to report: quote it
   and stop, do not work around it. A WARN that names a missing tool carries the command that
   installs it: ask, run it on a yes, then plan again.

7. **Apply the plan, only on the person's yes,** with the id from its last line:

   ```
   $ knowledge setup --apply <id>
   ...
   Done.
   ```

   The id refuses to apply if anything changed since the plan; then plan again and show it again.

   A new vault gets its first commit when git knows `user.email`. When the person chose an identity
   for the vault only, set it now with `git -C <vault> config`, then make that commit:
   `git -C <vault> add -A` and
   `git -C <vault> commit -m "chore: new vault from knowledge <version>"`.

8. **Build the index and check the vault:**

   ```
   $ knowledge doctor --repair --pretty
   knowledge doctor  PASS  2026-11-24T10:49:50.767Z
   ...
     WARN  hygiene
     WARN  pull-trust
   ```

   PASS with those two WARNs is a finished install: `hygiene` is uncommitted work, `pull-trust` is
   opt-in ([docs/security.md](docs/security.md)).

9. **The rules interview.** `knowledge rules list --json` lists the packs. Ask the person which to
   turn on, one question with the packs as options, then `knowledge rules enable <pack>` for each
   one they chose.

10. **For a new vault, fill it in with the person,** one section at a time. `grep -n "FILL:"` on
    `<vault>/00-Meta/Identity.md` and `<vault>/00-Meta/Hard-Rules.md` lists the placeholders. For
    each, ask one question that quotes it and offers a concrete answer from what you already know
    (their earlier answers, this machine's OS, shell and installed tools); they accept it, change
    it, or skip. Show the edit as a diff and write it on a yes; a skipped one keeps its `FILL:`
    comment for later. Then the same for `00-Meta/retrieval-cases.json`: propose three or four
    questions the vault must always answer, each with the note that answers it. After Hard-Rules
    changes, `knowledge rules lock` re-stamps the lock. For each new rule that names something a
    machine can see (a command, a path, an MCP tool), propose its guard: the
    `knowledge rules guard add` line from [docs/verbs.md](docs/verbs.md), run on a yes. Last,
    commit what was filled, on a yes, with a subject such as `docs(meta): identity and rules`.

11. **Offer a private GitHub repo for the vault.** The session start pulls and the hooks push their
    records when the vault has an `origin`, which is how a second machine shares it. Ask whether
    they want one, and its name (suggest `knowledge-vault`). On a yes, with `gh` signed in:

    ```
    $ gh repo create knowledge-vault --private --source ~/knowledge --remote origin --push
    ```

    Always `--private`: a vault holds who the person is and how they work. A public one is theirs
    to make on GitHub, never yours to offer.

12. **A new session, started in the vault,** picks up the hooks. Tell the person so, and stop.

**What you are never asked to do, and must not do:**

- read, enter or copy a credential, a token or a private key, or sign in for the person;
- turn off a protection: a permission rule, a sandbox, the secret scan, a hook, or a Windows
  setting such as Developer Mode (no step needs it: every link is a folder's junction);
- approve a permission prompt, or write Codex's hook trust (the person reviews hooks in `/hooks`);
- turn on automatic updates;
- commit to or push the person's vault without their yes, or make it public;
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
| `skills/` | the vault-writing skill and the slash-only `knowledge-*` skills; they run only `knowledge` |
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
