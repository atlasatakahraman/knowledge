//! Promotes a finished daily log into the curated tiers, then commits only what it wrote
//! (brain-compile.mjs, ported in Stage 6; spec [[2026-08-22-brain-memory-compiler]]).
//!
//!   knowledge compile [--force] [--dry-run]
//!
//! The gate is "the log is finished", not a wall clock: a past logical day's log never changes
//! again, so it compiles once, and its SHA-256 in the state file proves it. `--force` takes today's
//! still-open log. One log per run, oldest first: a backlog drains over sessions instead of firing
//! N sonnet calls at once. Background work: every failure ends the run quietly.
use crate::config::Config;
use crate::state::{self, DECISIONS, J, KNOWLEDGE, META, OPS, SESSIONS};
use crate::{Args, Fault, emit, vault};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A compile that died mid-run leaves its claim; past this it is expired. Longer than the 900 s
/// sonnet timeout, so a live run is never stolen from.
const CLAIM_STALE_MS: i64 = 20 * 60_000;
const HEAD: &str = "## Unsorted (brain-compile)";

pub fn state_file(cfg: &Config) -> PathBuf {
    cfg.vault.join(OPS).join("brain-compile-state.json")
}

fn read_state(cfg: &Config) -> J {
    match state::read_json(&state_file(cfg)) {
        Some(j @ J::Obj(_)) => j,
        _ => J::Obj(Vec::new()),
    }
}

fn claimed(v: Option<&J>) -> bool {
    v.and_then(J::str)
        .and_then(|s| s.strip_prefix("running:"))
        .and_then(|ms| ms.parse::<f64>().ok())
        .is_some_and(|ms| (state::now_ms() as f64) - ms < CLAIM_STALE_MS as f64)
}

fn subdirs(p: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(p)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Every daily log, chronological (the path sorts as YYYY/MM/YYYY-MM-DD).
pub fn logs(cfg: &Config) -> Vec<PathBuf> {
    let root = cfg.vault.join(SESSIONS);
    let mut out = Vec::new();
    for y in subdirs(&root) {
        for m in subdirs(&root.join(&y)) {
            let dir = root.join(&y).join(&m);
            let mut files: Vec<String> = fs::read_dir(&dir)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            files.sort();
            out.extend(
                files
                    .into_iter()
                    .filter(|f| state::is_log_name(f))
                    .map(|f| dir.join(f)),
            );
        }
    }
    out
}

/// SHA-256 of the log read as UTF-8, hex: the value bun's `createHash("sha256")` stored, so the
/// 51 logs compiled before the port are not compiled again.
pub fn hash_of(f: &Path) -> String {
    let text = String::from_utf8_lossy(&fs::read(f).unwrap_or_default()).into_owned();
    ring::digest::digest(&ring::digest::SHA256, text.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn write_state(cfg: &Config, s: &J) {
    let _ = fs::write(state_file(cfg), state::pretty1(s));
}

/// `git status` of the scope, path -> two-letter status, in git's order. Taken before and after the
/// run, so the commit holds only what this compiler touched: a `git add <dir>` once swept the
/// owner's half-written ADRs into a compile commit (2026-09-06).
fn snapshot(cfg: &Config) -> Vec<(String, String)> {
    let mut args = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    args.extend([META, KNOWLEDGE, DECISIONS, SESSIONS, OPS]);
    let o = state::git(&cfg.vault, &args, Duration::from_secs(60));
    if !o.ok() {
        return Vec::new();
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for l in o.stdout.split('\n').filter(|l| !l.is_empty()) {
        // `R  old -> new` reports both sides; the new path is the one to stage.
        let rest = l.get(3..).unwrap_or("");
        let p = rest.rsplit(" -> ").next().unwrap_or(rest);
        let p = p.strip_prefix('"').unwrap_or(p);
        let p = p.strip_suffix('"').unwrap_or(p).to_string();
        let s = l.get(..2).unwrap_or("").to_string();
        match out.iter_mut().find(|(k, _)| *k == p) {
            Some(e) => e.1 = s,
            None => out.push((p, s)),
        }
    }
    out
}

fn status<'a>(snap: &'a [(String, String)], p: &str) -> Option<&'a str> {
    snap.iter().find(|(k, _)| k == p).map(|(_, s)| s.as_str())
}

/// Link every note this run created that its MOC does not link yet, under an "Unsorted" heading.
/// Some runs never file their notes (the 09-17 compile wrote five and linked none), and a MOC is
/// one hop from 00-MOC-Root, which keeps the note within `reach`. Returns the names it linked.
fn file_unlinked(cfg: &Config, created: &[String], day: &str) -> Vec<String> {
    let mut filed = Vec::new();
    for (prefix, moc) in [
        (
            format!("{DECISIONS}/"),
            format!("{DECISIONS}/00-MOC-Decisions.md"),
        ),
        (
            format!("{KNOWLEDGE}/"),
            format!("{KNOWLEDGE}/00-MOC-Knowledge.md"),
        ),
    ] {
        let file = cfg.vault.join(&moc);
        let Ok(mut text) = fs::read_to_string(&file) else {
            continue;
        };
        let linked: HashSet<String> = vault::links_of(&text)
            .into_iter()
            .map(|l| l.name.to_lowercase())
            .collect();
        let add: Vec<String> = created
            .iter()
            .filter(|p| p.starts_with(&prefix) && p.ends_with(".md"))
            .map(|p| p.rsplit('/').next().unwrap_or(p))
            .filter(|b| !b.starts_with("00-MOC-"))
            .map(|b| b.strip_suffix(".md").unwrap_or(b).to_string())
            .filter(|n| !linked.contains(&n.to_lowercase()))
            .collect();
        if add.is_empty() {
            continue;
        }
        let lines: Vec<String> = add
            .iter()
            .map(|n| {
                format!(
                    "- [[{n}]] — from [[{day}]], filed by brain-compile; move it to its section"
                )
            })
            .collect();
        let open = format!("{HEAD}\n\n");
        text = if text.contains(&open) {
            text.replacen(&open, &format!("{open}{}\n", lines.join("\n")), 1)
        } else {
            format!("{}\n\n{open}{}\n", text.trim_end(), lines.join("\n"))
        };
        if fs::write(&file, text).is_err() {
            continue;
        }
        filed.extend(add);
    }
    filed
}

fn prompt(cfg: &Config, log: &str, body: &str, dry: bool) -> String {
    let v = cfg.vault.display();
    let dev = state::native(&cfg.dev.join("<remote-name>"));
    [
        format!("Compile a session log into the vault at {v}."),
        String::new(),
        format!("MANDATORY FIRST STEP, before writing anything: read {META}/00-MOC-Root.md and"),
        format!("{META}/Conventions.md, then LIST the full contents of the {DECISIONS}/ folder you will write to and every"),
        format!("{KNOWLEDGE}/<Area>/ you might write to. You must know every existing filename and the"),
        "highest ADR number already on disk before you create a single file. Do not skip this.".into(),
        String::new(),
        "Then route each item:".into(),
        format!("- decisions   -> {DECISIONS}/Repos/<Repo-Name>/<NNNN>-<slug>.md about one repo; {DECISIONS}/Identity/<NNNN>-<slug>.md about the vault, its hooks, or how the agent must behave. <Repo-Name> is the GitHub remote's exact casing."),
        format!("- gotchas     -> {KNOWLEDGE}/<Area>/<slug>.md"),
        format!("- patterns    -> {KNOWLEDGE}/Connections/<slug>.md"),
        format!("- preferences -> append to {META}/Hard-Rules.md or {META}/Identity.md"),
        String::new(),
        "A decision the user reversed, and a mistake either of you fixed, are BOTH decisions -".into(),
        "record what was chosen, what was rejected, and why. Do not drop the reversal.".into(),
        String::new(),
        "Rules:".into(),
        "- ADRs are CENTRAL. Never write into a checkout's own Decisions/ folder - the checkouts live".into(),
        format!("  outside the vault at {dev}, so anything written there is invisible to the"),
        format!("  vault's history, its search index and its graph. Write to {DECISIONS}/Repos/<Repo-Name>/ or {DECISIONS}/Identity/."),
        "- ONE decision per ADR file. Never bundle several decisions into one omnibus note - if the".into(),
        "  log holds four decisions, that is four files, not one titled 'X, Y and Z'.".into(),
        "- If a decision or gotcha in this log ALREADY has a file, EDIT THAT FILE. Do not write a".into(),
        "  second file for it under a different slug. Two notes describing one decision is the worst".into(),
        "  outcome available here - worse than no note, because both then rot separately. This log may".into(),
        "  have been compiled before: assume it was, and check before you create.".into(),
        "- ADR numbers: (highest number present in that repo folder) + 1, incrementing from there.".into(),
        "  Never reuse a number already on disk, even when the file holding it looks unrelated to you.".into(),
        "- Every note gets frontmatter per Conventions.md. Filenames kebab-case, H1 is `NNNN — Title`.".into(),
        "- Link back to the source log with a [[wikilink]], never a path link.".into(),
        format!("- Add EVERY new decision to the table in {DECISIONS}/00-MOC-Decisions.md, and EVERY new"),
        format!("  {KNOWLEDGE} note to {KNOWLEDGE}/00-MOC-Knowledge.md under its area's section, as"),
        "  `- [[<slug>]] — <one line>`. A note missing from its MOC is unreachable at retrieval tier 1".into(),
        "  and will be rewritten by a later run.".into(),
        "- Touch nothing outside the four target areas.".into(),
        if dry { "- DRY RUN: describe what you would write. Write NOTHING.".into() } else { String::new() },
        String::new(),
        format!("## The log ({log})"),
        body.to_string(),
    ]
    .join("\n")
}

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &[])?;
    let cfg = crate::config::load()?;
    // Kill switch for structural work on the vault: a compile firing mid-restructure would stage a
    // half-moved tree under its own author.
    if state::state_dir().join("compile.disabled").exists() {
        return Ok(0);
    }
    compile(&cfg, args.has("force"), args.has("dry-run"));
    Ok(0)
}

fn compile(cfg: &Config, force: bool, dry: bool) {
    let today = state::note_path(cfg, state::now_ms());
    let st = read_state(cfg);
    let log = if force {
        today.exists().then_some(today)
    } else {
        logs(cfg).into_iter().find(|f| {
            let v = st.get(&state::native(f));
            *f != today && !claimed(v) && v.and_then(J::str) != Some(hash_of(f).as_str())
        })
    };
    let Some(log) = log else { return };
    let key = state::native(&log);

    // Claim BEFORE the sonnet run, re-reading just before: on 2026-09-06 three SessionEnds raced
    // on one log and each wrote the same decisions at the same "next free" numbers.
    let mut fresh = read_state(cfg);
    if claimed(fresh.get(&key)) {
        return;
    }
    fresh.set(&key, J::s(format!("running:{}", state::now_ms())));
    write_state(cfg, &fresh);

    let body = String::from_utf8_lossy(&fs::read(&log).unwrap_or_default()).into_owned();
    let text = prompt(cfg, &key, &body, dry);
    let before = snapshot(cfg);

    let tools = if dry {
        "Read,Glob,Grep"
    } else {
        "Read,Glob,Grep,Write,Edit"
    };
    let mode = if dry { "plan" } else { "acceptEdits" };
    // The permission mode is explicit both ways: a spawned `claude -p` inherits settings.json's,
    // and the compiler must never silently degrade to emitting plans.
    let o = state::run(
        Path::new("claude"),
        &[
            "-p",
            "--model",
            "sonnet",
            "--permission-mode",
            mode,
            "--allowedTools",
            tools,
        ],
        Some(&cfg.vault),
        Some(&text),
        &[("CLAUDE_INVOKED_BY", "brain_compile")],
        Duration::from_secs(900),
    );
    // bun inherits stdout here: the dry run's description is the output.
    emit(&o.stdout);
    if !o.ok() {
        if !dry {
            let msg = if o.timed_out {
                "spawnSync claude ETIMEDOUT".to_string()
            } else if o.missing {
                "spawnSync claude ENOENT".to_string()
            } else {
                format!(
                    "Command failed: claude -p --model sonnet --permission-mode {mode} --allowedTools {tools}"
                )
            };
            // stdout went to the terminal in bun, so only stderr names the reason.
            let quiet = state::Out {
                stdout: String::new(),
                ..o
            };
            state::record_failure("compile", &state::claude_reason(&quiet, &msg));
        }
        return; // the claim stays until it goes stale; the next run retries
    }
    if dry {
        return;
    }
    state::clear_failure("compile");

    // Commit only what the compiler wrote, as its own author, so `git log` is a review queue. A
    // path the owner already had dirty keeps its status and is skipped, even when the compiler
    // also edited it: never fold someone else's work into this commit.
    let day = log
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let created: Vec<String> = snapshot(cfg)
        .into_iter()
        .filter(|(p, s)| s == "??" && status(&before, p).is_none())
        .map(|(p, _)| p)
        .collect();
    let filed = file_unlinked(cfg, &created, &day);
    let mine: Vec<String> = snapshot(cfg)
        .into_iter()
        .filter(|(p, s)| status(&before, p) != Some(s.as_str()))
        .map(|(p, _)| p)
        .collect();
    let git = |a: &[&str]| state::git(&cfg.vault, a, Duration::from_secs(60));
    let mut staged = String::new();
    if !mine.is_empty() {
        let mut add = vec!["add", "--"];
        add.extend(mine.iter().map(String::as_str));
        if !git(&add).ok() {
            return;
        }
        let d = git(&["diff", "--cached", "--name-only"]);
        if !d.ok() {
            return;
        }
        staged = d.out().to_string();
    }
    if !staged.is_empty() {
        // The commit note copies the body as the commit's "why" (spec D5 option A).
        let mut body = vec![
            format!(
                "Promoted the finished session log [[{day}]] into the vault: the decisions, gotchas and"
            ),
            "preferences it recorded, routed by brain-compile (sonnet).".into(),
            String::new(),
            "Files:".into(),
        ];
        body.extend(staged.split('\n').map(|f| format!("- {f}")));
        if !filed.is_empty() {
            body.extend([
                String::new(),
                format!(
                    "Linked by the fallback, not by the model (sort them): {}",
                    filed.join(", ")
                ),
            ]);
        }
        let subject = format!("chore(brain): compile {day}");
        let c = git(&[
            "-c",
            "user.name=brain-compiler",
            "-c",
            "user.email=noreply@local",
            "commit",
            "-m",
            &subject,
            "-m",
            &body.join("\n"),
        ]);
        if !c.ok() {
            return;
        }
    }
    // The compiler writes through a `claude -p` whose hooks exit at once, so no post-edit hook
    // reindexes its notes: index here, when something was written.
    if !mine.is_empty() {
        state::reindex();
    }
    // Hash AFTER the run (the compiler sometimes edits the log it read), into a re-read state:
    // our claim, and maybe another log's, landed after `st` was read.
    let mut fresh = read_state(cfg);
    fresh.set(&key, J::s(hash_of(&log)));
    write_state(cfg, &fresh);
}
