# Make a release

For the maintainer. A release is eight archives (`build.yml`'s targets), one `SHA256SUMS` over
them, a build attestation per archive, and `SHA256SUMS.sig`, the maintainer's signature. CI makes
everything but the signature, which is made by hand on the maintainer's machine, so the key never
reaches GitHub. Users check all three: [verify.md](verify.md).

## The release key, once

A key of its own, with a passphrase, kept apart from the per-machine key that signs commits. That
key has no passphrase because the hooks commit with nobody present, so anything running as you
could use it; this one needs you there.

```bash
ssh-keygen -t ed25519 -C knowledge-release -f ~/.ssh/knowledge_release_ed25519
```

Back up the private half offline; it never goes to GitHub or to a second machine. The public half
is the one line of `allowed_signers` at the repo root, limited to file signatures:

```
atlasatakahraman namespaces="file" ssh-ed25519 <the .pub's key> knowledge-release
```

To replace the key, add the new line to `allowed_signers` in a release still signed by the old one,
then drop the old line in the next.

## Each release

1. Set `version` in `Cargo.toml`, run `cargo build` so `Cargo.lock` follows, and commit. The
   workflow refuses a tag that does not name this version.
2. Tag and push the tag. `tag.gpgsign` signs it with the machine key.

   ```bash
   git tag v0.13.0 -m "knowledge 0.13.0: <one line>"
   git push origin v0.13.0
   ```

3. `release.yml` builds the eight archives, writes `SHA256SUMS`, attests each archive and opens a
   **draft** release with them. Nothing is public yet.
4. Sign `SHA256SUMS`, in an empty folder. `ssh-keygen` asks for the passphrase and writes
   `SHA256SUMS.sig`:

   ```bash
   gh release download v0.13.0 -R atlasatakahraman/knowledge-skeleton -p SHA256SUMS
   ssh-keygen -Y sign -f ~/.ssh/knowledge_release_ed25519 -n file SHA256SUMS
   ```

5. Check the signature with [verify.md](verify.md) step 1 against the repo's `allowed_signers`,
   then attach it:

   ```bash
   gh release upload v0.13.0 SHA256SUMS.sig -R atlasatakahraman/knowledge-skeleton
   ```

6. Download one archive from the draft and run all of [verify.md](verify.md) on it. Then publish:

   ```bash
   gh release edit v0.13.0 --draft=false -R atlasatakahraman/knowledge-skeleton
   ```

`workflow_dispatch` on `release.yml` is the dry run: the archives and `SHA256SUMS` are left as the
run's artifacts, with no attestation and no release.
