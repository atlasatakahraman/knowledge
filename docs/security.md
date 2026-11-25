# Security

What the engine protects, how, and what it cannot stop. To report a vulnerability:
[SECURITY.md](../.github/SECURITY.md).

## What it touches

- **Writes:** the vault, `~/.config/knowledge` (the config, the indexes, the hooks' state, an
  optional signing key), and the files `setup` lists in its plan before it writes them
  ([install.md](install.md)). `setup --remove` takes out everything but the data
  ([remove.md](remove.md)).
- **Network:** `git pull` and the records push to your vault's own remote; `fetch`, when you or the
  agent run it; `update --plan`, when you run it; the headless flush and compile, through the
  harness's own CLI and account. No hook phones a server of ours, and nothing checks for updates on
  its own.
- **Never read:** harness credentials (`~/.claude/.credentials.json`, `~/.codex/auth.json`). The
  engine runs the harness's CLI and lets it sign in itself.

## Credential fences

Recommended for `~/.claude/settings.json`; `setup` never writes it, since security settings stay
yours:

```json
{
  "permissions": {
    "deny": [
      "Read(~/.ssh/**)",
      "Read(~/.codex/auth.json)",
      "Read(~/.claude/.credentials.json)",
      "Read(**/.env*)",
      "Read(~/.config/knowledge/signing_ed25519)"
    ]
  }
}
```

Merge it into an existing `permissions.deny`. These rules cover the Read tool; a shell command can
still read the files, so keep shell commands on approval. Codex's elevated Windows sandbox cannot
read `~/.config/knowledge`, and that folder gets no read grant, since it holds the signing key:
retrieval inside Codex goes through the read-only `knowledge` MCP server instead.

## Signing and pull trust

Commits from every machine are signed with an SSH key, and each session start pulls only a tip
signed by a key you listed. The key has no passphrase, because the hooks commit with nobody present;
it signs commits and tags, and nothing else. A release is signed by a separate key with a
passphrase, so a process running as you can't sign one ([release.md](release.md)); this repo's own
`allowed_signers` holds that key, and the per-machine one below holds the commit keys. In this
order, because step 4 is the one that can refuse a pull:

1. **Each machine:** `knowledge setup --plan --signing`, then `--apply` its id. It writes
   `~/.config/knowledge/signing_ed25519`, sets `gpg.format ssh`, `user.signingkey`,
   `commit.gpgsign` and `tag.gpgsign` in the vault and in the skeleton clone, and adds this
   machine's key to `~/.config/knowledge/allowed_signers`.
   `ssh-keygen -lf ~/.config/knowledge/signing_ed25519.pub` prints its fingerprint. Add the public
   key on GitHub as a **signing** key (`gh ssh-key add <key>.pub --type signing`; needs
   `gh auth refresh -s admin:ssh_signing_key`).
2. **Each machine:** add the other machine's public key to `allowed_signers`, one line each:
   `<email> namespaces="git" ssh-ed25519 AAAA…`. Fetch it from
   `https://api.github.com/users/<you>/ssh_signing_keys`, and compare its fingerprint
   (`ssh-keygen -lf <file>`) with the one the other machine printed before trusting it.
3. **Both machines at the same tip** of the vault and the skeleton, so the first checked pull is a
   signed commit.
4. **Each machine:** `"pullTrust": "signed"` in `config.json`. Then, on GitHub, a ruleset that
   requires signed commits on `main`, once every machine signs.

Only the tip of a pull is checked: a signed tip vouches for the unsigned commits under it. `%G?`
must be `G`; `U` (a good signature from a key not in `allowed_signers`) is refused, and so is a
missing `allowed_signers`. `allowed_signers` is passed on each call, never read from the repo's
config. The doctor's `pull-trust` check WARNs until step 4 and FAILs if a piece goes missing.
Undo, at any step: `git config --unset commit.gpgsign` in a clone, or remove `pullTrust`.

## Rules and the guard

`00-Meta/Hard-Rules.md` stays one note, loaded whole. Each `##` section is a pack, and each rule is
a bullet that opens with a bold line, whose slug is the rule's id. `00-Meta/rules.lock` holds what
the text cannot: each rule's kind, enforcement points and hash, and the guards.

- **The guard** (`hook pre-tool`, on each harness's pre-tool event for shell, edit and MCP calls)
  splits a shell line into segments and words, and answers `deny` or `ask` with the rule's id. It
  reads only `rules.lock`. It prints nothing when it allows, and on a missing or broken lock it
  allows with one stderr line. Codex and opencode cannot ask, so an ask rule is a deny there.
- **The backstop** is `githooks/pre-commit`. It runs the repo's own hook, gitleaks, then
  `rules check --staged`, which fails a commit holding a guard's staged names or a Hard-Rules that
  disagrees with the lock.
- **The restore.** Each session start runs `rules check --restore`. A rule that differs from the
  lock comes back from `HEAD` when `HEAD` matches the lock, and one line says so. Otherwise the
  doctor FAILs until you relock or revert.

**What it cannot stop.** An agent that runs code as you can change any file you can. The guard puts
the plain forms to you: an Edit, `sed -i`, a redirect, `Set-Content`. It cannot judge a wrapper (an
interpreter's `-e`, a command substitution, an escape). That case is caught afterwards, by the next
session start's restore and the doctor. A harness with no hooks (the `generic` adapter, the desktop
Chat) gets the rules as text only; the pre-commit backstop still runs in every clone `setup` set.

## What `fetch` refuses

Before it connects: anything but `http` and `https`, credentials in a URL, and any host that
resolves to a loopback, private (RFC 1918), link-local, unique-local, CGNAT, multicast or
unspecified address, IPv4 inside IPv6 included. It connects only to the addresses it checked,
follows at most 5 redirects and checks each one again, stops at 20 s and 5 MB, reads no proxy from
the environment, and stores only HTML (converted), other text and JSON. `--allow-private` lifts the
address check and nothing else. TLS uses the OS certificate store, with no bundled root list.

## Secrets in commits

Every commit in the vault, the skeleton clone and each checkout `setup` set runs `gitleaks git
--staged` first, and the records push runs gitleaks before it commits. Without gitleaks on `PATH`,
the hook refuses the commit and the records stay uncommitted.
