#!/usr/bin/env bash
# Stage 9's runnable check (S9.3, layout and conventions as config): a vault with two folders
# renamed through config.json passes the doctor's vault checks; a bad `folders` is refused; a vault
# made by `setup --from-template` passes them with the generic packs locked; no folder name is
# written in src/ outside config.rs. S9.4: the docs exist, one per task. S9.5 (signed releases):
# allowed_signers holds one file-only ed25519 key, docs/verify.md names its fingerprint, and a
# fixture key's signature and checksum pass and fail as docs/verify.md says. Every run is on a
# fixture HOME and KNOWLEDGE_HOME, so the real state never moves. Builds nothing. Run from the repo
# root:
#   bash checks/stage-9.sh target/release/knowledge
set -u
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
ROOT=$(cd "$(dirname "$0")/.." && pwd)
fails=0
pass() { echo "PASS  $1"; }
fail() { echo "FAIL  $1"; fails=$((fails + 1)); }
check() { if eval "$2"; then pass "$1"; else fail "$1"; fi; }
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }

TMP=$(mktemp -d)
export HOME="$TMP/user" USERPROFILE="$TMP/user" KNOWLEDGE_BIN="$BIN" TMPDIR="$TMP" TMP TEMP="$TMP" # failures land in temp
unset KNOWLEDGE_CONFIG XDG_CONFIG_HOME CLAUDE_CONFIG_DIR CLAUDE_INVOKED_BY
k() { timeout 120 "$BIN" "$@"; }
# The doctor's checks that read the vault, not this machine's transcripts or harnesses.
VAULT_CHECKS=vault-index,retrieval,frontmatter,links,pointers,reach,commits,priming,priming-size,rules,portable
doctor_ok() { k doctor --json --only "$VAULT_CHECKS" > "$TMP/doctor.json" 2>&1; }

# --- A vault with `meta` and `sessions` renamed --------------------------------------------------
export KNOWLEDGE_HOME="$TMP/home"
mkdir -p "$KNOWLEDGE_HOME" "$HOME/.claude" "$TMP/dev"
V="$TMP/renamed"
cp -R "$ROOT/template" "$V"
mv "$V/00-Meta" "$V/Meta"
mv "$V/30-Sessions" "$V/Journal"
git -C "$V" init -q
VN=$(native "$V")
conf() { printf '{"vault": "%s", "devRoot": "%s"%s}\n' "$VN" "$(native "$TMP/dev")" "$1" > "$KNOWLEDGE_HOME/config.json"; }
conf ', "folders": {"meta": "Meta", "sessions": "Journal"}'
printf '# Mine\n\n@%s/Meta/Identity.md\n@%s/Meta/Hard-Rules.md\n' "$VN" "$VN" > "$HOME/.claude/CLAUDE.md"

check "config prints the renamed folders, and the default for one not named" \
  '[ "$(k config meta)" = Meta ] && [ "$(k config sessions)" = Journal ] && [ "$(k config ops)" = 50-Ops ]'
k doctor --repair --pretty > /dev/null 2>&1
check "the renamed vault passes the doctor's vault checks" 'doctor_ok'
doctor_ok || sed 's/^/      /' "$TMP/doctor.json" | head -20
check "rules reads the lock from the renamed meta folder" \
  'k doctor --json --only rules | grep -q "match Meta/rules.lock"'
day=$(k doctor --json --only sessions | sed -n 's/.*"day":"\([0-9-]*\)".*/\1/p')
check "sessions finds no note for the day before one is written" \
  '[ -n "$day" ] && k doctor --json --only sessions | grep -q "\"note\":false"'
mkdir -p "$V/Journal/${day:0:4}/${day:5:2}"
printf -- '---\ntype: session\ncreated: %s\n---\n\n# %s\n' "$day" "$day" > "$V/Journal/${day:0:4}/${day:5:2}/$day.md"
check "sessions finds the day's note in the renamed sessions folder" \
  'k doctor --json --only sessions | grep -q "\"note\":true"'

for bad in '{"meta": "../Meta"}' '{"meta": "C:/Meta"}' '{"meta": "a\\\\b"}' '{"notes": "Notes"}' \
  '{"meta": "Meta", "sessions": "meta"}' '{"meta": ""}' '{"meta": "Meta/./x"}'; do
  conf ", \"folders\": $bad"
  check "a bad folders is refused: $bad" '! k rules list > "$TMP/bad.txt" 2>&1 && grep -q "\"folders" "$TMP/bad.txt"'
done
conf ', "folders": {"meta": "Meta", "sessions": "Journal"}'
check "the good folders are taken again" 'k rules list > /dev/null 2>&1'
unset KNOWLEDGE_HOME

# --- A vault from `setup --from-template` --------------------------------------------------------
# The skeleton as this working tree has it, and the plugin recorded as installed at its version,
# so setup never runs the real `claude plugin` (as in stage-6.sh).
export HOME="$TMP/fresh" USERPROFILE="$TMP/fresh"
SK="$TMP/sk"
mkdir -p "$SK" "$HOME/.claude/plugins"
for d in commands skills githooks template claude; do cp -R "$ROOT/$d" "$SK/$d"; done
cp "$ROOT/Cargo.toml" "$SK/"
git -C "$SK" init -q
ver=$(sed -n 's/^ *"version": "\(.*\)",$/\1/p' "$ROOT/claude/.claude-plugin/plugin.json")
printf '{"knowledge": {"source": {"source": "git", "url": "fixture"}}}\n' > "$HOME/.claude/plugins/known_marketplaces.json"
printf '{"version": 2, "plugins": {"knowledge@knowledge": [{"scope": "user", "version": "%s"}]}}\n' "$ver" > "$HOME/.claude/plugins/installed_plugins.json"
T="$TMP/tvault"
s() { k setup "$@" --vault "$(native "$T")" --dev "$(native "$TMP/tdev")" --skeleton "$(native "$SK")" --from-template; }
id=$(s --plan | tail -n 1 | sed -n 's/^plan \([0-9a-f]\{16\}\)$/\1/p')
s --apply "$id" > "$TMP/apply.txt" 2>&1
check "setup --from-template applies" '[ -n "$id" ] && [ -f "$T/00-Meta/rules.lock" ]'
check "it writes the template's rules and lock as they are" \
  'cmp -s "$T/00-Meta/Hard-Rules.md" "$ROOT/template/00-Meta/Hard-Rules.md" && cmp -s "$T/00-Meta/rules.lock" "$ROOT/template/00-Meta/rules.lock"'
k doctor --repair --pretty > /dev/null 2>&1
check "the template vault passes the doctor's vault checks" 'doctor_ok'
doctor_ok || sed 's/^/      /' "$TMP/doctor.json" | head -20
packs=$(k rules list --json | tr -d ' \n')
on() { printf '%s' "$packs" | grep -q "\"id\":\"$1\",\"section\":\"[^\"]*\",\"enabled\":true"; }
check "the generic packs are locked and enabled" \
  'on tooling && on code && on git && on honesty && on verify && on safety && on knowledge'
check "no owner-specific pack ships" \
  '! printf "%s" "$packs" | grep -qE "\"id\":\"(purchases|people|repos|tauri|turborepo|bun|shadcn|design)\""'
check "the generic guards ship, and none bound to one machine or repo" \
  'k rules list | grep -q "^3 guards: rules-files, production, push-destructive-git$"'
# S9.4 defects: a hooksPath spelled otherwise for our own folder read as the checkout's own, and a
# machine with no Claude Code session yet failed the doctor's budget check.
git -C "$T" config core.hooksPath "$(native "$SK")/githooks/."
check "a hooksPath naming our githooks in another spelling reads ok" \
  's --plan | grep -E "^  [a-z]+ +hooksPath +$(native "$T")" | grep -q "^  ok "'
check "no Claude Code session yet: the budget check passes with nothing to measure" \
  'k doctor --json --only budget | grep -q "\"ok\":true.*\"transcript\":null"'

# --- No folder name in src/ outside config.rs (comment lines left out) ---------------------------
hits=$(grep -nE '"[^"]*(00-Meta|10-Repos|20-Knowledge|30-Sessions|40-Plans|50-Ops|60-Decisions|80-Assets|90-Inbox)' "$ROOT"/src/*.rs |
  grep -v '/src/config\.rs:' | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//')
check "src/ names no folder outside config.rs" '[ -z "$hits" ]'
[ -z "$hits" ] || printf '%s\n' "$hits" | sed 's/^/      /'

# --- S9.4: the docs split by task, and `update --plan` -------------------------------------------
DOCS="$ROOT/README.md $(ls "$ROOT"/README.*.md) $ROOT/AGENTS.md $ROOT/SECURITY.md $(ls "$ROOT"/docs/*.md)"
dead=""
for f in $DOCS; do
  dir=$(dirname "$f")
  for t in $(grep -oE '\]\([^)#[:space:]]+(#[^)]*)?\)' "$f" | sed -E 's/^\]\(//; s/\)$//; s/#.*//' | grep -vE '^[a-z]+:'); do
    [ -e "$dir/$t" ] || dead="$dead ${f#"$ROOT"/} -> $t"
  done
done
check "every link between the docs resolves" '[ -z "$dead" ]'
[ -z "$dead" ] || printf '      %s\n' $dead
check "the docs are one per task" \
  'for d in install verify update remove verbs security troubleshooting release; do [ -f "$ROOT/docs/$d.md" ] || exit 1; done'
# architecture.md and benchmark.md are contributor docs: the benchmark and the image build run through
# bun, so they name it. The rule is for the docs a user installs from.
BUN_DOCS=""
for f in $DOCS; do case "$f" in */docs/architecture.md|*/docs/benchmark.md) ;; *) BUN_DOCS="$BUN_DOCS $f" ;; esac; done
bun=$(grep -rnE '(^|[^[:alnum:]_./-])bunx?( |`|$)' $BUN_DOCS "$ROOT/skills" "$ROOT/commands" "$ROOT/template" 2>/dev/null)
check "no user doc, skill, command or template note names a bun command" '[ -z "$bun" ]'
[ -z "$bun" ] || printf '%s\n' "$bun" | sed 's/^/      /'
# Each translated README mirrors README.md: its code blocks verbatim, and the same link and image
# targets once its own -<lang>- image names are read as README.md's. The language row, the
# "translation of README.md" line and in-page anchors (#ajanlar, #agents) differ by design.
fences() { awk '/^```/ { print; f = !f; next } f' "$1"; }
targets() {
  grep -oE '\]\([^)]*\)|(src|srcset|href)="[^"]*"' "$1" | sed -E 's/^\]\(//; s/\)$//; s/^[a-z]+="//; s/"$//' |
    sed -E 's/-(tr|es|pt|fr|de)-(light|dark)\.svg$/-\2.svg/' | grep -vE '^(README(\.[a-z]+)?\.md|#)' | sort
}
fences "$ROOT/README.md" > "$TMP/en.fences"; targets "$ROOT/README.md" > "$TMP/en.targets"
tr_bad=""
for lang in tr es pt fr de; do
  r="$ROOT/README.$lang.md"
  [ -f "$r" ] || { tr_bad="$tr_bad missing:$lang"; continue; }
  fences "$r" | cmp -s - "$TMP/en.fences" || tr_bad="$tr_bad fences:$lang"
  targets "$r" | cmp -s - "$TMP/en.targets" || tr_bad="$tr_bad targets:$lang"
  # Its own images exist (the raw badges/ URLs are built by badges.yml, not kept here).
  for a in $(grep -oE '(src|srcset)="assets/[^"]*"' "$r" | sed -E 's/^[a-z]+="//; s/"$//'); do
    [ -f "$ROOT/$a" ] || tr_bad="$tr_bad image:$lang:$a"
  done
done
check "each translated README keeps README.md's code blocks, links and images" '[ -z "$tr_bad" ]'
[ -z "$tr_bad" ] || printf '      %s\n' $tr_bad
old=$(grep -rln 'INSTALL\.md' $DOCS "$ROOT/src" "$ROOT/template" "$ROOT/skills" "$ROOT/commands" 2>/dev/null)
check "nothing points at INSTALL.md" '[ -z "$old" ]'
export KNOWLEDGE_HOME="$TMP/uhome"; mkdir -p "$KNOWLEDGE_HOME"
up=$(k update --plan 2>&1); ucode=$?
if [ $ucode -ne 0 ] && printf '%s' "$up" | grep -q "api.github.com"; then
  echo "SKIP  update --plan: GitHub's API is out of reach ($up)"
else
  check "update --plan names this binary, then says no release exists or names the latest" \
    'printf "%s" "$up" | head -n 1 | grep -q "^knowledge [0-9.]*, " && printf "%s" "$up" | grep -qE "^(No release is published yet at |latest release: v)"'
fi
check "update --apply is refused with the reason" \
  '! k update --apply x > "$TMP/ua.txt" 2>&1 && grep -q "not built" "$TMP/ua.txt"'
unset KNOWLEDGE_HOME

# --- S9.5: the release key and docs/verify.md's commands -----------------------------------------
AS="$ROOT/allowed_signers"
check "allowed_signers is one ed25519 key, for file signatures only" \
  '[ "$(grep -cv "^[[:space:]]*$" "$AS")" = 1 ] && grep -qE "^atlasatakahraman namespaces=\"file\" ssh-ed25519 [A-Za-z0-9+/=]+ knowledge-release$" "$AS"'
awk '{print $3, $4}' "$AS" > "$TMP/release.pub"
fp=$(ssh-keygen -lf "$TMP/release.pub" 2>/dev/null | awk '{print $2}')
check "docs/verify.md names that key and its fingerprint" \
  '[ -n "$fp" ] && grep -qF "$fp" "$ROOT/docs/verify.md" && grep -qF "$(cat "$AS")" "$ROOT/docs/verify.md"'
# verify.md's own command, against a throwaway key: a good signature passes, and a changed
# SHA256SUMS or a signature for another namespace (a commit's) does not.
# macOS has shasum, not sha256sum; verify.md says so.
SUM=sha256sum; command -v sha256sum > /dev/null || SUM="shasum -a 256"
mkdir "$TMP/sig" && (
  cd "$TMP/sig" || exit 1
  ssh-keygen -q -t ed25519 -N "" -C fixture -f k < /dev/null
  printf 'x' > a.zip
  printf '%s  a.zip\n' "$($SUM a.zip | cut -c1-64)" > SHA256SUMS
  printf 'atlasatakahraman namespaces="file" %s\n' "$(cat k.pub)" > allowed_signers
  ssh-keygen -Y sign -f k -n file SHA256SUMS < /dev/null
  cp SHA256SUMS commit && ssh-keygen -Y sign -f k -n git commit < /dev/null
) > /dev/null 2>&1
sigv() { ssh-keygen -Y verify -f "$TMP/sig/allowed_signers" -I atlasatakahraman -n file -s "$1" < "$2" > /dev/null 2>&1; }
check "verify.md's signature command passes a good signature" 'sigv "$TMP/sig/SHA256SUMS.sig" "$TMP/sig/SHA256SUMS"'
check "and fails a changed SHA256SUMS" \
  'cp "$TMP/sig/SHA256SUMS" "$TMP/sig/bad" && echo x >> "$TMP/sig/bad" && ! sigv "$TMP/sig/SHA256SUMS.sig" "$TMP/sig/bad"'
check "and fails the same key's signature for a commit (namespace git)" \
  '[ -f "$TMP/sig/commit.sig" ] && ! sigv "$TMP/sig/commit.sig" "$TMP/sig/SHA256SUMS"'
check "verify.md's checksum command passes the archive and fails a changed one" \
  '(cd "$TMP/sig" && grep -F "  a.zip" SHA256SUMS | $SUM -c - > /dev/null && printf y > a.zip && ! grep -F "  a.zip" SHA256SUMS | $SUM -c - > /dev/null 2>&1)'

rm -rf "$TMP"
echo
if [ "$fails" = 0 ]; then echo "stage 9: all checks pass"; else echo "stage 9: $fails check(s) FAIL"; exit 1; fi
