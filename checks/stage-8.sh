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

# --- S8.2: find --code and graph --code -------------------------------------------------------
# A second Rust `beta`, a chain epsilon -> delta -> beta inside one file, and a JS `beta` that no
# Rust call may reach.
cat > "$FIX/src/other.rs" <<'EOF'
pub fn beta(n: u32) -> u32 {
    n
}
pub fn delta() -> u32 {
    beta(1)
}
pub fn epsilon() -> u32 {
    delta()
}
EOF
printf 'export function beta() {\n  return 0;\n}\n' > "$FIX/web/lib.mjs"
g add -A && g commit -q -m s8.2
out=$(k find beta --code --repo fixture)
check "find --code: the definition, as repo path:line kind signature" 'echo "$out" | grep -qx "fixture src/beta.rs:1 function pub fn beta(n: u32) -> u32"'
check "find --code: every definition of the name (3)" '[ "$(echo "$out" | grep -c " beta")" = 3 ]'
check "find --code: inside a checkout, no --repo needed" '(cd "$FIX/web" && k find badge --code) | grep -q "^fixture web/page.tsx:4 function"'
out=$(k graph beta --code --repo fixture)
check "graph --code: a caller in the same file links to that file's definition only" 'echo "$out" | grep -qx "    src/beta.rs:4 gamma function"'
check "graph --code: a caller with two candidates is marked ambiguous" 'echo "$out" | grep -qx "    src/alpha.rs:1 alpha function \[ambiguous: 2 by this name\]"'
check "graph --code: a Rust call never reaches a JS symbol" '[ "$(echo "$out" | awk "/^beta — fixture web\/lib.mjs/{f=1} f&&/used by/{print; exit}")" = "  used by (0):" ]'
check "graph --code: a JSX use is a render" 'k graph Badge --code --repo fixture | grep -qx "    web/page.tsx:1 Page function \[render\]"'
chain=$'fixture:\n  src/other.rs:7 epsilon function\n  → src/other.rs:4 delta function\n  → src/other.rs:1 beta function'
check "graph --code <a> <b>: the chain of calls between them" '[ "$(k graph epsilon beta --code --repo fixture)" = "$chain" ]'
check "graph --code <b> <a>: the same chain, in call order" '[ "$(k graph beta epsilon --code --repo fixture)" = "$chain" ]'
check "graph --code: no chain is said so" 'k graph label alpha --code --repo fixture | grep -q "^no chain of uses between label and alpha"'
check "graph --code: held to the budget" 'k graph beta --code --repo fixture --budget 200 | grep -q "past the 200-char budget"'
check "a --repo that is a path is refused" '! k find beta --code --repo ../fixture 2>/dev/null'
mcp() { printf '%s\n' "$1" | k mcp; }
call() { printf '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"%s","arguments":%s}}' "$1" "$2"; }
check "mcp: find with code answers from the code map" 'mcp "$(call find "{\"q\":\"epsilon\",\"code\":true,\"repo\":\"fixture\"}")" | grep -q "fixture src/other.rs:7 function"'
check "mcp: graph with code lists the uses" 'mcp "$(call graph "{\"note\":\"delta\",\"code\":true,\"repo\":\"fixture\"}")" | grep -q "src/other.rs:7 epsilon function"'
check "mcp: a path for repo is an error, not a read" 'mcp "$(call find "{\"q\":\"beta\",\"code\":true,\"repo\":\"../fixture\"}")" | grep -q "\"isError\":true"'
check "mcp: repo without code is refused" 'mcp "$(call graph "{\"note\":\"x\",\"repo\":\"fixture\"}")" | grep -q "repo: only with code"'

# --- S8.3: the duplicate audit ------------------------------------------------------------------
# Committed: a body long enough to have a shape, a component in one app, and that app's Next.js
# page. Then staged: the body under a new name, the component in a second app, the second app's
# page, and an unrelated new function. Only the first two are copies.
mkdir -p "$FIX/apps/one/src/app" "$FIX/apps/two/src/app"
cat > "$FIX/src/sum.rs" <<'EOF'
pub fn total(xs: &[u32]) -> u32 {
    let mut t = 0;
    for x in xs {
        if *x > 10 {
            t += x * 2;
        } else {
            t += x + 1;
        }
    }
    t
}
EOF
printf 'export function AppNav() {\n  return <nav>one</nav>;\n}\n' > "$FIX/apps/one/src/nav.tsx"
printf 'export default function Page() {\n  return <main>one</main>;\n}\n' > "$FIX/apps/one/src/app/page.tsx"
g add -A && g commit -q -m s8.3-base
sed 's/fn total/fn grand_total/' "$FIX/src/sum.rs" > "$FIX/src/copy.rs"
printf 'export function AppNav() {\n  return <nav>two</nav>;\n}\n' > "$FIX/apps/two/src/nav.tsx"
printf 'export default function Page() {\n  return <main>two</main>;\n}\n' > "$FIX/apps/two/src/app/page.tsx"
printf 'pub fn fresh() -> u32 {\n    7\n}\n' > "$FIX/src/fresh.rs"
g add -A
out=$(cd "$FIX" && k code duplicate --staged)
check "duplicate: a copy under a new name is listed" 'echo "$out" | grep -qx "duplicate: src/copy.rs:1 function grand_total: the same body as src/sum.rs:1 total"'
check "duplicate: a same-name component in a sibling app is listed" 'echo "$out" | grep -qx "duplicate: apps/two/src/nav.tsx:1 function AppNav: also in apps/one: apps/one/src/nav.tsx:1"'
check "duplicate: a Next.js convention file in a sibling app is not" '! echo "$out" | grep -q "Page"'
check "duplicate: an unrelated new function is not" '! echo "$out" | grep -q "fresh"'
check "duplicate: exactly the two findings, under one heading" '[ "$(echo "$out" | grep -c "^duplicate: ")" = 3 ]'
if command -v gitleaks >/dev/null 2>&1; then
  # The real hook, with this build first on PATH and a HOME of its own: its `rules check` reads
  # the instructions files under HOME, and the owner's are not this check's to read.
  ROOT=$(cd "$(dirname "$0")/.." && pwd)
  mkdir -p "$TMP/bin" "$TMP/user"
  for b in knowledge knowledge-code; do
    src="$(dirname "$BIN")/$b"; [ -f "$src.exe" ] && src="$src.exe"
    cp "$src" "$TMP/bin/"
  done
  rm -rf "$TMP/vault" && cp -R "$ROOT/template" "$TMP/vault"
  hooked=$(HOME="$TMP/user" USERPROFILE="$TMP/user" PATH="$TMP/bin:$PATH" \
    git -C "$FIX" -c user.name=check -c user.email=check@example.invalid \
      -c core.hooksPath="$(native "$ROOT/githooks")" commit -q -m s8.3 2>&1)
  check "duplicate: the commit goes through the hook" '[ "$(g log -1 --format=%s)" = s8.3 ]'
  check "duplicate: and the hook shows the warning" 'echo "$hooked" | grep -q "^duplicate: src/copy.rs:1"'
else
  echo "SKIP  duplicate through the pre-commit hook: gitleaks not on PATH"
  g commit -q -m s8.3
fi
check "duplicate: what HEAD already has is not listed again" '[ -z "$(cd "$FIX" && k code duplicate)" ]'

# --- S8.4: uses of a type are refs ----------------------------------------------------------------
# A Rust struct, a TS interface and a Python class, each used only as a parameter's type: no call
# reaches them, so before S8.4 each had "used by (0)".
printf 'pub struct Gauge {\n    pub v: u32,\n}\n' > "$FIX/src/gauge.rs"
printf 'pub fn read(g: &Gauge) -> u32 {\n    g.v\n}\n' > "$FIX/src/read.rs"
printf 'export interface Shape {\n  w: number;\n}\n' > "$FIX/web/shape.ts"
printf 'export function area(s: Shape): number {\n  return s.w;\n}\n' > "$FIX/web/area.ts"
printf 'class Point:\n    pass\n\ndef norm(p: Point):\n    return 0\n' > "$FIX/tools/point.py"
g add -A && g commit -q -m s8.4
check "graph --code: a Rust parameter type is a [type] use" 'k graph Gauge --code --repo fixture | grep -qx "    src/read.rs:1 read function \[type\]"'
check "graph --code: a TS annotation is a [type] use" 'k graph Shape --code --repo fixture | grep -qx "    web/area.ts:1 area function \[type\]"'
check "graph --code: a Python annotation is a [type] use" 'k graph Point --code --repo fixture | grep -qx "    tools/point.py:4 norm function \[type\]"'
# Another file reaches only what it can name: not a module's unexported `split`, not a private Rust
# `State`, and a Rust `v.join()` is a method, never the free `join`. A TS `type` alias is a symbol,
# and a function handed on as a value is used.
printf 'function split(p: string): string[] {\n  return [p];\n}\nfunction shout(s: string): string {\n  return s;\n}\nexport { shout };\n' > "$FIX/web/util.ts"
printf 'export function menu(s: string) {\n  return s.split(",").map(shout);\n}\n' > "$FIX/web/menu.ts"
printf 'export type Mode = "on" | "off";\nexport function flip(m: Mode): Mode {\n  return m;\n}\n' > "$FIX/web/mode.ts"
printf 'pub fn join(a: u32) -> u32 {\n    a\n}\nstruct State;\n' > "$FIX/src/path.rs"
printf 'pub fn walk(v: Vec<String>, s: tauri::State<u32>) -> String {\n    v.join(",")\n}\n' > "$FIX/src/walk.rs"
g add -A && g commit -q -m s8.4-reach
check "graph --code: an unexported function is not another file's" 'k graph split --code --repo fixture | grep -qx "  used by (0):"'
check "graph --code: a function exported by name, handed on as a value, is a [value] use" 'k graph shout --code --repo fixture | grep -qx "    web/menu.ts:1 menu function \[value\]"'
check "graph --code: a TS type alias is a symbol with its [type] uses" 'k graph Mode --code --repo fixture | grep -qx "    web/mode.ts:2 flip function \[type\]"'
check "graph --code: a Rust method call never reaches a free function" 'k graph join --code --repo fixture | grep -qx "  used by (0):"'
check "graph --code: a private Rust struct is not another file's" 'k graph State --code --repo fixture | grep -qx "  used by (0):"'

# --- S8.4b: a Tauri command's front-end call and its registration --------------------------------
# The front end names a command by string, and `generate_handler!` registers it by path inside a
# macro, so before S8.4b a command had "used by (0)". A command in an inline `mod` is a function,
# and an `invoke` of a name no handler registers links nothing.
printf 'pub mod commands {\n    pub fn ping() -> u32 {\n        1\n    }\n    pub fn stray() -> u32 {\n        2\n    }\n}\n' > "$FIX/src/cmds.rs"
printf 'pub fn run() {\n    tauri::generate_handler![crate::cmds::commands::ping];\n}\n' > "$FIX/src/app.rs"
printf 'export function pingNow() {\n  return invoke("ping");\n}\nexport function strayNow() {\n  return invoke("stray");\n}\n' > "$FIX/web/ipc.ts"
g add -A && g commit -q -m s8.4b-tauri
out=$(k graph ping --code --repo fixture)
check "graph --code: a command in an inline mod is a function" 'echo "$out" | grep -q "^ping — fixture src/cmds.rs:2 function"'
check "graph --code: a front-end invoke of the name is an [invoke] use" 'echo "$out" | grep -qx "    web/ipc.ts:1 pingNow function \[invoke\]"'
check "graph --code: generate_handler registers it, its only other user" 'echo "$out" | grep -qx "    src/app.rs:1 run function \[register\]" && echo "$out" | grep -qx "  used by (2):"'
check "graph --code: an unregistered command is not reached by invoke" 'k graph stray --code --repo fixture | grep -qx "  used by (0):"'
check "graph --code <a> <b>: the chain crosses from TS to Rust" '! k graph pingNow ping --code --repo fixture | grep -q "^no chain"'

rm -rf "$TMP"
echo "$fails failed"
[ "$fails" -eq 0 ]
