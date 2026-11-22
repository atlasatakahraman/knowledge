#!/usr/bin/env bash
# Stage 4's runnable check: keep, find --captures, the repo-docs index and fetch's refusals, on a
# copy of template/ in a temp KNOWLEDGE_HOME. Builds nothing. Run from the repo root:
#   bash checks/stage-4.sh target/release/knowledge
# Needs python3 (or python) for the local listener that proves a refusal opened no connection.
# The two checks that need the internet SKIP when it cannot be reached; a refusal never does.
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
DEV="$TMP/dev"
export KNOWLEDGE_HOME="$TMP/home"
unset KNOWLEDGE_CONFIG
mkdir -p "$KNOWLEDGE_HOME" "$DEV"
cp -R "$ROOT/template" "$VAULT"
printf '{"vault": "%s", "devRoot": "%s"}\n' "$(native "$VAULT")" "$(native "$DEV")" > "$KNOWLEDGE_HOME/config.json"
k() { "$BIN" "$@"; }
k index >/dev/null

PY=""
for p in python3 python; do
  if "$p" -c 'import sys; sys.exit(sys.version_info < (3, 8))' >/dev/null 2>&1; then PY=$p; break; fi
done

# --- S4.1: the crates ---------------------------------------------------------------------------
if command -v cargo >/dev/null 2>&1; then
  check "no webpki-roots on any CI target" '! (cd "$ROOT" && for t in x86_64-pc-windows-msvc aarch64-pc-windows-msvc x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do cargo tree -e normal --target $t --prefix none 2>/dev/null; done) | grep -q "^webpki-root"'
else
  skip "crates: cargo not on PATH"
fi

# --- S4.2: keep ---------------------------------------------------------------------------------
FIX="$TMP/keep.txt"
{
  echo "FIRSTLINE sentinel of the input"
  for s in Alpha Beta Gamma Delta Epsilon Zeta Eta Theta; do
    echo "# $s"
    for i in $(seq 1 12); do echo "$s line $i holds ordinary words about section $s and nothing more."; done
    [ "$s" = Gamma ] && echo "The zebra and the giraffe live only in Gamma."
    echo
  done
} > "$FIX"
out=$(k keep fixture --q "zebra" --q "giraffe zebra" < "$FIX")
check "keep: two queries on one section print it once" '[ "$(printf "%s\n" "$out" | grep -c "^## Gamma")" = 1 ]'
check "keep: the input's first line is not echoed" '! printf "%s" "$out" | grep -q FIRSTLINE'
check "keep: within the default budget (2000)" '[ "$(printf "%s" "$out" | wc -c)" -le 2000 ]'
check "keep: the overflow line counts the other 8 sections" 'printf "%s" "$out" | grep -q "^8 sections not shown"'
check "keep: the other titles are listed" 'printf "%s" "$out" | grep -q "^Other: Alpha · Beta"'
small=$(k keep tiny --budget 900 < "$FIX")
check "keep: --budget holds" '[ "$(printf "%s" "$small" | wc -c)" -le 900 ]'
check "keep: a small input with no query is printed as it is" '[ "$(printf "short\n" | k keep short)" = short ]'

# --- S4.3: find --captures, the repo-docs index -------------------------------------------------
check "find --captures finds a kept section" 'k find giraffe --captures | grep -q "^keep:fixture § Gamma"'
check "find --captures: within its budget" '[ "$(k find line --captures --budget 600 | wc -c)" -le 600 ]'
mkdir -p "$DEV/Repo-One/docs/deep" "$DEV/Repo-One/node_modules/x" "$DEV/Repo-Two"
printf '# Okapi\nThe okapi notes live in docs.\n' > "$DEV/Repo-One/docs/deep/okapi.md"
printf '# Hidden\nokapi in node_modules is never indexed\n' > "$DEV/Repo-One/node_modules/x/README.md"
printf '# Repo One\nThe readme mentions the pangolin.\n' > "$DEV/Repo-One/README.md"
printf '# Two\nAnother okapi, in the second repo.\n' > "$DEV/Repo-Two/CLAUDE.md"
k index --repo "$(native "$DEV/Repo-One")" >/dev/null
k index --repo "$(native "$DEV/Repo-Two")" >/dev/null
check "find --repo finds a planted repo doc" 'k find okapi --repo | grep -q "okapi.md"'
check "find --repo <name> keeps to that repo" 'k find okapi --repo Repo-Two | grep -q "Repo-Two" && ! k find okapi --repo Repo-Two | grep -q "Repo-One"'
check "index --repo skips node_modules" '! k find okapi --repo Repo-One | grep -q node_modules'
hook_out=$(printf '{"prompt":"where does the okapi pangolin live"}' | k hook prompt --harness claude)
check "a repo doc never reaches hook prompt" '! printf "%s" "$hook_out" | grep -qi "okapi\|pangolin"'
check "find (vault) never returns a repo doc" '! k find okapi pangolin | grep -q Repo-One'
printf '# Okapi\nNow the okapi met a tapir.\n' > "$DEV/Repo-One/docs/deep/okapi.md"
printf '{"tool_input":{"file_path":"%s"}}' "$(native "$DEV/Repo-One/docs/deep/okapi.md")" | k hook post-edit --harness claude
check "post-edit reindexes a repo doc" 'k find tapir --repo | grep -q okapi.md'
rm "$DEV/Repo-One/docs/deep/okapi.md"
k index --repo "$(native "$DEV/Repo-One")" >/dev/null
check "index --repo removes a deleted doc" '! k find tapir --repo | grep -q okapi.md && k find pangolin --repo | grep -q README'
check "find (vault) still works" 'k find retrieval protocol tiers | head -1 | grep -q Retrieval-Protocol'

if [ -n "$PY" ]; then
  "$PY" - "$KNOWLEDGE_HOME/captures.db" <<'EOF'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
db.execute("UPDATE captures SET created = created - 15 * 24 * 3600 WHERE label = 'keep:tiny'")
db.commit()
EOF
  printf 'fresh\n' | k keep fresh >/dev/null
  check "a capture past 14 days is gone after the next write" '! k find section --captures --limit 24 | grep -q "^keep:tiny" && k find section --captures --limit 24 | grep -q "^keep:fixture"'
else
  skip "capture expiry: no python"
fi

# --- S4.4: fetch --------------------------------------------------------------------------------
if [ -n "$PY" ]; then
  LOG="$TMP/listener.log"
  : > "$LOG"
  "$PY" - "$TMP/port" "$LOG" <<'EOF' &
import socket, sys
s = socket.socket()
s.bind(("127.0.0.1", 0))
s.listen(8)
open(sys.argv[1], "w").write(str(s.getsockname()[1]))
while True:
    c, _ = s.accept()
    open(sys.argv[2], "a").write("connection\n")
    c.recv(4096)
    c.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 12\r\nConnection: close\r\n\r\nlistener-ok\n")
    c.close()
EOF
  LPID=$!
  for _ in $(seq 1 50); do [ -s "$TMP/port" ] && break; sleep 0.1; done
  PORT=$(cat "$TMP/port" 2>/dev/null)
  refuses() { # $1 label, $2 URL: refused, and the listener saw nothing
    local out
    out=$(k fetch "$2" 2>&1)
    check "fetch refuses $1" '[[ "$out" == *refused* ]] && [ ! -s "$LOG" ]'
  }
  refuses "127.0.0.1" "http://127.0.0.1:$PORT/"
  refuses "[::1]" "http://[::1]:$PORT/"
  refuses "10.0.0.1" "http://10.0.0.1:$PORT/"
  refuses "169.254.169.254" "http://169.254.169.254/latest/meta-data/"
  refuses "[fe80::1]" "http://[fe80::1]:$PORT/"
  refuses "100.64.0.1" "http://100.64.0.1:$PORT/"
  refuses "[::ffff:127.0.0.1]" "http://[::ffff:127.0.0.1]:$PORT/"
  refuses "localhost" "http://localhost:$PORT/"
  check "fetch refuses a non-http scheme" 'k fetch "file:///etc/passwd" 2>&1 | grep -q "not an http or https URL"'
  # A public redirector that sends us to the listener: the second hop is checked again.
  redir=$(k fetch "https://postman-echo.com/redirect-to?url=http://127.0.0.1:$PORT/" 2>&1)
  if [[ "$redir" == *refused* ]]; then
    check "fetch refuses a redirect to 127.0.0.1" '[ ! -s "$LOG" ]'
  else
    skip "redirect to 127.0.0.1: postman-echo.com unreachable ($(printf "%s" "$redir" | head -1 | cut -c1-80))"
  fi
  check "--allow-private reaches the listener" '[ "$(k fetch "http://127.0.0.1:$PORT/" --allow-private)" = listener-ok ] && [ -s "$LOG" ]'
  kill "$LPID" 2>/dev/null
else
  skip "fetch refusals: no python for the listener"
fi
page=$(k fetch "https://doc.rust-lang.org/std/net/struct.Ipv4Addr.html" --q "is_private" 2>&1)
if [[ "$page" == fetch:* ]]; then
  check "fetch: a real docs page keeps its headings" 'printf "%s" "$page" | grep -q "^## pub const fn is\\\\_private"'
  check "fetch: links keep their text, not their URLs" '! printf "%s" "$page" | grep -q "](http"'
  check "fetch: within the default budget" '[ "$(printf "%s" "$page" | wc -c)" -le 2000 ]'
else
  skip "real page: doc.rust-lang.org unreachable ($(printf "%s" "$page" | head -1 | cut -c1-80))"
fi

rm -rf "$TMP"
echo
if [ "$fails" = 0 ]; then echo "stage-4: PASS"; else echo "stage-4: $fails FAILED"; fi
exit $((fails > 0))
