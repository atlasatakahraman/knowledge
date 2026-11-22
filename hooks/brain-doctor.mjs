// brain-doctor.mjs - zero-token vault health check. JSON by default; exit 0 always.
// Plan: [[vault-governance-06-daily-sweep]] (vault plan).
//
//   bun --bun ~/.claude/hooks/brain-doctor.mjs            JSON, for the Routine
//   ... --pretty                                          table, for a human
//   ... --repair                                          idempotent repairs, then check
//   ... --record                                          write today's line to 50-Ops/Health/
//   ... --only frontmatter,links,reach                    run just these checks (a template copy, an installer step)
//
// Every failure of this system so far was SILENT - the retrieval hook never returned a vault note
// for as long as it existed, and nothing noticed. Each check below exists because of one.
// A `fail` check makes the report FAIL; a `warn` check is reported and does not.
// --repair runs only what is safe to run twice and cannot lose data (spec D9): commit notes, hubs,
// `knowledge index`, the repo-graph merge. It never edits, moves or deletes a note.
import { readdirSync, readFileSync, existsSync, writeFileSync, mkdirSync, realpathSync, statSync } from "node:fs";
import { join, basename, dirname, isAbsolute } from "node:path";
import { homedir } from "node:os";
import { execFileSync } from "node:child_process";
import {
  VAULT, DEV_ROOT, HOOKS, HOOK_EVENTS, FOLDERS, CONFIG,
  failures, logicalDate, notePath,
  repoList, recentCommits, recordedCommits, unrecorded,
  rebuildGraph, skeletonPin, PIN_FILE, isRecordPath,
  NOTE_EXEMPT as EXEMPT, walkVault as walk, linksOf, frontmatterOk, forwardLinks, brokenLinksIn,
  knowledgeBin, knowledge,
} from "./brain-lib.mjs";

const pretty = process.argv.includes("--pretty");
const repair = process.argv.includes("--repair");
const record = process.argv.includes("--record");
const onlyAt = process.argv.indexOf("--only");
const only = onlyAt === -1 ? null : new Set((process.argv[onlyAt + 1] || "").split(",").filter(Boolean));
const GRACE_MS = 10 * 60_000; // compile and a pull index detached; a note written a minute ago is fine

/**
 * Ground truth, owned by the vault: [{ "query": "...", "note": "<basename>" }]. Keyed by basename,
 * so moves cannot break it; a failing case means retrieval broke. The template ships cases against
 * its own notes; a vault replaces them with questions it must always answer.
 */
const CASES_FILE = join(VAULT, FOLDERS.meta, "retrieval-cases.json");

// ---- the vault on disk (walk, links, frontmatter: in brain-lib) ----------------------------------

const rel = (p) => p.slice(VAULT.length + 1);
const isRootFile = (p) => !rel(p).includes("\\") && !rel(p).includes("/");

// ---- checks -------------------------------------------------------------------------------------

/**
 * The binary's index (D11) against the disk: a row for every note, no ghost, none stale. A note
 * written in the last GRACE_MS by a detached writer (compile, a pull) is pending, not a failure.
 */
function checkIndex() {
  if (!knowledgeBin()) return { id: "vault-index", level: "fail", ok: false, detail: { reason: "knowledge binary not installed: run install.ts" } };
  const r = JSON.parse(knowledge(["index", "--check", "--json"], { ok: [0, 1] }));
  const fresh = (p) => {
    try {
      return Date.now() - statSync(join(VAULT, p)).mtimeMs < GRACE_MS;
    } catch {
      return false;
    }
  };
  const missing = r.missing.filter((p) => !fresh(p));
  const stale = r.stale.filter((p) => !fresh(p));
  return {
    id: "vault-index", level: "fail",
    ok: !missing.length && !stale.length && !r.ghosts.length,
    detail: {
      notes: r.notes, rows: r.rows, ghosts: r.ghosts.length, missing: missing.length, stale: stale.length,
      pending: r.missing.length + r.stale.length - missing.length - stale.length,
      samples: [
        ...missing.slice(0, 5).map((p) => `missing: ${p}`),
        ...stale.slice(0, 5).map((p) => `stale: ${p}`),
        ...r.ghosts.slice(0, 5).map((p) => `ghost: ${p}`),
      ],
    },
  };
}

/** Each case's note is among the six the prompt hook would inject, asked through the binary. */
function checkRetrieval() {
  let cases;
  try {
    cases = JSON.parse(readFileSync(CASES_FILE, "utf8"));
  } catch (e) {
    return { id: "retrieval", level: "fail", ok: false, detail: { reason: `${rel(CASES_FILE)}: ${e.code === "ENOENT" ? "missing" : e.message}` } };
  }
  if (!Array.isArray(cases) || !cases.length || !cases.every((c) => c?.query && c?.note))
    return { id: "retrieval", level: "fail", ok: false, detail: { reason: `${rel(CASES_FILE)}: want a non-empty array of {"query", "note"}` } };
  if (!knowledgeBin()) return { id: "retrieval", level: "fail", ok: false, detail: { reason: "knowledge binary not installed: run install.ts" } };
  const failed = [];
  for (const { query: q, note: want } of cases) {
    const got = JSON.parse(knowledge(["find", q, "--json", "--limit", "6"])).map((h) => h.name);
    if (!got.includes(want)) failed.push({ q, want, got: got.slice(0, 4) });
  }
  return { id: "retrieval", level: "fail", ok: !failed.length, detail: { passed: cases.length - failed.length, total: cases.length, failed } };
}

/**
 * Yesterday's daily note exists - or every session that captured yesterday was flushed and judged
 * non-durable. A flush writes a note only on durable output, so a quiet day has captures and no
 * note by design; 2026-10-12/13 failed this check while every flush was a genuine FLUSH_OK.
 */
function checkSessions() {
  const file = notePath(new Date(Date.now() - 86_400_000));
  const day = basename(file, ".md");
  if (existsSync(file)) return { id: "sessions", level: "fail", ok: true, detail: { day, note: true } };
  // Without the capture log this check could not tell a broken pipeline from a day off.
  const captured = new Map(); // sid -> last capture ts that day
  const settled = new Map(); // sid -> last flush ts that judged its material
  try {
    for (const l of readFileSync(HOOK_EVENTS, "utf8").split("\n")) {
      try {
        const e = JSON.parse(l);
        if (e.hook === "capture" && basename(notePath(new Date(e.ts)), ".md") === day) captured.set(e.sid, e.ts);
        // Any day: a session's tail is often flushed by SessionEnd after the rollover.
        if (e.hook === "flush" && ["flush-ok", "nothing-new", "wrote"].includes(e.outcome)) settled.set(e.sid, e.ts);
      } catch {}
    }
  } catch {}
  const unsettled = [...captured].filter(([sid, ts]) => !(settled.get(sid) >= ts)).map(([sid]) => sid.slice(0, 8));
  const reading = !captured.size ? "no session that day"
    : unsettled.length ? "sessions ran and no note was written" : "sessions ran, every flush judged nothing durable";
  return { id: "sessions", level: "fail", ok: !unsettled.length, detail: { day, note: false, sessions: captured.size, unsettled, reading } };
}

function checkAuth() {
  const f = failures();
  return { id: "auth", level: "fail", ok: !Object.keys(f).length, detail: Object.fromEntries(Object.entries(f).map(([k, v]) => [k, v.reason])) };
}

/**
 * Portability (D16): the traps that break the vault on the other boot, checked by the binary
 * (`knowledge doctor --only portable`). Before the binary is installed this is a WARN, not a FAIL.
 */
function checkPortable() {
  if (!knowledgeBin()) return { id: "portable", level: "warn", ok: false, detail: { reason: "knowledge binary not installed" } };
  try {
    const r = JSON.parse(knowledge(["doctor", "--only", "portable", "--json"], { ok: [0, 1] }));
    return { id: "portable", level: "fail", ok: r.ok, detail: { files: r.files, findings: r.findings } };
  } catch (e) {
    return { id: "portable", level: "fail", ok: false, detail: { reason: e.message } };
  }
}

/** Pull trust (0039): unset is a WARN, the rollout's last step. Set, every piece it needs must be there. */
function checkPullTrust() {
  if (CONFIG.pullTrust !== "signed") return { id: "pull-trust", level: "warn", ok: false, detail: { pullTrust: "unset: pulls are not signature-checked" } };
  const missing = ["allowed_signers", "signing_ed25519", "signing_ed25519.pub"].filter((f) => !existsSync(join(CONFIG.trustDir, f)));
  for (const repo of [VAULT, skeletonPin().root]) {
    try {
      if (execFileSync("git", ["-C", repo, "config", "commit.gpgsign"], { encoding: "utf8", windowsHide: true }).trim() !== "true") throw 0;
    } catch {
      missing.push(`commit.gpgsign in ${repo}`);
    }
  }
  return { id: "pull-trust", level: "fail", ok: !missing.length, detail: { pullTrust: "signed", missing } };
}

function checkFrontmatter({ notes }) {
  const bad = [];
  for (const { path } of notes.values()) {
    if (EXEMPT.test(basename(path))) continue;
    if (!frontmatterOk(readFileSync(path, "utf8"))) bad.push(rel(path));
  }
  return { id: "frontmatter", level: "fail", ok: !bad.length, detail: { bad: bad.length, samples: bad.slice(0, 8) } };
}

/** Every [[link]] resolves, except the intentional forward links listed in 00-Meta/Forward-Links.md. */
function checkLinks(vault) {
  const allowed = forwardLinks();
  const brokenIn = brokenLinksIn(vault, allowed);
  const broken = [];
  for (const { path } of vault.notes.values())
    for (const t of brokenIn(readFileSync(path, "utf8"))) broken.push(`${rel(path)} -> [[${t}]]`);
  return { id: "links", level: "fail", ok: !broken.length, detail: { broken: broken.length, allowedForward: allowed.size, samples: broken.slice(0, 8) } };
}

/** The vault's own promise: every note within 2 hops of 00-MOC-Root; records (daily, commit) within 3. */
function checkReach({ notes }) {
  const byName = new Map();
  for (const n of notes.values()) byName.set(basename(n.path, ".md").toLowerCase(), n.path);
  const adj = new Map();
  for (const n of notes.values()) adj.set(n.path, linksOf(readFileSync(n.path, "utf8")).map((t) => byName.get(t.toLowerCase())).filter(Boolean));
  const start = byName.get("00-moc-root");
  const depth = new Map([[start, 0]]);
  let frontier = [start];
  for (let d = 1; d <= 3; d++) {
    const next = [];
    for (const p of frontier) for (const q of adj.get(p) || []) if (!depth.has(q)) { depth.set(q, d); next.push(q); }
    frontier = next;
  }
  const RECORD = new RegExp(`[\\\\/](${FOLDERS.sessions}[\\\\/]|${FOLDERS.ops}[\\\\/]Commits[\\\\/]\\d{4}-\\d{2}[\\\\/])`);
  const isRecord = (p) => RECORD.test(p);
  const orphans = [];
  for (const { path } of notes.values()) {
    if (isRootFile(path) && EXEMPT.test(basename(path))) continue;
    const d = depth.get(path);
    if (d === undefined || d > (isRecord(path) ? 3 : 2)) orphans.push(rel(path));
  }
  return { id: "reach", level: "fail", ok: !orphans.length, detail: { orphans: orphans.length, samples: orphans.slice(0, 8) } };
}

/** Stale ignore paths broke Obsidian (33 GB indexable) and shrank the graph, both silently. */
function checkIgnores() {
  const stale = [];
  try {
    for (const raw of readFileSync(join(VAULT, ".graphifyignore"), "utf8").split("\n")) {
      const line = raw.trim();
      if (!line || line.startsWith("#")) continue;
      const fixed = line.replace(/^!/, "").split("*")[0].replace(/\/+$/, "");
      if (fixed && !existsSync(join(VAULT, fixed))) stale.push(`.graphifyignore: ${line}`);
    }
  } catch {}
  try {
    for (const p of JSON.parse(readFileSync(join(VAULT, ".obsidian", "app.json"), "utf8")).userIgnoreFilters || [])
      if (!existsSync(join(VAULT, String(p).replace(/\/+$/, "")))) stale.push(`userIgnoreFilters: ${p}`);
  } catch {}
  return { id: "ignores", level: "fail", ok: !stale.length, detail: { stale } };
}

/** Every commit of the last 3 days has its note (Stage 01). */
function checkCommits() {
  const { hashes } = recordedCommits();
  const missing = [];
  for (const c of unrecorded(repoList().flatMap((r) => recentCommits(r, "3.days.ago")), hashes)) {
    let files = [];
    try { files = execFileSync("git", ["show", "--name-only", "--format=", c.hash], { cwd: c.root, encoding: "utf8" }).split("\n").filter(Boolean); } catch {}
    if (c.root === VAULT && files.length && files.every(isRecordPath)) continue; // record-only commits are skipped by design
    missing.push(`${c.name} ${c.hash.slice(0, 7)} ${c.subject.slice(0, 60)}`);
  }
  return { id: "commits", level: "fail", ok: !missing.length, detail: { missing: missing.length, samples: missing.slice(0, 8) } };
}

/**
 * Identity and Hard-Rules reach every session whole: ~/.claude/CLAUDE.md imports both, every import
 * resolves, and the session hook stays under Claude Code's 10,000-char additionalContext limit (over
 * it, Claude gets a file path and a 2,000-char preview). The old in-hook cap dropped the rules
 * kernel instead, and nothing noticed (2026-10-09).
 */
function checkPriming() {
  const LIMIT = 10_000;
  const md = join(homedir(), ".claude", "CLAUDE.md");
  const norm = (p) => p.replace(/\\/g, "/").toLowerCase();
  const imports = [...readFileSync(md, "utf8").matchAll(/^@(\S+)\s*$/gm)]
    .map((m) => m[1].replace(/^~(?=[\\/])/, homedir()))
    .map((p) => (isAbsolute(p) ? p : join(dirname(md), p)));
  const missing = imports.filter((p) => !existsSync(p));
  const unimported = ["Identity.md", "Hard-Rules.md"]
    .map((f) => join(VAULT, FOLDERS.meta, f))
    .filter((r) => !imports.some((i) => norm(i) === norm(r)));
  let hookChars = -1;
  try {
    const out = execFileSync(process.execPath, ["--bun", join(HOOKS, "brain-session.mjs"), "--dry"], { input: "{}", encoding: "utf8", timeout: 30_000, windowsHide: true });
    hookChars = JSON.parse(out).hookSpecificOutput.additionalContext.length;
  } catch {}
  return {
    id: "priming", level: "fail",
    ok: !missing.length && !unimported.length && hookChars >= 0 && hookChars <= LIMIT,
    detail: { imports: imports.length, missing, unimported: unimported.map(rel), hookChars, limit: LIMIT },
  };
}

/**
 * The engine runs from a knowledge-skeleton checkout: ~/.claude/hooks, the /brain skill and every
 * brain-* command must resolve into the same one. A partial link (hooks repointed, commands left
 * behind) runs new hooks with old instructions, and nothing else would notice. Read-only, local:
 * no fetch. The version is the checkout's package.json; HEAD and dirty files say what is running.
 * A vault that pins a version in `.skeleton.json` fails on any other: it was written for that engine.
 */
function checkSkeletonVersion() {
  const hooks = realpathSync(HOOKS);
  const root = execFileSync("git", ["rev-parse", "--show-toplevel"], { cwd: hooks, encoding: "utf8", windowsHide: true }).trim();
  const git = (...a) => execFileSync("git", a, { cwd: root, encoding: "utf8", windowsHide: true }).split("\n").filter(Boolean);
  let pkg = {};
  try {
    pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
  } catch {}
  const claude = join(homedir(), ".claude");
  const inside = (link, target) => {
    try {
      return realpathSync(link) === realpathSync(target);
    } catch {
      return false;
    }
  };
  const unlinked = [];
  if (!inside(join(claude, "skills", "brain"), join(root, "skills", "brain"))) unlinked.push("skills/brain");
  for (const f of existsSync(join(root, "commands")) ? readdirSync(join(root, "commands")) : [])
    if (f.endsWith(".md") && !inside(join(claude, "commands", f), join(root, "commands", f))) unlinked.push(`commands/${f}`);
  const isSkeleton = pkg.name === "knowledge-skeleton";
  const { pin } = skeletonPin();
  const pinOk = !pin || pin === pkg.version;
  return {
    id: "skeleton-version", level: "fail", ok: isSkeleton && !unlinked.length && pinOk,
    detail: {
      root, version: pkg.version || null, pin, head: git("rev-parse", "--short", "HEAD")[0],
      dirty: git("status", "--porcelain").length, unlinked,
      ...(isSkeleton ? {} : { problem: `${HOOKS} does not resolve into a knowledge-skeleton checkout` }),
      ...(pinOk ? {} : { problem: `${PIN_FILE} pins ${pin}, the skeleton is ${pkg.version}: pull the skeleton, or adapt the vault and bump the pin` }),
    },
  };
}

/** Reported, not failed: normal mid-day, but worth seeing every day. */
function checkHygiene() {
  const out = {};
  try {
    // Machine-written paths are dirty by design until the next vault commit (D9: the Routine never
    // commits), so counting them would warn every single day. What matters is human work at risk.
    const MACHINE = new RegExp(`^.. (\\.obsidian/|${FOLDERS.ops}/|${FOLDERS.sessions}/|graphify-out/)`);
    out.uncommitted = execFileSync("git", ["status", "--porcelain"], { cwd: VAULT, encoding: "utf8" }).split("\n").filter((l) => l && !MACHINE.test(l)).length;
  } catch {}
  // D11: durable memory belongs in the vault. Non-empty per-cwd shards and stray files in checkouts are drift.
  const shards = [];
  try {
    const root = join(homedir(), ".claude", "projects");
    for (const d of readdirSync(root)) {
      const m = join(root, d, "memory");
      if (existsSync(m) && readdirSync(m).some((f) => f.endsWith(".md") && f !== "MEMORY.md")) shards.push(d);
    }
  } catch {}
  out.memoryShards = shards.length;
  // A file the checkout's own git tracks is that repo's documentation, which the vault never
  // duplicates - only untracked agent leftovers are strays (owner's decision, Stage 08).
  const tracked = (repo, f) => {
    try {
      execFileSync("git", ["ls-files", "--error-unmatch", f], { cwd: repo, stdio: "ignore" });
      return true;
    } catch {
      return false;
    }
  };
  const strays = [];
  try {
    for (const d of readdirSync(DEV_ROOT))
      for (const f of ["MEMORY.md", ".implementation_plan.md", ".remember"])
        if (existsSync(join(DEV_ROOT, d, f)) && !tracked(join(DEV_ROOT, d), f)) strays.push(`${d}/${f}`);
  } catch {}
  out.straysInCheckouts = strays;
  try {
    out.unpromotedDecisions = readFileSync(join(VAULT, FOLDERS.ops, "decisions.jsonl"), "utf8").split("\n").filter((l) => l.includes('"promoted":null')).length;
  } catch {
    out.unpromotedDecisions = 0;
  }
  const ok = !out.uncommitted && !shards.length && !strays.length && !out.unpromotedDecisions;
  return { id: "hygiene", level: "warn", ok, detail: out };
}

const CHECKS = {
  "vault-index": checkIndex, retrieval: checkRetrieval, sessions: checkSessions, auth: checkAuth,
  frontmatter: checkFrontmatter, links: checkLinks, reach: checkReach, ignores: checkIgnores,
  commits: checkCommits, priming: checkPriming,
  "skeleton-version": checkSkeletonVersion, hygiene: checkHygiene, "pull-trust": checkPullTrust,
  portable: checkPortable,
};
if (only) {
  const unknown = [...only].filter((id) => !(id in CHECKS));
  if (!only.size || unknown.length) {
    process.stderr.write(`--only wants check ids from: ${Object.keys(CHECKS).join(", ")}${unknown.length ? ` (unknown: ${unknown.join(", ")})` : ""}\n`);
    process.exit(1);
  }
  if (record) {
    process.stderr.write("--only and --record do not mix: a health line must cover every check\n");
    process.exit(1);
  }
}

function runChecks() {
  const vault = walk();
  const guard = ([id, fn]) => {
    try {
      return fn(vault);
    } catch (e) {
      return { id, level: "fail", ok: false, detail: { error: String(e?.message ?? e) } };
    }
  };
  return Object.entries(CHECKS).filter(([id]) => !only || only.has(id)).map(guard);
}

// ---- repair (derived artifacts only) -------------------------------------------------------------

function runRepairs() {
  const since = new Date(Date.now() - 3 * 864e5).toISOString().slice(0, 10); // checkCommits' window
  const steps = [
    // Commits made outside a Claude Bash call (terminal, PowerShell, other hooks) get no PostToolUse
    // note; the sweep records them here, before reindex so the new notes are indexed.
    ["commits", () => execFileSync(process.execPath, ["--bun", join(HOOKS, "brain-commit.mjs"), "--backfill", since], { timeout: 120_000, stdio: "ignore", windowsHide: true })],
    // Hubs read the filesystem only, and write MOC links (a new day's [[YYYY-MM-DD]]) that the index
    // must see, so the reindex runs after them.
    ["hubs", () => execFileSync(process.execPath, ["--bun", join(HOOKS, "brain-hubs.mjs")], { timeout: 60_000, stdio: "ignore", windowsHide: true })],
    ["reindex", () => knowledge(["index"])],
    ["graph", () => rebuildGraph()], // the repo graphs, merged into the served graph
  ];
  return steps.map(([step, fn]) => {
    const t = Date.now();
    try {
      fn();
      return { step, ok: true, ms: Date.now() - t };
    } catch (e) {
      return { step, ok: false, ms: Date.now() - t, error: String(e?.message ?? e).split("\n")[0] };
    }
  });
}

// ---- record --------------------------------------------------------------------------------------

function writeRecord(report) {
  const d = logicalDate(new Date());
  const pad = (n) => String(n).padStart(2, "0");
  const month = `${d.getFullYear()}-${pad(d.getMonth() + 1)}`;
  const day = `${month}-${pad(d.getDate())}`;
  const now = new Date();
  const failing = report.checks.filter((c) => c.level === "fail" && !c.ok).map((c) => c.id);
  const warning = report.checks.filter((c) => c.level === "warn" && !c.ok).map((c) => c.id);
  const notes = report.checks.find((c) => c.id === "vault-index")?.detail?.notes ?? "?";
  const line = `- ${day} ${pad(now.getHours())}:${pad(now.getMinutes())} **${report.ok ? "PASS" : "FAIL"}** · failing: ${failing.join(", ") || "none"} · warn: ${warning.join(", ") || "none"} · notes=${notes}`;
  const dir = join(VAULT, FOLDERS.ops, "Health");
  const file = join(dir, `health-${month}.md`);
  const created = !existsSync(file);
  mkdirSync(dir, { recursive: true });
  const head = ["---", "type: usage", "project: knowledge", "tags: [health, ops, generated]", `created: ${month}-01`, `updated: ${month}-01`, "status: active", "---", "", `# Vault health — ${month}`, "", "One line per logical day from `brain-doctor --record`. The 17:00 Routine may indent an explanation under a FAIL line.", ""].join("\n");
  let body = existsSync(file) ? readFileSync(file, "utf8") : head;
  // Replace today's line on a re-run, so a day has one line. The explanation indented under it is kept,
  // and a replaced FAIL line is kept as a bullet so that explanation still says what it explains.
  const lines = body.split("\n");
  const at = lines.findIndex((l) => l.startsWith(`- ${day} `));
  if (at !== -1) {
    const old = lines[at];
    lines.splice(at, 1, line, ...(old.includes("**FAIL**") ? [`  - replaced: ${old.slice(2)}`] : []));
    body = lines.join("\n");
  } else body = `${body.replace(/\n*$/, "\n")}${line}\n`;
  // `updated` stays fixed: this file merges by union across the boots, so bumping it per day would
  // leave duplicate frontmatter keys. The dated lines already say when it last changed.
  writeFileSync(file, body, "utf8");
  // This write lands AFTER the checks and repairs, and a script write fires no reindex hook. Without
  // re-deriving here, a month's first file is an orphan and every day's line goes `stale` 10 min later.
  if (created) try { execFileSync(process.execPath, ["--bun", join(HOOKS, "brain-hubs.mjs")], { timeout: 60_000, stdio: "ignore", windowsHide: true }); } catch {}
  // After the hubs, so a regenerated hub is indexed too.
  try { knowledge(["index"]); } catch {}
  return rel(file);
}

// ---- main ----------------------------------------------------------------------------------------

let before = null;
let repairs;
if (repair) {
  before = runChecks();
  repairs = runRepairs();
}
const checks = runChecks();
const report = {
  ok: checks.every((c) => c.level !== "fail" || c.ok),
  at: new Date().toISOString(),
  checks,
  ...(repair ? { repairs, repaired: before.filter((b) => !b.ok && checks.find((c) => c.id === b.id)?.ok).map((b) => b.id) } : {}),
};
if (record) report.recorded = writeRecord(report);

if (pretty) {
  const mark = (c) => (c.ok ? "PASS" : c.level === "warn" ? "WARN" : "FAIL");
  console.log(`brain-doctor  ${report.ok ? "PASS" : "FAIL"}  ${report.at}\n`);
  if (repairs) console.log(`  repairs: ${repairs.map((r) => `${r.step}=${r.ok ? "ok" : "FAILED"} ${r.ms}ms`).join("  ")}${report.repaired.length ? `  -> fixed: ${report.repaired.join(", ")}` : ""}\n`);
  for (const c of checks) {
    console.log(`  ${mark(c)}  ${c.id}`);
    for (const [k, v] of Object.entries(c.detail)) {
      if (Array.isArray(v)) for (const x of v) console.log(`          ${typeof x === "string" ? x : JSON.stringify(x)}`);
      else console.log(`          ${k}: ${typeof v === "object" ? JSON.stringify(v) : v}`);
    }
  }
  if (report.recorded) console.log(`\n  recorded: ${report.recorded}`);
} else process.stdout.write(JSON.stringify(report));
process.exit(0);
