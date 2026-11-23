#!/usr/bin/env bash
# Stage 6's runnable check: `knowledge setup` on a fixture HOME. `--plan` writes nothing; `--apply`
# changes exactly the files the plan names; a file touched between plan and apply makes `--apply`
# refuse with the tree unchanged; `--remove --plan` names no note and `--remove --apply` leaves the
# vault as it was. Builds nothing. Run from the repo root:
#   bash checks/stage-6.sh target/release/knowledge
set -u
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
ROOT=$(cd "$(dirname "$0")/.." && pwd)
fails=0
pass() { echo "PASS  $1"; }
fail() { echo "FAIL  $1"; fails=$((fails + 1)); }
check() { if eval "$2"; then pass "$1"; else fail "$1"; fi; }
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }

TMP=$(mktemp -d)
FX=$(native "$TMP")
export HOME="$TMP/home" USERPROFILE="$TMP/home"
unset KNOWLEDGE_CONFIG KNOWLEDGE_HOME XDG_CONFIG_HOME CLAUDE_CONFIG_DIR
# The hook block names this build. Without graphify-mcp on PATH the plan registers no MCP server,
# so the check never runs the real `claude` against the fixture HOME.
export KNOWLEDGE_BIN="$BIN"
PATH=$(printf '%s' "$PATH" | tr ':' '\n' | while IFS= read -r d; do
  [ -e "$d/graphify-mcp" ] || [ -e "$d/graphify-mcp.exe" ] || printf '%s\n' "$d"
done | paste -sd: -)
export PATH

# The skeleton as this working tree has it (a clone would take the last commit), and an adopted
# vault from its template with one finished daily log for the compile state to seed.
SK="$TMP/sk"
mkdir -p "$SK" "$HOME/.claude"
for d in hooks commands skills githooks template; do cp -R "$ROOT/$d" "$SK/$d"; done
rm -rf "$SK/hooks/.state"
cp "$ROOT/settings.hooks.json" "$ROOT/package.json" "$SK/"
git -C "$SK" init -q
VAULT="$TMP/vault"
cp -R "$ROOT/template" "$VAULT"
mkdir -p "$VAULT/30-Sessions/2026/01"
printf '# 2026-01-01\n\nA finished log.\n' > "$VAULT/30-Sessions/2026/01/2026-01-01.md"
git -C "$VAULT" init -q
# Someone else's hook and setting, which setup must keep, and a CLAUDE.md of the user's own.
cat > "$HOME/.claude/settings.json" <<'EOF'
{
  "model": "fixture",
  "hooks": {
    "Stop": [
      { "hooks": [{ "type": "command", "command": "echo someone-elses" }] }
    ]
  }
}
EOF
printf '# Mine\n\nThe user'"'"'s own instructions.\n' > "$HOME/.claude/CLAUDE.md"

k() { "$BIN" setup "$@" --vault "$(native "$VAULT")" --dev "$(native "$TMP/dev")" --skeleton "$(native "$SK")"; }
# Every file, link and empty folder under the fixture with its content (a link: its target), git's
# own object store and index left out: `git config` writes only .git/config.
snap() {
  (cd "$TMP" && find . -path ./out -prune -o \( ! -type d -o -type d -empty \) -print | grep -v -e '/\.git/objects/' -e '/\.git/logs/' -e '/\.git/index$' | LC_ALL=C sort |
    while IFS= read -r f; do
      if [ -L "$f" ]; then printf '%s L %s\n' "$FX/${f#./}" "$(readlink "$f")"
      elif [ -d "$f" ]; then printf '%s D\n' "$FX/${f#./}"; else printf '%s %s\n' "$FX/${f#./}" "$(cksum < "$f")"; fi
    done)
}
changed() { diff <(printf '%s\n' "$1") <(printf '%s\n' "$2") | sed -n 's/^[<>] \([^ ]*\) .*/\1/p' | LC_ALL=C sort -u; }
# The paths a plan's create/change/remove lines name: the target, and any copy kept beside it. The
# detail starts at column 28 ("  " + status in 7 + " " + name in 16 + " ").
listed() {
  printf '%s\n' "$1" | grep -E '^  (create|change|remove) ' | cut -c28- | while IFS= read -r l; do
    first="${l%%: *}"; first="${first%% -> *}"; printf '%s\n' "$first"
    case "$l" in *"kept as "*) kept="${l##*kept as }"; printf '%s\n' "${kept%)}" ;; esac
  done | LC_ALL=C sort -u
}
plan_id() { printf '%s\n' "$1" | tail -n 1 | sed -n 's/^plan \([0-9a-f]\{16\}\)$/\1/p'; }

mkdir -p "$TMP/out"
before=$(snap)
plan=$(k --plan); code=$?
id=$(plan_id "$plan")
check "--plan exits 0 and ends with the plan id" '[ "$code" = 0 ] && [ -n "$id" ]'
check "--plan leaves the tree hash unchanged" '[ "$(snap)" = "$before" ]'
check "--plan twice gives the same id" '[ "$(plan_id "$(k --plan)")" = "$id" ]'
check "--plan names the adopted vault, the links, the hook block and the compile state" \
  'printf "%s" "$plan" | grep -q "(adopted)" && printf "%s" "$plan" | grep -q "^  create  link " && printf "%s" "$plan" | grep -q "^  change  settings.json" && printf "%s" "$plan" | grep -q "1 finished log(s) marked compiled"'

# A file touched between plan and apply: refused, nothing written. Put back, the plan is current again.
md="$HOME/.claude/CLAUDE.md"
cp "$md" "$TMP/out/CLAUDE.md"
printf 'touched\n' >> "$md"
touched=$(snap)
k --apply "$id" > "$TMP/out/refused.txt" 2>&1; code=$?
check "--apply refuses a plan a touched file made stale" '[ "$code" != 0 ] && grep -q "the plan changed since --plan" "$TMP/out/refused.txt"'
check "--apply that refuses leaves the tree unchanged" '[ "$(snap)" = "$touched" ]'
cp "$TMP/out/CLAUDE.md" "$md"
check "putting the file back makes the plan current again" '[ "$(plan_id "$(k --plan)")" = "$id" ]'

k --apply "$id" > "$TMP/out/apply.txt" 2>&1; code=$?
after=$(snap)
check "--apply exits 0" '[ "$code" = 0 ]'
check "--apply changes exactly the files the plan lists" '[ "$(changed "$before" "$after")" = "$(listed "$plan")" ]'
[ "$(changed "$before" "$after")" = "$(listed "$plan")" ] || diff <(changed "$before" "$after") <(listed "$plan") | sed 's/^/      /'
check "a second --plan has nothing to change" 'k --plan | grep -q "^Nothing to change.$"'
s="$HOME/.claude/settings.json"
check "the hook block names only knowledge, and keeps someone else's hook and setting" \
  'grep -q "echo someone-elses" "$s" && grep -q "\"model\": \"fixture\"" "$s" && ! grep -q "bun" "$s" && [ "$(grep -c " hook [a-z-]* --harness claude" "$s")" = 10 ]'
check "CLAUDE.md keeps the user's text and imports Identity and Hard-Rules" \
  'grep -q "^# Mine$" "$md" && grep -q "^@.*/00-Meta/Identity.md$" "$md" && grep -q "^@.*/00-Meta/Hard-Rules.md$" "$md"'

vault_before=$(snap | grep "^$FX/vault/" | grep -v "/\.git/config ")
rplan=$(k --remove --plan); code=$?
rid=$(plan_id "$rplan")
check "--remove --plan exits 0 and names no note" '[ "$code" = 0 ] && [ -n "$rid" ] && ! listed "$rplan" | grep -q "^$FX/vault/.*\.md$"'
check "--remove --plan leaves the tree unchanged" '[ "$(snap)" = "$after" ]'
k --remove --apply "$rid" > "$TMP/out/remove.txt" 2>&1; code=$?
check "--remove --apply exits 0" '[ "$code" = 0 ]'
# The folders the removed links leave empty stay: setup cannot tell whether it made them.
check "--remove --apply changes exactly the files its plan lists" \
  '[ "$(changed "$after" "$(snap)" | grep -v -e "^$FX/home/.claude/commands$" -e "^$FX/home/.claude/skills$")" = "$(listed "$rplan")" ]'
check "--remove leaves the vault as it was" '[ "$(snap | grep "^$FX/vault/" | grep -v "/\.git/config ")" = "$vault_before" ]'
check "--remove takes our hooks and links out and keeps the rest" \
  '! grep -q " hook " "$s" && grep -q "echo someone-elses" "$s" && [ ! -e "$HOME/.claude/hooks" ] && [ "$(cat "$md")" = "$(cat "$TMP/out/CLAUDE.md")" ]'

rm -rf "$TMP"
echo
if [ "$fails" = 0 ]; then echo "stage 6: all checks pass"; else echo "stage 6: $fails check(s) FAIL"; exit 1; fi
