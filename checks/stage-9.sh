#!/usr/bin/env bash
# Stage 9's runnable check (S9.3, layout and conventions as config): a vault with two folders
# renamed through config.json passes the doctor's vault checks; a bad `folders` is refused; a vault
# made by `setup --from-template` passes them with the generic packs locked; no folder name is
# written in src/ outside config.rs. Every run is on a fixture HOME and KNOWLEDGE_HOME, so the real
# state never moves. Builds nothing. Run from the repo root:
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
export HOME="$TMP/user" USERPROFILE="$TMP/user" KNOWLEDGE_BIN="$BIN"
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
for d in hooks commands skills githooks template claude; do cp -R "$ROOT/$d" "$SK/$d"; done
rm -rf "$SK/hooks/.state"
cp "$ROOT/package.json" "$SK/"
git -C "$SK" init -q
ver=$(sed -n 's/^ *"version": "\(.*\)",$/\1/p' "$ROOT/claude/.claude-plugin/plugin.json")
printf '{"knowledge-skeleton": {"source": {"source": "git", "url": "fixture"}}}\n' > "$HOME/.claude/plugins/known_marketplaces.json"
printf '{"version": 2, "plugins": {"knowledge@knowledge-skeleton": [{"scope": "user", "version": "%s"}]}}\n' "$ver" > "$HOME/.claude/plugins/installed_plugins.json"
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

# --- No folder name in src/ outside config.rs (comment lines left out) ---------------------------
hits=$(grep -nE '"[^"]*(00-Meta|10-Repos|20-Knowledge|30-Sessions|40-Plans|50-Ops|60-Decisions|80-Assets|90-Inbox)' "$ROOT"/src/*.rs |
  grep -v '/src/config\.rs:' | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//')
check "src/ names no folder outside config.rs" '[ -z "$hits" ]'
[ -z "$hits" ] || printf '%s\n' "$hits" | sed 's/^/      /'

rm -rf "$TMP"
echo
if [ "$fails" = 0 ]; then echo "stage 9: all checks pass"; else echo "stage 9: $fails check(s) FAIL"; exit 1; fi
