#!/usr/bin/env bash
# Stage 8's runnable check: the code map, on a fixture checkout with a fixture KNOWLEDGE_HOME, so
# no real checkout or cache is read or written. Builds nothing. Run from the repo root:
#   bash checks/stage-8.sh target/release/knowledge
set -u
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
fails=0
pass() { echo "PASS  $1"; }
fail() { echo "FAIL  $1"; fails=$((fails + 1)); }
check() { if eval "$2"; then pass "$1"; else fail "$1"; fi; }
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }

TMP=$(mktemp -d)
DEV="$TMP/dev"
FIX="$DEV/fixture"
export KNOWLEDGE_HOME="$TMP/home"
unset KNOWLEDGE_CONFIG
mkdir -p "$KNOWLEDGE_HOME" "$TMP/vault" "$FIX/src" "$FIX/web" "$FIX/tools"
printf '{"vault": "%s", "devRoot": "%s"}\n' "$(native "$TMP/vault")" "$(native "$DEV")" > "$KNOWLEDGE_HOME/config.json"
k() { "$BIN" "$@"; }
g() { git -C "$FIX" -c user.name=check -c user.email=check@example.invalid -c core.hooksPath= -c core.autocrlf=false "$@"; }

cat > "$FIX/src/alpha.rs" <<'EOF'
pub fn alpha(n: u32) -> u32 {
    crate::beta::beta(n) + 1
}
EOF
cat > "$FIX/src/beta.rs" <<'EOF'
pub fn beta(n: u32) -> u32 {
    n * 2
}
EOF
cat > "$FIX/web/page.tsx" <<'EOF'
export function Page() {
  return <main><Badge />{label("home")}</main>;
}
function Badge() {
  return <b>new</b>;
}
function label(s: string): string {
  return s.toUpperCase();
}
EOF
cat > "$FIX/tools/run.py" <<'EOF'
def run():
    return helper()

def helper():
    return 1
EOF
# Over the 1 MB cap: never parsed.
head -c 1100000 /dev/zero | tr '\0' 'a' > "$FIX/web/big.js"
g init -q -b main && g add -A && g commit -q -m fixture

counts() { sed -n 's/.*; \([0-9]*\) symbols, \([0-9]*\) refs.*/\1 \2/p'; }

# --- S8.1: the store and the indexer ------------------------------------------------------------
out=$(k code index fixture)
check "index: the four files under the cap, all parsed ($out)" 'echo "$out" | grep -q "fixture: 4 files: 4 parsed"'
# alpha -> beta through a path call, Page -> label, Page -> Badge through JSX, run -> helper.
check "index: 7 symbols and 4 refs, a path call and a JSX render among them" '[ "$(echo "$out" | counts)" = "7 4" ]'
check "index: the cache is in the knowledge home" '[ -f "$KNOWLEDGE_HOME/code/fixture.db" ]'
check "index: nothing inside the checkout" '[ -z "$(g status --porcelain)" ]'
out=$(k code index fixture)
check "a second run parses no file" 'echo "$out" | grep -q "0 parsed, 4 unchanged"'
sleep 1 && touch "$FIX/src/alpha.rs"
out=$(k code index fixture)
check "a touched, unchanged file is hashed, not parsed" 'echo "$out" | grep -q "0 parsed, 4 unchanged"'
printf 'pub fn gamma() -> u32 {\n    beta(3)\n}\n' >> "$FIX/src/beta.rs"
out=$(k code index fixture)
check "an edited file is the only one parsed" 'echo "$out" | grep -q "1 parsed, 3 unchanged"'
incremental=$(echo "$out" | counts)
out=$(cd "$FIX/web" && k code index --rebuild)
check "inside a checkout, no name needed; --rebuild parses all" 'echo "$out" | grep -q "fixture: 4 files: 4 parsed"'
check "a rebuild gives the incremental run's rows ($incremental)" '[ "$(echo "$out" | counts)" = "$incremental" ]'
rm "$FIX/tools/run.py"
check "a deleted file is removed" 'k code index fixture | grep -q "1 removed"'
check "a name that is not a checkout is refused" '! k code index nope 2>/dev/null'

rm -rf "$TMP"
echo "$fails failed"
[ "$fails" -eq 0 ]
