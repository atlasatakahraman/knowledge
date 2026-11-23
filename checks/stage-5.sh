#!/usr/bin/env bash
# Stage 5's runnable check: the rules lock, the guard, the commit-time backstop and the restore,
# on a fixture vault in a temp KNOWLEDGE_HOME. Builds nothing. Run from the repo root:
#   bash checks/stage-5.sh target/release/knowledge
# The commit-time checks SKIP without gitleaks on PATH; the timing line SKIPs without bun.
set -u
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
ROOT=$(cd "$(dirname "$0")/.." && pwd)
fails=0
pass() { echo "PASS  $1"; }
fail() { echo "FAIL  $1"; fails=$((fails + 1)); }
skip() { echo "SKIP  $1"; }
check() { if eval "$2"; then pass "$1"; else fail "$1"; fi; }
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }

TMP=$(mktemp -d)
VAULT="$TMP/vault"
export KNOWLEDGE_HOME="$TMP/home"
# `rules check --restore` repairs the instructions blocks under HOME: a fixture vault must never
# reach the real ~/.claude/CLAUDE.md or ~/.codex/AGENTS.md (Stage 7).
export HOME="$TMP/user" USERPROFILE="$TMP/user"
unset KNOWLEDGE_CONFIG
mkdir -p "$KNOWLEDGE_HOME" "$TMP/bin" "$HOME"
cp -R "$ROOT/template" "$VAULT"
printf '{"vault": "%s", "devRoot": "%s"}\n' "$(native "$VAULT")" "$(native "$TMP")" > "$KNOWLEDGE_HOME/config.json"
# The pre-commit hook calls `knowledge` from PATH: this build, not the installed one. On Windows
# CI passes the path without `.exe`, and the copy keeps the extension so sh can run it.
src="$BIN"; [ -f "$BIN.exe" ] && src="$BIN.exe"
cp "$src" "$TMP/bin/$(basename "$src")"
export PATH="$TMP/bin:$PATH"
k() { "$BIN" "$@"; }
# BSD sed's -i takes a suffix argument and GNU's does not, so edit through a temp file.
sub() { sed "$1" "$2" > "$2.tmp" && mv "$2.tmp" "$2"; }
g() { git -C "$VAULT" -c user.name=check -c user.email=check@example.invalid -c core.hooksPath= "$@"; }

RULES="$VAULT/00-Meta/Hard-Rules.md"
cat > "$RULES" <<'EOF'
# Hard Rules

Fixture rules for Stage 5's check.

## Package management

- **Bun always.** `bun install`, `bun add`, `bun run`.
- **Never npm, pnpm, or yarn.** A `package-lock.json` is contamination.
- **Never bare bunx.** `bunx --bun` only.

## Safety

- **Rules change only on a yes.** The owner edits this note, then runs `knowledge rules lock`.
- **Ask before every push.** Each push needs its own yes.
- **Never write to F.** The drive is end of life.
EOF
SEED="$TMP/seed.json"
cat > "$SEED" <<'EOF'
{
  "packs": { "": "head", "Package management": "bun", "Safety": "safety" },
  "rules": {
    "bun-always": { "kind": "guard", "enforce": ["P", "G", "C"] },
    "never-npm-pnpm-or-yarn": { "kind": "guard", "enforce": ["P", "G", "C"] },
    "never-bare-bunx": { "kind": "guard", "enforce": ["P", "G"] },
    "rules-change-only-on-a-yes": { "kind": "guard", "enforce": ["P", "G", "D"] },
    "ask-before-every-push": { "kind": "agent", "enforce": ["P", "G"] },
    "never-write-to-f": { "kind": "guard", "enforce": ["P", "G"] }
  },
  "guards": [
    { "id": "no-npm", "rule": "never-npm-pnpm-or-yarn", "action": "deny", "reason": "bun always",
      "words": [["npm"], ["pnpm"], ["yarn"], ["npx"]],
      "staged": ["package-lock.json", "yarn.lock", "pnpm-lock.yaml"] },
    { "id": "bare-bunx", "rule": "never-bare-bunx", "action": "deny", "reason": "bunx --bun",
      "words": [["bunx"]], "unless": ["--bun"] },
    { "id": "rules-files", "rule": "rules-change-only-on-a-yes", "action": "ask", "reason": "the owner's yes",
      "words": [["knowledge", "rules", "lock"]],
      "paths": ["*/00-meta/hard-rules.md", "*/00-meta/rules.lock"] },
    { "id": "push", "rule": "ask-before-every-push", "action": "ask", "reason": "each push its own yes",
      "words": [["git", "push"], ["git", "reset", "--hard"]] },
    { "id": "production", "rule": "ask-before-every-push", "action": "ask", "reason": "production",
      "tools": ["*__apply_migration"] },
    { "id": "f-drive", "rule": "never-write-to-f", "action": "deny", "reason": "end of life",
      "paths": ["f:*", "/f/*"] }
  ]
}
EOF
g init -q -b main
g add -A && g commit -q -m "fixture"

# --- S5.2: the lock ------------------------------------------------------------------------------
check "rules lock --seed writes the lock" 'k rules lock --seed "$(native "$SEED")" >/dev/null 2>&1 && [ -f "$VAULT/00-Meta/rules.lock" ]'
check "rules check: a fresh lock matches" 'k rules check >/dev/null 2>&1'
check "rules list names the six guards" 'k rules list | grep -q "^6 guards: no-npm"'
g add -A && g commit -q -m "lock"
sub 's/A `package-lock.json` is contamination./A `package-lock.json` is fine./' "$RULES"
out=$(k rules check 2>&1); code=$?
check "check: one weakened bullet fails, exit 1" '[ "$code" = 1 ]'
check "check: names that rule" 'printf "%s" "$out" | grep -q "never-npm-pnpm-or-yarn changed"'
check "check: names only that rule" '! printf "%s" "$out" | grep -q "bun-always\|never-bare-bunx\|ask-before"'
out=$(k rules check --restore 2>&1)
check "--restore brings it back from HEAD, with one line" 'printf "%s" "$out" | grep -q "restored from HEAD: never-npm-pnpm-or-yarn" && k rules check >/dev/null 2>&1'
check "--restore leaves the working tree clean" '[ -z "$(g status --porcelain)" ]'
sub 's/A `package-lock.json` is contamination./A `package-lock.json` is fine./' "$RULES"
g commit -q -am "weaken without a relock"
out=$(k rules check --restore 2>&1); code=$?
check "--restore refuses when HEAD disagrees too, exit 1" '[ "$code" = 1 ] && printf "%s" "$out" | grep -q "^!! RULES"'
check "--restore touched nothing then" 'grep -q "is fine" "$RULES"'
g reset -q --hard HEAD~1

# --- S5.3: the guard -----------------------------------------------------------------------------
V=$(native "$VAULT")
pre() { printf '%s' "$1" | k hook pre-tool --harness claude 2>/dev/null; }
decides() { pre "$1" | grep -q "\"permissionDecision\":\"$2\""; }
bash_p() { printf '{"tool_name":"Bash","tool_input":{"command":%s},"cwd":"%s"}' "$1" "$V"; }
check "guard: npm i is denied" 'decides "$(bash_p "\"npm i\"")" deny'
check "guard: cd x && npx y is denied" 'decides "$(bash_p "\"cd x && npx y\"")" deny'
check "guard: bare bunx is denied" 'decides "$(bash_p "\"bunx shadcn\"")" deny'
check "guard: bunx --bun prints nothing" '[ -z "$(pre "$(bash_p "\"bunx --bun shadcn\"")")" ]'
check "guard: an Edit of Hard-Rules asks" 'decides "{\"tool_name\":\"Edit\",\"tool_input\":{\"file_path\":\"$V/00-Meta/Hard-Rules.md\"},\"cwd\":\"$V\"}" ask'
check "guard: sed -i on Hard-Rules asks" 'decides "$(bash_p "\"sed -i s/a/b/ 00-Meta/Hard-Rules.md\"")" ask'
check "guard: a redirect onto the lock asks" 'decides "$(bash_p "\"echo {} > 00-Meta/rules.lock\"")" ask'
check "guard: PowerShell Set-Content on Hard-Rules asks" 'decides "{\"tool_name\":\"PowerShell\",\"tool_input\":{\"command\":\"Set-Content 00-Meta/Hard-Rules.md x\"},\"cwd\":\"$V\"}" ask'
check "guard: knowledge rules lock asks" 'decides "$(bash_p "\"knowledge rules lock\"")" ask'
check "guard: git push asks" 'decides "$(bash_p "\"git -C x push origin main\"")" ask'
check "guard: an MCP apply_migration asks" 'decides "{\"tool_name\":\"mcp__db__apply_migration\",\"tool_input\":{},\"cwd\":\"$V\"}" ask'
check "guard: a Write under F: is denied" 'decides "{\"tool_name\":\"Write\",\"tool_input\":{\"file_path\":\"F:/x.txt\"},\"cwd\":\"$V\"}" deny'
check "guard: git log prints nothing" '[ -z "$(pre "$(bash_p "\"git log --oneline\"")")" ]'
check "guard: cat Hard-Rules prints nothing" '[ -z "$(pre "$(bash_p "\"cat 00-Meta/Hard-Rules.md\"")")" ]'
check "guard: a commit message naming npm prints nothing" '[ -z "$(pre "$(bash_p "\"git commit -m \\\"drop npm i\\\"\"")")" ]'
WRAP="bun -e \\\"require('fs').writeFileSync('00-Meta/Hard-Rules.md', 'x')\\\""
check "guard: a bun -e wrapper passes, by design (its backstop is the restore)" '[ -z "$(pre "$(bash_p "\"$WRAP\"")")" ]'
mv "$VAULT/00-Meta/rules.lock" "$TMP/lock.bak"
check "guard: no lock, an npm call is allowed silently on stdout" '[ -z "$(pre "$(bash_p "\"npm i\"")")" ]'
mv "$TMP/lock.bak" "$VAULT/00-Meta/rules.lock"
printf '{"broken' > "$TMP/broken.json"
check "guard: a broken lock allows with one stderr line" 'cp "$VAULT/00-Meta/rules.lock" "$TMP/lock.bak" && cp "$TMP/broken.json" "$VAULT/00-Meta/rules.lock" && [ -z "$(pre "$(bash_p "\"npm i\"")")" ] && [ "$(printf "%s" "$(bash_p "\"npm i\"")" | k hook pre-tool --harness claude 2>&1 >/dev/null | wc -l)" -eq 1 ]'
cp "$TMP/lock.bak" "$VAULT/00-Meta/rules.lock"

if command -v bun >/dev/null 2>&1; then
  P="$TMP/npm.json"; bash_p '"npm i"' > "$P"
  ms=$(bun -e '
    const [bin, p] = process.argv.slice(1);
    const input = require("fs").readFileSync(p);
    const t = [];
    for (let i = 0; i < 21; i++) {
      const s = performance.now();
      Bun.spawnSync([bin, "hook", "pre-tool", "--harness", "claude"], { stdin: input });
      t.push(performance.now() - s);
    }
    t.sort((a, b) => a - b);
    console.log(t[10].toFixed(1));
  ' "$(native "$BIN")" "$(native "$P")")
  echo "      guard, matched deny, median of 21: ${ms} ms"
  check "guard: a matched call is at or under 30 ms (median of 21)" 'awk "BEGIN{exit !($ms <= 30)}"'
else
  skip "guard timing: bun not on PATH"
fi

# --- S5.4: the commit-time backstop --------------------------------------------------------------
if command -v gitleaks >/dev/null 2>&1; then
  APP="$TMP/app"
  mkdir -p "$APP"
  a() { git -C "$APP" -c user.name=check -c user.email=check@example.invalid "$@"; }
  a init -q -b main
  a config core.hooksPath "$(native "$ROOT/githooks")"
  echo plain > "$APP/a.txt"; a add a.txt
  check "pre-commit: a plain file commits" 'a commit -q -m plain >/dev/null 2>&1'
  echo '{}' > "$APP/package-lock.json"; a add package-lock.json
  check "pre-commit: a staged package-lock.json is refused by no-npm" '! a commit -q -m lock 2>&1 | tee "$TMP/c.out" >/dev/null; grep -q no-npm "$TMP/c.out" && [ -z "$(a log --oneline -- package-lock.json)" ]'
  a rm -q --cached package-lock.json
  g config core.hooksPath "$(native "$ROOT/githooks")"
  sub 's/Each push needs its own yes./Pushes are free./' "$RULES"
  git -C "$VAULT" add 00-Meta/Hard-Rules.md
  check "pre-commit: a weakened rule staged alone is refused" '! git -C "$VAULT" -c user.name=check -c user.email=check@example.invalid commit -q -m weak >/dev/null 2>&1'
  git -C "$VAULT" restore --staged --worktree 00-Meta/Hard-Rules.md
else
  skip "pre-commit backstops: gitleaks not on PATH"
fi

# --- S5.8: the proof: a wrapper passes the guard, the doctor's check fails, the restore holds ----
git -C "$VAULT" config --unset core.hooksPath 2>/dev/null
(cd "$VAULT" && node_or_bun=$(command -v bun || command -v node) && "$node_or_bun" -e "
  const f = '00-Meta/Hard-Rules.md', fs = require('fs');
  fs.writeFileSync(f, fs.readFileSync(f, 'utf8').replace('Each push needs its own yes.', 'Push freely.'));
")
check "proof: between the write and the next start, rules check FAILs" '! k rules check >/dev/null 2>&1'
out=$(k rules check --restore 2>&1)
check "proof: the next start's restore brings the rule back" 'printf "%s" "$out" | grep -q "restored from HEAD: ask-before-every-push" && grep -q "Each push needs its own yes." "$RULES"'

rm -rf "$TMP"
echo
[ "$fails" = 0 ] && echo "stage 5: all checks pass" || echo "stage 5: $fails failing"
exit "$fails"
