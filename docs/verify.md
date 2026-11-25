# Verify a release

Check an archive before you extract it. Three checks, each answering a different question:

| Check | Answers | Trusts |
|---|---|---|
| The signature over `SHA256SUMS` | Did the maintainer approve exactly these bytes? | Only the release key, which never leaves the maintainer's machine |
| The checksum | Is your archive the one `SHA256SUMS` lists? | The signed `SHA256SUMS` |
| The build provenance | Was the archive built by this repository's release workflow, from the tag? | GitHub and the workflow |

The signature is the one that does not trust GitHub: anyone who could rewrite a release (a stolen
token, a compromised build step) could replace the archives and `SHA256SUMS` together, but not
sign them. Run the signature first, then the checksum; the provenance adds GitHub's word on how the
bytes were built.

All three OSes ship OpenSSH with `ssh-keygen -Y verify` (OpenSSH 8.2 or later; Windows 10 and 11
have it in `C:\Windows\System32\OpenSSH`). The provenance needs the GitHub CLI, `gh`.

## Download

From the release page, into an empty folder: your platform's archive, `SHA256SUMS` and
`SHA256SUMS.sig`. Then `allowed_signers` from the same tag:

```
https://raw.githubusercontent.com/atlasatakahraman/knowledge/v0.13.2/allowed_signers
```

It holds one line, the release key:

```
atlasatakahraman namespaces="file" ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOQM0nPqBftl8DiBjK/iwZNA0Ut7iamaOJhVMXJk1Dy7 knowledge-release
```

Its fingerprint is `SHA256:4lk0qVkGnFfrnKNselpgVf9e55AvCRSy5OkdhcCq6uc`. A copy fetched from the same place as the archive proves
only as much as that place, so compare the fingerprint with one you got elsewhere: the copy in the
checkout or binary you already trust (`knowledge update --plan` prints the line it was built with),
or this page at an older tag.

## 1. The signature

```bash
ssh-keygen -Y verify -f allowed_signers -I atlasatakahraman -n file -s SHA256SUMS.sig < SHA256SUMS
```

```powershell
cmd /c "ssh-keygen -Y verify -f allowed_signers -I atlasatakahraman -n file -s SHA256SUMS.sig < SHA256SUMS"
```

PowerShell has no `<`, and piping the file through it would change its bytes, so `cmd` reads it.
A good signature prints the key's fingerprint and exits 0:

```
Good "file" signature for atlasatakahraman with ED25519 key SHA256:4lk0qVkGnFfrnKNselpgVf9e55AvCRSy5OkdhcCq6uc
```

A changed `SHA256SUMS` exits non-zero:

```
Could not verify signature.
Signature verification failed: incorrect signature
```

So does a signature made for anything but a file (`-n file`), such as a commit:

```
Could not verify signature.
Couldn't verify signature: namespace does not match
```

## 2. The checksum

```bash
grep -F "  knowledge-0.13.2-x86_64-unknown-linux-gnu.tar.gz" SHA256SUMS | sha256sum -c -
```

```
knowledge-0.13.2-x86_64-unknown-linux-gnu.tar.gz: OK
```

On macOS, `shasum -a 256 -c -` in place of `sha256sum -c -`. On Windows:

```powershell
$f = "knowledge-0.13.2-x86_64-pc-windows-msvc.zip"
((Get-FileHash $f -Algorithm SHA256).Hash + "  $f") -in (Get-Content SHA256SUMS)
```

`True` is a match. `False` is a changed archive, or a name that is not in `SHA256SUMS`.

## 3. The build provenance

```bash
gh attestation verify knowledge-0.13.2-x86_64-unknown-linux-gnu.tar.gz \
  --repo atlasatakahraman/knowledge \
  --signer-workflow atlasatakahraman/knowledge/.github/workflows/release.yml \
  --source-ref refs/tags/v0.13.2 --deny-self-hosted-runners
```

It checks the archive's attestation in Sigstore's public transparency log: built by `release.yml`,
from the tag, on a GitHub-hosted runner. It exits 0 on a match. An archive with no attestation
exits 1:

```
Error: HTTP 404: Not Found (https://api.github.com/repos/atlasatakahraman/knowledge/attestations/sha256:…)
```

## Then

Extract, put `knowledge` and `knowledge-code` on your `PATH` side by side, and follow
[install.md](install.md) from `knowledge setup --plan`, or [update.md](update.md) over an older
version. How a release is made and signed: [release.md](release.md).
