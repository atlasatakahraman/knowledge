// What an agent reads to reach an answer: `knowledge find` (and `--code`) against grep + Read.
// The fixture is this repository itself, so the number is public and anyone can rerun it: its
// `template/` and `docs/` (plus README.md and AGENTS.md) as the vault, a clone of its HEAD as the
// checkout, both in a temp folder with their own KNOWLEDGE_HOME and HOME. Method: docs/benchmark.md.
// Run from the repo root:  bun --bun bench/tokens.ts target/release/knowledge
import { cpSync, mkdtempSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative, resolve } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";

const repo = resolve(import.meta.dir, "..");
const bin = resolve(process.argv[2] ?? "target/release/knowledge");
const { notes, code } = JSON.parse(readFileSync(join(import.meta.dir, "questions.json"), "utf8"));
const RUNS = 5; // latency: the median of five runs per question

// --- the fixture -------------------------------------------------------------------------------
const tmp = mkdtempSync(join(tmpdir(), "kb-"));
const vault = join(tmp, "vault"), dev = join(tmp, "dev"), home = join(tmp, "home"), user = join(tmp, "user");
const checkout = join(dev, "knowledge");
cpSync(join(repo, "template"), vault, { recursive: true });
const docs = join(vault, "20-Knowledge", "knowledge");
mkdirSync(docs, { recursive: true });
for (const f of readdirSync(join(repo, "docs")).filter((f) => f.endsWith(".md"))) cpSync(join(repo, "docs", f), join(docs, f));
cpSync(join(repo, "README.md"), join(docs, "Overview.md")); // `README` is the template's own note
cpSync(join(repo, "AGENTS.md"), join(docs, "Agent-Install.md"));
mkdirSync(dev); mkdirSync(home); mkdirSync(user);
const git = spawnSync("git", ["clone", "-q", "--no-hardlinks", repo, checkout]);
if (git.status !== 0) throw new Error(`git clone: ${git.stderr}`);
writeFileSync(join(home, "config.json"), JSON.stringify({ vault, devRoot: dev }));

// The binary sees only the fixture: its own KNOWLEDGE_HOME, and a HOME with nothing in it.
const env: Record<string, string> = { ...process.env, KNOWLEDGE_HOME: home, HOME: user, USERPROFILE: user } as any;
delete env.KNOWLEDGE_CONFIG;
const k = (args: string[], cwd = tmp) => {
  const t = performance.now();
  const r = spawnSync(bin, args, { env, cwd, encoding: "utf8" });
  return { out: (r.stdout ?? "") + (r.stderr ?? ""), ms: performance.now() - t };
};
k(["index", "--rebuild"]);
k(["code", "index", "knowledge"]);

// --- what a tool call costs -------------------------------------------------------------------
// Characters the agent reads. Tokens are estimated at 4 characters each; the percentages below are
// ratios of characters, so they do not depend on that estimate.
/** The Read tool's output for a file, or for some of its lines: `cat -n` (a 6-wide number, a tab). */
const read = (file: string, from = 1, to = Infinity) =>
  readFileSync(file, "utf8").split("\n").slice(from - 1, Math.min(to, 2000))
    .reduce((n, line, i) => n + `${String(from + i).padStart(6)}\t${line.slice(0, 2000)}\n`.length, 0);
const median = (xs: number[]) => xs.sort((a, b) => a - b)[xs.length >> 1];

const files = (dir: string): string[] => readdirSync(dir).flatMap((f) => {
  const p = join(dir, f);
  return f === ".git" ? [] : statSync(p).isDirectory() ? files(p) : [p];
});
const vaultFiles = files(vault).map((p) => ({ p, text: readFileSync(p, "utf8").toLowerCase() }));
const STOP = new Set("the and for with how what does from into after before when where which why who are its how not one per off out own".split(" "));
const words = (q: string) => [...new Set(q.toLowerCase().split(/[^a-z0-9.-]+/).filter((w) => w.length >= 3 && !STOP.has(w)))];
const count = (text: string, w: string) => text.split(w).length - 1;

/** Grep (files_with_matches: absolute paths) for `terms`, then Read whole files in the order given
 * until the target is read. Returns the characters, or undefined when the target never matched. */
function grepThenRead(terms: string[], target: string, rank: (f: { p: string; text: string }) => number[]) {
  const hits = vaultFiles.filter((f) => terms.some((t) => f.text.includes(t)));
  hits.sort((a, b) => { const x = rank(a), y = rank(b); for (let i = 0; i < x.length; i++) if (x[i] !== y[i]) return y[i] - x[i]; return a.p.localeCompare(b.p); });
  let chars = hits.reduce((n, f) => n + f.p.length + 1, 0), reads = 0;
  for (const f of hits) { chars += read(f.p); reads++; if (f.p === target) return { chars, reads }; }
  return undefined;
}

// --- notes ------------------------------------------------------------------------------------
const rows: string[] = [];
let kNotes = 0, gNotes = 0, bNotes = 0, found = 0;
const noteMs: number[] = [];
for (const { q, note } of notes) {
  const target = vaultFiles.find((f) => basename(f.p, ".md") === note)?.p;
  if (!target) throw new Error(`no note ${note} in the fixture`);
  const terms = words(q);
  // A realistic grep: every word of the question, files ranked by how many of the words they hold.
  const alt = grepThenRead(terms, target, (f) => [terms.filter((t) => f.text.includes(t)).length, terms.reduce((n, t) => n + count(f.text, t), 0)]);
  // The best a grep can do: the single word of the question that reaches the note for the least.
  const best = Math.min(...terms.map((t) => grepThenRead([t], target, (f) => [count(f.text, t)])?.chars ?? Infinity));
  const runs = Array.from({ length: RUNS }, () => k(["find", q]));
  noteMs.push(median(runs.map((r) => r.ms)));
  const names = runs[0].out.split("\n").filter((l) => / — /.test(l) && !l.startsWith(" ")).map((l) => l.split(" — ")[0]);
  const rank = names.indexOf(note) + 1;
  // A miss falls back to the realistic grep, so it costs find's output on top of grep's.
  const kChars = runs[0].out.length + (rank ? read(target) : alt?.chars ?? 0);
  if (rank) found++;
  // A note no grep reaches costs grep nothing here: the comparison only ever errs in grep's favour.
  kNotes += kChars; gNotes += alt?.chars ?? 0; bNotes += best === Infinity ? alt?.chars ?? 0 : best;
  rows.push(`note  ${String(rank || "-").padStart(2)}  ${String(kChars).padStart(7)} ${String(alt?.chars ?? "miss").padStart(7)} ${String(best).padStart(7)}  ${q}`);
}

// --- code -------------------------------------------------------------------------------------
let kCode = 0, gCode = 0, defs = 0;
const codeMs: number[] = [], pairMs: number[] = [];
const SRC = ["*.rs", "*.ts", "*.mjs", "*.js", "*.sh"];
for (const { symbol, def } of code) {
  const runs = Array.from({ length: RUNS }, () => {
    const f = k(["find", symbol, "--code", "--repo", "knowledge"]);
    const g = k(["graph", symbol, "--code", "--repo", "knowledge"]);
    return { out: f.out + g.out, found: f.out, ms: f.ms, both: f.ms + g.ms };
  });
  codeMs.push(median(runs.map((r) => r.ms)));
  pairMs.push(median(runs.map((r) => r.both)));
  const ok = runs[0].found.replaceAll("\\", "/").includes(def);
  if (ok) defs++;
  // grep -n -w over the source, then Read the 40 lines above each use and 5 below (where the
  // enclosing function starts), merged per file. The definition's own line needs no Read.
  const gr = spawnSync("git", ["grep", "-n", "-w", symbol, "--", ...SRC], { cwd: checkout, encoding: "utf8" }).stdout;
  let gChars = gr.split("\n").filter(Boolean).reduce((n, l) => n + checkout.length + 1 + l.length + 1, 0);
  const windows = new Map<string, [number, number][]>();
  for (const l of gr.split("\n").filter(Boolean)) {
    const [path, line, ...rest] = l.split(":");
    if (path === def && new RegExp(`fn\\s+${symbol}\\b`).test(rest.join(":"))) continue;
    const n = Number(line), w = windows.get(path) ?? windows.set(path, []).get(path)!;
    const last = w.at(-1);
    if (last && n - 40 <= last[1]) last[1] = n + 5; else w.push([Math.max(1, n - 40), n + 5]);
  }
  for (const [path, w] of windows) for (const [a, b] of w) gChars += read(join(checkout, path), a, b);
  kCode += runs[0].out.length; gCode += gChars;
  rows.push(`code  ${ok ? " ✓" : " -"}  ${String(runs[0].out.length).padStart(7)} ${String(gChars).padStart(7)}          ${symbol}`);
}

rmSync(tmp, { recursive: true, force: true });

// --- report -----------------------------------------------------------------------------------
const pct = (a: number, b: number) => `${Math.round((1 - a / b) * 100)}%`;
const tok = (c: number) => `~${Math.round(c / 4).toLocaleString("en")}`;
const mean = (xs: number[]) => (xs.reduce((a, b) => a + b, 0) / xs.length).toFixed(1);
console.log("kind rank knowledge    grep  best-1  question  (characters read)");
console.log(rows.join("\n"));
console.log(`
notes: ${found}/${notes.length} found · knowledge ${tok(kNotes)} tokens · grep ${tok(gNotes)} (${pct(kNotes, gNotes)} fewer) · best single-word grep ${tok(bNotes)} (${pct(kNotes, bNotes)} fewer) · find ~${mean(noteMs)} ms mean
code:  ${defs}/${code.length} defined · knowledge ${tok(kCode)} tokens · grep + Read ${tok(gCode)} (${pct(kCode, gCode)} fewer) · find --code ~${mean(codeMs)} ms, with graph ~${mean(pairMs)} ms mean
all:   knowledge ${tok(kNotes + kCode)} · grep ${tok(gNotes + gCode)} · ${pct(kNotes + kCode, gNotes + gCode)} fewer tokens`);
