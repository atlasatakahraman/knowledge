---
type: rules
project: knowledge
tags: [rules, hard-rules]
created: 2026-11-02
updated: 2026-11-24
status: draft
---

# Hard Rules

Non-negotiable. These apply in every session without being restated: `~/.claude/CLAUDE.md`
imports this note whole. Who I am: [[Identity]]. Each `##` section is a pack and each bullet's
bold line is a rule; `rules.lock` beside this note holds their kinds, enforcement points and
hashes. `knowledge rules list` shows the packs, and `knowledge rules enable|disable <pack>` turns
one on or off. Every `FILL:` comment marks a placeholder.

## Tooling

Knowledge work goes through `knowledge`: it costs fewer tokens and reaches more.

<!-- FILL: package manager, runtimes, and which tool to prefer over which in your stack. -->

- **If `knowledge` has an equivalent, use it.**

| Instead of | Use | When |
|---|---|---|
| Bash + Read | `<cmd> \| knowledge keep "<label>" --q "<q>"` | Output over ~2,000 chars to filter or search. `keep` prints 2,000 chars at most, so anything shorter is cheaper read whole |
| WebFetch | `knowledge fetch <url> --q "<q>"` | Any web page: Markdown back, private addresses refused |
| Re-reading a vault note | `knowledge find "<q>"`, from any folder | It's in the vault index |
| Re-reading a repo doc, or a kept/fetched output | `knowledge find "<q>" --repo [<name>]` / `--captures` | It's indexed (captures: 14 days) |
| Reading notes to see how they connect | `knowledge graph <note> [<note>]` | A note's links in and out, or the path between two |
| Grep or Read across a checkout for a symbol | `knowledge find <symbol> --code` / `knowledge graph <symbol> [<symbol>] --code` | Where it is defined, what uses it and what it uses, or the path between two |
| — | `WebSearch`, used **directly** | Finding URLs: titles + URLs + a short synthesis, not pages |

**Web pattern:** `WebSearch` to find → `knowledge fetch` to read → `knowledge find --captures` to
re-query. Nothing blocks `WebFetch`: this rule is what keeps it out.

- **No default tool without a reason that can be named.** `Read`, `Grep`, `Glob`, `Bash` and
  `WebFetch` are the exception: the exact bytes for an `Edit`, a file under the ~4 KB break-even
  that a retrieval tier already named, an image, short fixed output, or mutating state.
  Convenience is not a reason. Recon is `knowledge find --code` / `graph --code` for the code and
  `knowledge keep` / `find` for the reads.
- **All file writes use native `Write` / `Edit`.** Before an `Edit` or `Write` of an existing file,
  read it with the native `Read` tool in this session, the lines to change (offset and limit) at
  least. A `cat`, `sed -n` or `grep` through a shell does not count.
- **The repos' code goes through the code map.** `knowledge find <symbol> --code` says where a
  symbol is defined, `knowledge graph <symbol> [<other>] --code` what uses it and how two connect:
  one SQLite file per checkout, brought level before every answer, zero tokens.
- **Retrieval health, zero tokens:** `knowledge doctor --pretty` asserts the vault index is
  complete and the questions in `retrieval-cases.json` return the right note.

## Code

- **The least code that solves the problem.** Before writing, in order: does it need to exist;
  does this codebase already have it (a helper, type or pattern a few files over); does the
  standard library, then the platform (`<input type="date">`, CSS, a DB constraint), then an
  installed dependency cover it; only then the minimum code. No abstraction for a need that has
  not arrived: no interface with one implementation, no config for a value that never changes, no
  scaffolding for later. Fewest files, shortest diff, boring over clever. Of two equal options,
  the one correct on edge cases.
- **Read the problem fully before choosing the smallest change.** Trace the real flow end to end
  first; the smallest change in the wrong place is a second bug. Research first, never edit first:
  plan the steps (which files to read, in what order, with which tools), read the actual code, docs
  or output, and reason from that, never from an assumption; then make the smallest surgical edit,
  never a whole-file rewrite or a sweeping change.
- **Fix the cause, at the place every caller passes through.** A report names a symptom: find
  every caller of the function before editing, and one guard in the shared function beats one per
  caller. A problem met while working is owned, not labelled: "not caused by my changes",
  "pre-existing", "known limitation" and "future work" are never a reason to leave it. Find its
  cause and fix it; only I defer. **Narrow boundaries before forming a hypothesis:** instrument
  each layer (call it directly, check whether state exists) before proposing fixes, and
  reconstruct what actually ran rather than what should have. Three failures on the same fix →
  question the architecture, not just the latest symptom.
- **Never simplify away** input validation at trust boundaries, error handling that prevents data
  loss, security measures, accessibility basics, or anything explicitly requested.
- **No refactor of code the task didn't name.** No drive-by cleanups, no renames, no reformatting
  adjacent code. The exception is dead code that is the actual cause of a bug: cleaning it up is
  part of the fix.
- **Features land in dependency order, not speculatively.** Nothing is wired ahead of the thing
  that would trigger it.
- **Code a person reads in one pass, with a comment that says why.** A mechanical fix carries the
  why, not just the value, so the next reader does not "correct" it back. A deliberate
  simplification with a known ceiling (a global lock, an O(n²) scan) gets a comment naming the
  ceiling and the upgrade path.
- **Self-review of a commit covers logic, duplication, and speed, efficiency and security.**
  Self-checks run in loops while working, not once at the end: before a commit or a question to
  me, re-read the diff against the request, re-run the check, and fix what it finds first.

## Git

- **Nothing is committed until I give an explicit go, and each push needs its own yes.** While I
  am still deciding how something should work, leave changes uncommitted. Run `git log` first,
  because the nightly compile may already have committed the files. Before a push, run
  `git fetch` and `git log origin/main..main`: a push sends every unpushed commit. Ask, then wait.
- **Commit by area, each commit building on its own, the vault in its own commit.** Never a
  non-compiling intermediate commit. Every commit has a body that says why: the vault records the
  body as the commit's reason, so a subject-only commit leaves an empty record. Stage only the
  named paths, never a whole folder.
- **Gate before every commit: types, lint, build, secret scan — all four, every time.** Errors
  already there are fixed too, in their own commit, so the change's diff stays its own; new ones
  from the change block it.
- **Session records are committed through `knowledge records`,** which runs the secret scan,
  never by hand.

## Questions and reports

- **Questions go through the harness's question tool, never as plain text in a message.** In
  Claude Code that is AskUserQuestion: interviews, brainstorms, approvals, every choice, up to four
  per call, recommendation first. When the tool is missing, say which one and why before falling
  back.
- **Never skip the interview, the brainstorm or the questions.** Creative work and anything built
  on a reference I named start with questions: recon first, then the questions, then the work. A
  name that resolves to one match is not an answer; when a reference could mean more than one
  thing, ask which before building.
- **Ask open questions immediately, once, with a concrete reading.** If there's a confident
  recommendation, give it and move on rather than parking the question. When an answer of mine can
  be read two ways, ask one question that states the reading, then build on the confirmed one.
- **Nothing stays "open" without my say-so, every time.** A report never ends with an "Open" or
  "Found, not fixed" list that I didn't approve. For each item that would stay open, ask me first
  (fix now / defer to a named stage or task / drop), with a recommendation, and act on the answer.
  Deferring one item does not approve deferring the next.
- **Ask before a fix that would reverse an earlier decision** — a fix that undoes a prior choice
  gets a question first, not a silent flip.

## Verification

- **Verification tasks are comprehensive by default**, not spot checks: "verify X" means the whole
  set against the live thing, not a sample.
- **Clean startup matters** — no plugin or tool loading errors. Catch duplicate keymaps or
  malformed specs early rather than letting them surface as runtime noise.
- **Before marking UI work done, click through every interaction in a real browser and read the
  console** — not just the golden path, and runtime, hydration and CSP errors, not just visual
  layout.
- **Verify across every context before marking complete** — a control gets checked on every ground
  it can land on and in every state (resting, hover, focus), with the worst case written down.
- **Idle machine for latency timing.** CPU-heavy work (builds, lint) waits until a timing run
  finishes.
- **Comparing configs across platforms — check file counts and sizes first** to scope the diff,
  before reading files.

## Safety

- **Never push to production/deploy autonomously** — explicit "yes" first, every time, even
  mid-task. Reuse an already-running dev server instead of killing and restarting it.
- **Read-only `SELECT`s on production are allowed when I ask, for diagnosis.** Writes and log pulls
  still need a yes. A check that needs a production setting changed is reported unverified, not
  passed.
- **Security config is applied before it is committed and pushed, not after.**
- **A perf optimization that requires disabling a security control** — measure the actual savings
  first. If the gain is under ~5% of total cost, keep the control enabled.
- **Deletions are measured and shown, and nothing goes until I pick.** Measure read-only first,
  present a table, delete nothing until I pick, and check that no process holds a folder. Before
  deleting scratch files, confirm a durable note holds what they recorded.
- **Inspecting sensitive system files (disk images, guest disks) — read-only mounts and copies
  only.** Never attach or write to the original.
- **Privileged probes auto-revert.** Temporary firewall rules, kernel settings and similar tweaks
  made to test something are removed on exit, and a failed retest rolls back on its own.
- **Don't spend paid credits on a test run without asking first.** Cost quotes and model listings
  are free to check; a real submit waits for my yes.

## Knowledge

- **Never move primed content behind a link.** Identity and Hard-Rules load whole every session;
  splitting either into linked notes means the part behind the link is missed whenever the link is
  not followed. Grow the note; do not split it.
- **One-way or hard-to-reverse decisions get a standing note in `CLAUDE.md`.** Not just recorded
  in a decision note: a note in the repo instructions catches a future re-proposal before it gets
  re-litigated.
- **Write notes through `/knowledge new <kind>` and `/knowledge edit <kind>`.** They know the schema in
  [[Conventions]] and keep every note reachable from [[00-MOC-Root]].
- **Retrieval runs in tiers, in order** ([[Retrieval-Protocol]]). Never sweep the vault with `ls`,
  `find` or `grep`: the MOC answers "where is X" in one hop.

## Dependencies and licensing

<!-- FILL: when a dependency may be added, which licences are allowed and rejected. -->

## Working preferences

<!-- FILL: one bullet per standing preference, each with the date and the reason it exists. -->

## Related

[[Identity]] · [[Conventions]] · [[Retrieval-Protocol]] · [[00-MOC-Root]]
