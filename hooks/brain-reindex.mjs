// brain-reindex.mjs - PostToolUse(Edit|Write), async. Trailing-debounce reindex of repo docs into
// context-mode. Runtime is bun (`bun --bun`), per 00-Meta/Hard-Rules.
// Spec: [[2026-08-22-brain-memory-compiler]] (vault plan). The vault half (its index, the write-time
// check, graphify) moved to the knowledge binary in vault audit Stage 3: `knowledge hook post-edit`.
import { readFileSync, writeFileSync, unlinkSync, existsSync, readdirSync } from "node:fs";
import { join, sep } from "node:path";
import { tmpdir } from "node:os";
import { spawn, execFileSync } from "node:child_process";
import { readStdin, normDir, HOOKS, DEV_ROOT, contextModeCli, INDEX_EXCLUDES } from "./brain-lib.mjs";

const STAMP = join(tmpdir(), "brain-reindex.json");
const DEBOUNCE_MS = 90_000;
const LOCK_STALE_MS = 15 * 60_000; // a settler that died leaves a lock; expire it
const EXCLUDES = INDEX_EXCLUDES; // shared with brain-doctor, so what is indexed and what counts as a note cannot drift
// context-mode is NOT on PATH - it ships as a plugin bundle. Invoke it through the runtime.
const CM_CLI = contextModeCli();
// Checkouts left the vault on 2026-09-06 - see [[0012-repos-leave-the-vault-index-stays]].
// DEV_ROOT comes from the machine's config (skeleton-config.mjs).

// Every child runs headless. Without this, each one opens a console window on Windows.
const QUIET = { stdio: "ignore", windowsHide: true };

const read = () => {
  try {
    return JSON.parse(readFileSync(STAMP, "utf8"));
  } catch {
    return null;
  }
};
const write = (o) => {
  try {
    writeFileSync(STAMP, JSON.stringify(o));
  } catch {}
};
const clear = () => {
  try {
    if (existsSync(STAMP)) unlinkSync(STAMP);
  } catch {}
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * Index one checkout's markdown — `docs/` (plans, DESIGN, DEVELOPMENT), `CLAUDE.md`, README —
 * under `repo:<Name>:docs`. The checkouts left the vault on 2026-09-06, and from then on nothing
 * re-indexed them: a decision recorded in `docs/plans` was invisible to `ctx_search` until someone
 * indexed it by hand.
 */
function indexRepoDocs(root) {
  const name = root.split(sep).pop();
  if (!CM_CLI) return;
  try {
    execFileSync(
      process.execPath,
      [
        "--bun", CM_CLI, "index", root,
        "--source", `repo:${name}:docs`,
        "--max-depth", "5",
        "--max-files", "300",
        "--ext", ".md",
        ...EXCLUDES.flatMap((e) => ["--exclude", `**/${e}/**`]),
      ],
      { cwd: root, timeout: 600_000, ...QUIET },
    );
  } catch {}
}

/** Record the repo and start a settler if none is running; repos accumulate across one window. */
function schedule(repo) {
  const prev = read();
  const lockLive = prev?.lock && Date.now() - prev.lock < LOCK_STALE_MS;
  // Always refresh `at` so a live settler extends its wait instead of firing early.
  write({ at: Date.now(), lock: lockLive ? prev.lock : Date.now(), repos: [...new Set([...(prev?.repos ?? []), repo])] });
  if (!lockLive)
    spawn(process.execPath, ["--bun", join(HOOKS, "brain-reindex.mjs"), "--settle"], { detached: true, ...QUIET }).unref();
}

/**
 * `<DEV_ROOT>/<Repo>/…` → `<DEV_ROOT>/<Repo>`, or null outside DEV_ROOT. On Windows `path`
 * arrives lower-cased by `normDir`, so the name is looked up on disk for its real casing:
 * `my-app` and `My-App` are one directory to Windows and two different projects to the
 * source label (Conventions § Renaming).
 */
function repoRoot(path) {
  const base = normDir(DEV_ROOT) + sep;
  if (!path.startsWith(base)) return null;
  const folded = path.slice(base.length).split(sep)[0];
  if (!folded) return null;
  try {
    const name = readdirSync(DEV_ROOT).find((n) => normDir(n) === folded);
    return name ? join(DEV_ROOT, name) : null;
  } catch {
    return null;
  }
}

if (process.argv.includes("--settle")) {
  // Trailing debounce: keep waiting while edits are still arriving, then run once.
  (async () => {
    try {
      for (;;) {
        const s = read();
        if (!s) break; // cleared by someone else; nothing to do
        const quiet = Date.now() - (s.at || 0);
        if (quiet < DEBOUNCE_MS) {
          await sleep(DEBOUNCE_MS - quiet);
          continue; // re-check: more edits may have landed while sleeping
        }
        for (const repo of s.repos ?? []) indexRepoDocs(repo);
        break;
      }
    } catch {}
    clear(); // release the lock so the next edit can schedule again
    process.exit(0);
  })();
} else {
  try {
    // `--repo <root>`: brain-commit's entry, for markdown committed by any means — an edit made
    // through a shell script never reaches the Edit|Write matcher.
    const flag = process.argv.indexOf("--repo");
    const path = normDir(flag !== -1 ? process.argv[flag + 1] || "" : readStdin().tool_input?.file_path || "");
    const root = path && repoRoot(path);
    if (root && (flag !== -1 || (path.endsWith(".md") && !EXCLUDES.some((e) => path.includes(`${sep}${e}${sep}`))))) schedule(root);
  } catch {}
  process.exit(0);
}
