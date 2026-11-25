#!/usr/bin/env bash
# Stage 3's runnable check: the knowledge binary's verbs on a copy of template/, in a temp
# KNOWLEDGE_HOME. Builds nothing. Run from the repo root:
#   bash checks/stage-3.sh target/release/knowledge
# Exit 0 when every line says PASS (SKIP lines are platforms whose filesystem cannot hold a trap).
set -u
BIN=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
ROOT=$(cd "$(dirname "$0")/.." && pwd)
fails=0
pass() { echo "PASS  $1"; }
fail() { echo "FAIL  $1"; fails=$((fails + 1)); }
skip() { echo "SKIP  $1"; }
check() { if eval "$2"; then pass "$1"; else fail "$1"; fi; }
# The binary is native: on Windows it needs C:/... paths, not Git Bash's /tmp/...
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) WINDOWS=1 ;; *) WINDOWS=0 ;; esac

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
VAULT="$TMP/vault"
export KNOWLEDGE_HOME="$TMP/home"
unset KNOWLEDGE_CONFIG
mkdir -p "$KNOWLEDGE_HOME"
cp -R "$ROOT/template" "$VAULT"
printf '{"vault": "%s", "devRoot": "%s"}\n' "$(native "$VAULT")" "$(native "$TMP")" > "$KNOWLEDGE_HOME/config.json"
k() { "$BIN" "$@"; }

# --- S3.1: the binary, FTS5, the crates ---------------------------------------------------------
check "FTS5 is compiled in" '"$BIN" --version | grep -q "FTS5 on"'
if command -v cargo >/dev/null 2>&1; then
  extra=$(cd "$ROOT" && cargo tree -e normal --prefix none 2>/dev/null | sed 's/ .*//' | sort -u |
    grep -vxF -f <(grep -v '^#' "$ROOT/checks/crates.txt"))
  check "crates are exactly the allowlist${extra:+ (extra: $extra)}" '[ -z "$extra" ]'
else
  skip "crates: cargo not on PATH"
fi

# --- S3.3: index ---------------------------------------------------------------------------------
notes_on_disk=$(find "$VAULT" -name '*.md' -not -path '*/.obsidian/*' | wc -l | tr -d ' ')
k index --rebuild >/dev/null
check "a rebuild holds every note ($notes_on_disk)" '[ "$(k index --check --json | grep -o "\"rows\":[0-9]*" | cut -d: -f2)" = "$notes_on_disk" ]'
check "--check: 0 ghosts, 0 missing" 'k index --check --json | grep -q "\"ghosts\":\[\],\"missing\":\[\]"'
printf -- '---\ntype: doc\ncreated: 2026-11-22\n---\n# Zebra\nquokkas live here\n' > "$VAULT/90-Inbox/new-note.md"
k index >/dev/null
check "an added note is found" 'k find quokkas | grep -q new-note'
printf -- '---\ntype: doc\ncreated: 2026-11-22\n---\n# Zebra\nwombats live here now\n' > "$VAULT/90-Inbox/new-note.md"
touch -d '2 minutes' "$VAULT/90-Inbox/new-note.md" 2>/dev/null || true
k index >/dev/null
check "an edited note is reindexed" 'k find wombats | grep -q new-note && ! k find quokkas | grep -q new-note'
rm "$VAULT/90-Inbox/new-note.md"
k index >/dev/null
check "a deleted note leaves the index" '! k find wombats | grep -q new-note && k index --check >/dev/null'
# Two reindexes at once (a doctor's and a backfill's): the second waits for the write lock, never
# "database is locked" (a deferred transaction that reads first is refused past the busy timeout).
locked=0
for _ in 1 2 3 4 5 6 7 8 9 10; do
  find "$VAULT" -name '*.md' -not -path '*/.obsidian/*' -exec touch {} +
  k index > "$TMP/ix1.txt" 2>&1 & k index > "$TMP/ix2.txt" 2>&1; wait
  grep -q locked "$TMP/ix1.txt" "$TMP/ix2.txt" && locked=$((locked + 1))
done
check "two reindexes at once both finish ($locked of 10 rounds locked)" '[ "$locked" = 0 ]'

# --- S3.4 / S3.5: find, inject, hook prompt ------------------------------------------------------
check "find ranks the protocol note for its words" 'k find retrieval protocol tiers | head -1 | grep -q Retrieval-Protocol'
check "find --json is JSON" 'k find retrieval protocol tiers --json | grep -q "^\[{\"heading\""'
check "inject names vault notes" 'k inject "how do the retrieval tiers work" | grep -q "Retrieval-Protocol"'
hook_out=$(printf '{"prompt":"how do the retrieval tiers work"}' | k hook prompt --harness claude)
check "hook prompt prints additionalContext" '[[ "$hook_out" == *"\"hookEventName\":\"UserPromptSubmit\""* ]]'
check "hook prompt: bad JSON, exit 0, no output" '[ -z "$(printf "not json" | k hook prompt --harness claude)" ]'
check "hook prompt: unknown harness, no output" '[ -z "$(printf "{\"prompt\":\"retrieval\"}" | k hook prompt --harness other)" ]'
check "hook prompt: no index, exit 0, no output" '[ -z "$(printf "{\"prompt\":\"retrieval\"}" | KNOWLEDGE_HOME="$TMP/none" KNOWLEDGE_CONFIG="$KNOWLEDGE_HOME/config.json" k hook prompt --harness claude)" ]'

# --- S3.6: graph ---------------------------------------------------------------------------------
check "graph lists a MOC's neighbours" 'k graph 00-MOC-Root | grep -q "^links to ([1-9]"'
check "graph finds a path between two notes" 'k graph Conventions Hard-Rules | grep -q "→"'

# --- S3.8: hook post-edit ------------------------------------------------------------------------
edit() { printf '{"tool_name":"Write","tool_input":{"file_path":"%s"}}' "$(native "$VAULT/$1")" | k hook post-edit --harness claude; }
printf -- '# no frontmatter\nsee [[nowhere-note]] and [[Conventions#No Such Heading]]\n' > "$VAULT/90-Inbox/bad.md"
bad_out=$(edit 90-Inbox/bad.md)
check "post-edit reports missing frontmatter" '[[ "$bad_out" == *"frontmatter needs"* ]]'
check "post-edit reports a dead link and a dead heading" '[[ "$bad_out" == *"[[nowhere-note]], [[Conventions#No Such Heading]]"* ]]'
check "post-edit indexed the note" 'k find nowhere | grep -q "^bad "'
printf -- '---\ntype: doc\ncreated: 2026-11-22\n---\nsee [[Conventions]]\n' > "$VAULT/90-Inbox/good.md"
check "post-edit is silent on a good note" '[ -z "$(edit 90-Inbox/good.md)" ]'
check "post-edit is silent outside the vault" '[ -z "$(printf "{\"tool_input\":{\"file_path\":\"%s\"}}" "$(native "$TMP/elsewhere.md")" | k hook post-edit --harness claude)" ]'
rm "$VAULT/90-Inbox/bad.md" "$VAULT/90-Inbox/good.md"
k index >/dev/null

# --- S3.2: portable ------------------------------------------------------------------------------
check "portable passes on the template" 'k doctor --only portable >/dev/null'
trap_fails() { # $1 label, then the command that plants the trap, then the one that removes it
  eval "$2"
  if k doctor --only portable >/dev/null; then fail "portable catches: $1"; else pass "portable catches: $1"; fi
  eval "$3"
}
trap_fails "a machine path in frontmatter" \
  'printf -- "---\ntype: doc\ncreated: 2026-11-22\nsource: C:/Users/me/x.md\n---\n" > "$VAULT/90-Inbox/p.md"' 'rm "$VAULT/90-Inbox/p.md"'
trap_fails "a backslash in a link" \
  'printf -- "---\ntype: doc\ncreated: 2026-11-22\n---\n[[00-Meta\\\\Conventions]]\n" > "$VAULT/90-Inbox/p.md"' 'rm "$VAULT/90-Inbox/p.md"'
trap_fails "a name not in NFC" "touch \"\$VAULT/90-Inbox/\$(printf 'cafe\\xcc\\x81').md\"" "rm \"\$VAULT/90-Inbox/\$(printf 'cafe\\xcc\\x81').md\""
trap_fails "a path over 240 chars" 'mkdir -p "$VAULT/90-Inbox/$(printf "%0120d" 0)" && touch "$VAULT/90-Inbox/$(printf "%0120d" 0)/$(printf "%0120d" 1).md"' 'rm -r "$VAULT/90-Inbox/$(printf "%0120d" 0)"'
trap_fails ".gitattributes without LF" 'mv "$VAULT/.gitattributes" "$TMP/ga"' 'mv "$TMP/ga" "$VAULT/.gitattributes"'
if [ "$WINDOWS" = 0 ]; then
  # Probe the filesystem, not the OS: macOS's default APFS is case-insensitive, so the second
  # touch lands on the first file and there is no pair to catch.
  touch "$VAULT/90-Inbox/CaseProbe"
  if [ -e "$VAULT/90-Inbox/caseprobe" ]; then
    skip "names differing only by case: this filesystem is case-insensitive (Linux CI runs it)"
  else
    trap_fails "names differing only by case" 'touch "$VAULT/90-Inbox/Case.md" "$VAULT/90-Inbox/case.md"' 'rm "$VAULT/90-Inbox/Case.md" "$VAULT/90-Inbox/case.md"'
  fi
  rm "$VAULT/90-Inbox/CaseProbe"
  trap_fails "a reserved name" 'touch "$VAULT/90-Inbox/aux.md"' 'rm "$VAULT/90-Inbox/aux.md"'
  trap_fails "a forbidden character" 'touch "$VAULT/90-Inbox/a?b.md"' 'rm "$VAULT/90-Inbox/a?b.md"'
  trap_fails "a trailing dot" 'touch "$VAULT/90-Inbox/note."' 'rm "$VAULT/90-Inbox/note."'
else
  skip "case pairs, reserved names, forbidden characters, trailing dots: NTFS cannot hold them (Linux CI runs them)"
fi

echo
[ "$fails" = 0 ] && echo "stage-3: PASS" || echo "stage-3: FAIL ($fails)"
[ "$fails" = 0 ]
