//! The zero-token vault health check (brain-doctor.mjs, ported in Stage 6; plan
//! [[vault-governance-06-daily-sweep]]). JSON by default; exit 0 always.
//!
//!   knowledge doctor [--json]            the report, as JSON
//!   ... --pretty                         a table, for a human
//!   ... --repair                         idempotent repairs, then check
//!   ... --record                         today's line in 50-Ops/Health/
//!   ... --only frontmatter,links,reach   just these checks (not with --record)
//!
//! Every failure of this system so far was SILENT; each check exists because of one. A `fail` check
//! makes the report FAIL, a `warn` check is reported and does not. `--repair` runs only what is safe
//! to run twice and cannot lose data (spec D9): commit notes, hubs, the index.
//! It never edits, moves or deletes a note. The session start runs `--repair --record` detached,
//! once per logical day (spec item 28). The checks run on their own threads: most of their time is
//! child processes and file reads, and none depends on another.
use crate::config::Config;
use crate::state::{self, J, META, OPS, SESSIONS, obj};
use crate::{Fault, emit, index, linkcheck::LinkCheck, recorder, search, session, vault};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Compile and a pull index detached: a note written within this is pending, not a failure.
const GRACE_MS: i64 = 10 * 60_000;
/// Claude Code replaces a hook's additionalContext over this with a path and a 2,000-char preview.
const HOOK_LIMIT: i64 = 10_000;
/// What a vault session loads whole: the merge left 64,691 bytes (spec item 21), plus 15%.
const PRIMING_CEILING: i64 = 74_395;

pub const IDS: [&str; 18] = [
    "vault-index",
    "retrieval",
    "sessions",
    "auth",
    "frontmatter",
    "links",
    "reach",
    "ignores",
    "commits",
    "priming",
    "priming-size",
    "rules",
    "budget",
    "skeleton-version",
    "hygiene",
    "pull-trust",
    "codex-trust",
    "portable",
];

struct Check {
    id: &'static str,
    level: &'static str,
    ok: bool,
    detail: J,
}

impl Check {
    fn json(&self) -> J {
        obj([
            ("id", J::s(self.id)),
            ("level", J::s(self.level)),
            ("ok", J::Bool(self.ok)),
            ("detail", self.detail.clone()),
        ])
    }
}

fn fail(id: &'static str, ok: bool, detail: J) -> Check {
    Check {
        id,
        level: "fail",
        ok,
        detail,
    }
}

fn warn(id: &'static str, ok: bool, detail: J) -> Check {
    Check {
        id,
        level: "warn",
        ok,
        detail,
    }
}

fn strs(v: &[String]) -> J {
    J::Arr(v.iter().map(J::s).collect())
}

fn n(v: usize) -> J {
    J::n(v as i64)
}

/// A vault-relative path as bun printed it: the platform's separator.
fn rel(r: &str) -> String {
    state::native(Path::new(r))
}

fn first(v: &[String], k: usize) -> Vec<String> {
    v.iter().take(k).cloned().collect()
}

/// What every check reads: the walk, once.
struct Vault {
    files: Vec<vault::File>,
}

impl Vault {
    fn notes(&self) -> impl Iterator<Item = &vault::File> {
        self.files.iter().filter(|f| f.is_note())
    }
}

fn text_of(cfg: &Config, r: &str) -> String {
    String::from_utf8_lossy(&fs::read(cfg.vault.join(r)).unwrap_or_default()).into_owned()
}

fn base(r: &str) -> &str {
    r.rsplit('/').next().unwrap_or(r)
}

fn exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("knowledge"))
}

// ---- checks --------------------------------------------------------------------------------------

/// The index (D11) against the disk: a row for every note, no ghost, none stale.
fn check_index(cfg: &Config) -> Result<Check, Fault> {
    let r = index::check(&index::open_ro(cfg)?, cfg)?;
    let now = state::now_ms();
    let fresh =
        |p: &String| session::mtime_ms(&cfg.vault.join(p)).is_some_and(|t| now - t < GRACE_MS);
    let missing: Vec<String> = r.missing.iter().filter(|p| !fresh(p)).cloned().collect();
    let stale: Vec<String> = r.stale.iter().filter(|p| !fresh(p)).cloned().collect();
    let pending = r.missing.len() + r.stale.len() - missing.len() - stale.len();
    let mut samples: Vec<String> = missing
        .iter()
        .take(5)
        .map(|p| format!("missing: {p}"))
        .collect();
    samples.extend(stale.iter().take(5).map(|p| format!("stale: {p}")));
    samples.extend(r.ghosts.iter().take(5).map(|p| format!("ghost: {p}")));
    Ok(fail(
        "vault-index",
        missing.is_empty() && stale.is_empty() && r.ghosts.is_empty(),
        obj([
            ("notes", n(r.notes)),
            ("rows", n(r.rows)),
            ("ghosts", n(r.ghosts.len())),
            ("missing", n(missing.len())),
            ("stale", n(stale.len())),
            ("pending", n(pending)),
            ("samples", strs(&samples)),
        ]),
    ))
}

/// Each case's note is among the six the prompt hook would inject. Ground truth the vault owns,
/// keyed by basename so moves cannot break it: a failing case means retrieval broke.
fn check_retrieval(cfg: &Config) -> Result<Check, Fault> {
    let file = format!("{META}/retrieval-cases.json");
    let reason = |why: String| {
        Ok(fail(
            "retrieval",
            false,
            obj([("reason", J::s(format!("{}: {why}", rel(&file))))]),
        ))
    };
    let text = match fs::read_to_string(cfg.vault.join(&file)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return reason("missing".into()),
        Err(e) => return reason(e.to_string()),
    };
    let cases: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return reason(e.to_string()),
    };
    let truthy = |v: &serde_json::Value| v.as_str().is_some_and(|s| !s.is_empty());
    let list = cases
        .as_array()
        .filter(|a| !a.is_empty() && a.iter().all(|c| truthy(&c["query"]) && truthy(&c["note"])));
    let Some(list) = list else {
        return reason("want a non-empty array of {\"query\", \"note\"}".into());
    };
    let conn = index::open_ro(cfg)?;
    let t = search::Tuning {
        weights: search::TUNED.weights,
        floor: search::TUNED.floor,
        tokens: search::TUNED.tokens,
        pool: search::TUNED.pool,
    };
    let mut failed = Vec::new();
    for c in list {
        let (q, want) = (
            c["query"].as_str().unwrap_or(""),
            c["note"].as_str().unwrap_or(""),
        );
        let got: Vec<String> = search::search(&conn, q, 6, "", None, &t)?
            .into_iter()
            .map(|h| h.name)
            .collect();
        if !got.iter().any(|g| g == want) {
            failed.push(obj([
                ("q", J::s(q)),
                ("want", J::s(want)),
                ("got", strs(&first(&got, 4))),
            ]));
        }
    }
    Ok(fail(
        "retrieval",
        failed.is_empty(),
        obj([
            ("passed", n(list.len() - failed.len())),
            ("total", n(list.len())),
            ("failed", J::Arr(failed)),
        ]),
    ))
}

/// Yesterday's daily note exists, or every session that captured yesterday was flushed and judged
/// non-durable: a flush writes a note only on durable output, so a quiet day has captures and no
/// note by design (2026-10-12/13 failed while every flush was a genuine FLUSH_OK).
fn check_sessions(cfg: &Config) -> Result<Check, Fault> {
    let file = state::note_path(cfg, state::now_ms() - 86_400_000);
    let day = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if file.exists() {
        return Ok(fail(
            "sessions",
            true,
            obj([("day", J::s(&day)), ("note", J::Bool(true))]),
        ));
    }
    let mut captured: Vec<(String, String)> = Vec::new();
    let mut settled: HashMap<String, String> = HashMap::new();
    let events = fs::read_to_string(state::events_file(cfg)).unwrap_or_default();
    for l in events.split('\n') {
        let Ok(e) = serde_json::from_str::<serde_json::Value>(l) else {
            continue;
        };
        let (Some(sid), Some(ts)) = (e["sid"].as_str(), e["ts"].as_str()) else {
            continue;
        };
        if e["hook"] == "capture"
            && let Some(ms) = state::parse_ms(ts)
            && state::note_path(cfg, ms)
                .file_stem()
                .is_some_and(|s| s.to_string_lossy() == day)
        {
            match captured.iter_mut().find(|(k, _)| k == sid) {
                Some(slot) => slot.1 = ts.to_string(),
                None => captured.push((sid.to_string(), ts.to_string())),
            }
        }
        // Any day: a session's tail is often flushed by SessionEnd after the rollover.
        if e["hook"] == "flush"
            && ["flush-ok", "nothing-new", "wrote"].contains(&e["outcome"].as_str().unwrap_or(""))
        {
            settled.insert(sid.to_string(), ts.to_string());
        }
    }
    let unsettled: Vec<String> = captured
        .iter()
        .filter(|(sid, ts)| settled.get(sid).is_none_or(|s| s < ts))
        .map(|(sid, _)| sid.chars().take(8).collect())
        .collect();
    let reading = if captured.is_empty() {
        "no session that day"
    } else if !unsettled.is_empty() {
        "sessions ran and no note was written"
    } else {
        "sessions ran, every flush judged nothing durable"
    };
    Ok(fail(
        "sessions",
        unsettled.is_empty(),
        obj([
            ("day", J::s(day)),
            ("note", J::Bool(false)),
            ("sessions", n(captured.len())),
            ("unsettled", strs(&unsettled)),
            ("reading", J::s(reading)),
        ]),
    ))
}

fn check_auth() -> Check {
    let f = state::failures();
    let detail = J::Obj(
        f.iter()
            .map(|(k, v)| (k.clone(), v.get("reason").cloned().unwrap_or(J::Null)))
            .collect(),
    );
    fail("auth", f.is_empty(), detail)
}

/// The Codex plugin's hooks run only once the owner has reviewed them in the Codex CLI (C1), and
/// the ChatGPT app skips them silently until then. Codex itself says which are trusted (its
/// `hooks/list`, once a day in the sweep). WARNs when fewer are trusted than the plugin has, or
/// when Codex does not answer; passes when the plugin is not installed.
fn check_codex_trust() -> Check {
    match crate::setup::codex_trust() {
        None => warn(
            "codex-trust",
            true,
            J::s("the Codex plugin is not installed"),
        ),
        Some(None) => warn(
            "codex-trust",
            false,
            obj([
                ("trusted", J::s("unknown: Codex did not answer hooks/list")),
                (
                    "fix",
                    J::s("review the plugin's hooks in the Codex CLI (/hooks)"),
                ),
            ]),
        ),
        Some(Some((trusted, hooks))) => {
            let ok = hooks > 0 && trusted == hooks;
            let mut detail = obj([("trusted", n(trusted)), ("hooks", n(hooks))]);
            // The fix is printed only while there is something to fix.
            if !ok {
                detail.set(
                    "fix",
                    J::s("review the plugin's hooks in the Codex CLI (/hooks)"),
                );
            }
            warn("codex-trust", ok, detail)
        }
    }
}

/// Portability (D16): the traps that break the vault on the other boot.
fn check_portable(cfg: &Config) -> Result<Check, Fault> {
    let (files, found) = crate::doctor::portable(cfg)?;
    let findings = J::Arr(
        found
            .iter()
            .map(|(k, p)| obj([("kind", J::s(k)), ("path", J::s(p))]))
            .collect(),
    );
    Ok(fail(
        "portable",
        found.is_empty(),
        obj([("files", n(files)), ("findings", findings)]),
    ))
}

/// Pull trust (0039): unset is a WARN, the rollout's last step; set, every piece must be there.
fn check_pull_trust(cfg: &Config) -> Check {
    if !cfg.pull_trust {
        return warn(
            "pull-trust",
            false,
            obj([("pullTrust", J::s("unset: pulls are not signature-checked"))]),
        );
    }
    let mut missing: Vec<String> = ["allowed_signers", "signing_ed25519", "signing_ed25519.pub"]
        .iter()
        .filter(|f| !cfg.trust_dir.join(f).exists())
        .map(|f| f.to_string())
        .collect();
    for repo in [
        cfg.vault.display().to_string(),
        session::skeleton_root().display().to_string(),
    ] {
        let o = state::git(
            Path::new(&repo),
            &["config", "commit.gpgsign"],
            Duration::from_secs(10),
        );
        if !o.ok() || o.out() != "true" {
            missing.push(format!("commit.gpgsign in {repo}"));
        }
    }
    fail(
        "pull-trust",
        missing.is_empty(),
        obj([("pullTrust", J::s("signed")), ("missing", strs(&missing))]),
    )
}

/// brain-lib's frontmatterOk: a leading `---` block, CRLF or LF, with `type:` and `created:`.
fn frontmatter_ok(text: &str) -> bool {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return false;
    };
    let Some(k) = rest.find("\n---") else {
        return false;
    };
    let block = rest[..k].strip_suffix('\r').unwrap_or(&rest[..k]);
    vault::front_ok(block)
}

fn check_frontmatter(cfg: &Config, v: &Vault) -> Check {
    let bad: Vec<String> = v
        .notes()
        .filter(|f| !vault::exempt(base(&f.rel)) && !frontmatter_ok(&text_of(cfg, &f.rel)))
        .map(|f| rel(&f.rel))
        .collect();
    fail(
        "frontmatter",
        bad.is_empty(),
        obj([("bad", n(bad.len())), ("samples", strs(&first(&bad, 8)))]),
    )
}

/// Every [[link]] resolves, except the forward links listed in 00-Meta/Forward-Links.md.
fn check_links(cfg: &Config, v: &Vault) -> Check {
    let lc = LinkCheck::new(&cfg.vault, &v.files);
    let mut broken = Vec::new();
    for f in v.notes() {
        for t in lc.broken(&text_of(cfg, &f.rel)) {
            broken.push(format!("{} -> [[{t}]]", rel(&f.rel)));
        }
    }
    fail(
        "links",
        broken.is_empty(),
        obj([
            ("broken", n(broken.len())),
            ("allowedForward", n(lc.allowed())),
            ("samples", strs(&first(&broken, 8))),
        ]),
    )
}

/// A daily log or a commit note: allowed one hop further from the root than other notes.
fn is_record(r: &str) -> bool {
    let p = format!("/{r}");
    if p.contains(&format!("/{SESSIONS}/")) {
        return true;
    }
    let key = format!("/{OPS}/Commits/");
    p.match_indices(&key).any(|(i, _)| {
        let m = p.as_bytes().get(i + key.len()..i + key.len() + 8);
        m.is_some_and(|m| {
            m[..4].iter().all(u8::is_ascii_digit)
                && m[4] == b'-'
                && m[5..7].iter().all(u8::is_ascii_digit)
                && m[7] == b'/'
        })
    })
}

/// The vault's promise: every note within 2 hops of 00-MOC-Root; records within 3.
fn check_reach(cfg: &Config, v: &Vault) -> Check {
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let notes: Vec<&vault::File> = v.notes().collect();
    for (i, f) in notes.iter().enumerate() {
        by_name.insert(vault::name_of(&f.rel).to_lowercase(), i);
    }
    let adj: Vec<Vec<usize>> = notes
        .iter()
        .map(|f| {
            vault::links_of(&text_of(cfg, &f.rel))
                .iter()
                .filter_map(|l| by_name.get(&l.name.to_lowercase()).copied())
                .collect()
        })
        .collect();
    let mut depth: HashMap<usize, usize> = HashMap::new();
    if let Some(&start) = by_name.get("00-moc-root") {
        depth.insert(start, 0);
        let mut frontier = vec![start];
        for d in 1..=3 {
            let mut next = Vec::new();
            for p in frontier {
                for &q in &adj[p] {
                    if let std::collections::hash_map::Entry::Vacant(e) = depth.entry(q) {
                        e.insert(d);
                        next.push(q);
                    }
                }
            }
            frontier = next;
        }
    }
    let mut orphans = Vec::new();
    for (i, f) in notes.iter().enumerate() {
        if !f.rel.contains('/') && vault::exempt(&f.rel) {
            continue;
        }
        let max = if is_record(&f.rel) { 3 } else { 2 };
        if depth.get(&i).is_none_or(|&d| d > max) {
            orphans.push(rel(&f.rel));
        }
    }
    fail(
        "reach",
        orphans.is_empty(),
        obj([
            ("orphans", n(orphans.len())),
            ("samples", strs(&first(&orphans, 8))),
        ]),
    )
}

/// Stale ignore paths broke Obsidian (33 GB indexable), silently.
fn check_ignores(cfg: &Config) -> Check {
    let mut stale = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&state::read(
        &cfg.vault.join(".obsidian").join("app.json"),
    )) && let Some(filters) = v["userIgnoreFilters"].as_array()
    {
        for p in filters {
            let s = p.as_str().map_or_else(|| p.to_string(), str::to_string);
            if !cfg.vault.join(s.trim_end_matches('/')).exists() {
                stale.push(format!("userIgnoreFilters: {s}"));
            }
        }
    }
    fail("ignores", stale.is_empty(), obj([("stale", strs(&stale))]))
}

/// Every commit of the last 3 days has its note (Stage 01). Record-only vault commits are skipped
/// by design.
fn check_commits(cfg: &Config) -> Check {
    let hashes = recorder::recorded(cfg).hashes;
    let scan: Vec<recorder::Commit> = recorder::repo_list(cfg)
        .iter()
        .flat_map(|r| recorder::recent_commits(r, "3.days.ago"))
        .collect();
    let mut missing = Vec::new();
    for c in recorder::unrecorded(scan, &hashes) {
        let files = recorder::files_of(&c);
        if c.root == cfg.vault
            && !files.is_empty()
            && files.iter().all(|f| recorder::is_record_path(f))
        {
            continue;
        }
        missing.push(format!(
            "{} {} {}",
            c.name,
            &c.hash[..7.min(c.hash.len())],
            c.subject.chars().take(60).collect::<String>()
        ));
    }
    fail(
        "commits",
        missing.is_empty(),
        obj([
            ("missing", n(missing.len())),
            ("samples", strs(&first(&missing, 8))),
        ]),
    )
}

/// `~/.claude/CLAUDE.md` and the paths it imports, one `@path` per line, `~` expanded.
fn primed() -> Result<(PathBuf, Vec<String>), Fault> {
    let md = state::home().join(".claude").join("CLAUDE.md");
    let text = fs::read_to_string(&md).map_err(|e| Fault::io(&md, e))?;
    let home = state::native(&state::home());
    let dir = md.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut imports = Vec::new();
    for line in text.split('\n') {
        let Some(rest) = line.strip_prefix('@') else {
            continue;
        };
        let token = rest.split(char::is_whitespace).next().unwrap_or("");
        if token.is_empty() || !rest[token.len()..].trim().is_empty() {
            continue;
        }
        let p = match token.strip_prefix('~') {
            Some(t) if t.starts_with(['/', '\\']) => format!("{home}{t}"),
            _ => token.to_string(),
        };
        let abs = Path::new(&p).is_absolute() || (cfg!(windows) && p.starts_with(['/', '\\']));
        imports.push(if abs { p } else { state::native(&dir.join(&p)) });
    }
    Ok((md, imports))
}

/// Identity and Hard-Rules reach every session whole: imported, every import resolving, and the
/// session hook under the 10,000-char limit. The old in-hook cap dropped the rules kernel instead,
/// and nothing noticed (2026-10-09).
fn check_priming(cfg: &Config) -> Result<Check, Fault> {
    let (_, imports) = primed()?;
    let norm = |p: &str| p.replace('\\', "/").to_lowercase();
    let missing: Vec<String> = imports
        .iter()
        .filter(|p| !Path::new(p).exists())
        .cloned()
        .collect();
    let unimported: Vec<String> = ["Identity.md", "Hard-Rules.md"]
        .iter()
        .filter(|f| {
            let r = state::native(&cfg.vault.join(META).join(f));
            !imports.iter().any(|i| norm(i) == norm(&r))
        })
        .map(|f| rel(&format!("{META}/{f}")))
        .collect();
    let o = state::run(
        &exe(),
        &["hook", "session", "--harness", "claude", "--dry"],
        None,
        Some("{}"),
        &[],
        Duration::from_secs(30),
    );
    let hook_chars = serde_json::from_str::<serde_json::Value>(&o.stdout)
        .ok()
        .and_then(|v| {
            v["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .map(|s| s.encode_utf16().count() as i64)
        })
        .unwrap_or(-1);
    // One branch per instructions target that holds a block: it must be the one the vault
    // generates now. A machine with no block keeps the check's old shape.
    let blocks = crate::instructions::blocks(&cfg.vault);
    let stale: Vec<String> = blocks
        .iter()
        .filter(|(_, current)| !current)
        .map(|(p, _)| state::native(p))
        .collect();
    let mut detail = obj([
        ("imports", n(imports.len())),
        ("missing", strs(&missing)),
        ("unimported", strs(&unimported)),
        ("hookChars", J::n(hook_chars)),
        ("limit", J::n(HOOK_LIMIT)),
    ]);
    if !blocks.is_empty() {
        detail.set("blocks", n(blocks.len()));
        detail.set("staleBlocks", strs(&stale));
    }
    Ok(fail(
        "priming",
        missing.is_empty()
            && unimported.is_empty()
            && (0..=HOOK_LIMIT).contains(&hook_chars)
            && stale.is_empty(),
        detail,
    ))
}

/// What a vault session loads whole before the first prompt. A WARN: the owner decides what to condense.
fn check_priming_size(cfg: &Config) -> Result<Check, Fault> {
    let (md, imports) = primed()?;
    let mut files = vec![md];
    files.extend(imports.iter().map(PathBuf::from));
    files.push(cfg.vault.join("CLAUDE.md"));
    let sizes: Vec<i64> = files
        .iter()
        .filter_map(|f| fs::metadata(f).ok())
        .map(|m| m.len() as i64)
        .collect();
    let bytes: i64 = sizes.iter().sum();
    Ok(warn(
        "priming-size",
        bytes <= PRIMING_CEILING,
        obj([
            ("bytes", J::n(bytes)),
            ("ceiling", J::n(PRIMING_CEILING)),
            ("files", n(sizes.len())),
        ]),
    ))
}

/// Hard-Rules against 00-Meta/rules.lock. A missing, broken or disagreeing lock FAILs (K40).
fn check_rules() -> Check {
    let o = state::run(
        &exe(),
        &["rules", "check"],
        None,
        None,
        &[],
        Duration::from_secs(30),
    );
    let all = format!("{}{}", o.stdout, o.stderr);
    let out: Vec<String> = all.trim().split('\n').take(6).map(str::to_string).collect();
    fail(
        "rules",
        o.ok(),
        obj([
            ("exit", o.code.map_or(J::Null, |c| J::n(i64::from(c)))),
            ("out", strs(&out)),
        ]),
    )
}

/// The newest vault session's start cost by owner: WARNs on a listing over 1,000 tokens a session
/// with no call in 30 days (D10).
fn check_budget() -> Result<Check, Fault> {
    let o = state::run(
        &exe(),
        &["doctor", "budget", "--json"],
        None,
        None,
        &[],
        Duration::from_secs(60),
    );
    if !o.ok() {
        let code = o.code.map_or("null".into(), |c| c.to_string());
        return Err(Fault::Usage(format!(
            "knowledge doctor budget --json: exit {code}: {}",
            o.stderr.trim()
        )));
    }
    let b: J = serde_json::from_str(o.stdout.trim()).map_err(Fault::Json)?;
    let idle: Vec<String> = match b.get("idle") {
        Some(J::Arr(v)) => v
            .iter()
            .map(|i| {
                let name = i.get("name").and_then(J::str).unwrap_or("undefined");
                let tokens = i.get("tokens").map_or("undefined".into(), J::text);
                format!("{name}: {tokens} tokens, no call in 30 days")
            })
            .collect(),
        _ => Vec::new(),
    };
    let ok = matches!(b.get("ok"), Some(J::Bool(true)));
    Ok(warn(
        "budget",
        ok,
        obj([
            (
                "transcript",
                b.get("transcript").cloned().unwrap_or(J::Null),
            ),
            ("tokens", b.get("tokens").cloned().unwrap_or(J::Null)),
            ("idle", strs(&idle)),
        ]),
    ))
}

fn canonical(p: &Path) -> Option<PathBuf> {
    fs::canonicalize(p).ok()
}

/// The engine runs from one knowledge-skeleton checkout: ~/.claude/hooks, the /brain skill and every
/// brain-* command resolve into it. A partial link runs new hooks with old instructions. Read-only,
/// no fetch. A vault that pins a version in `.skeleton.json` fails on any other.
fn check_skeleton_version(cfg: &Config) -> Result<Check, Fault> {
    let hooks = state::hooks_dir();
    let real = canonical(&hooks)
        .ok_or_else(|| Fault::Usage(format!("{}: not found", state::native(&hooks))))?;
    let top = state::git(
        &real,
        &["rev-parse", "--show-toplevel"],
        Duration::from_secs(10),
    );
    if !top.ok() {
        return Err(Fault::Usage(format!(
            "Command failed: git rev-parse --show-toplevel\n{}",
            top.stderr.trim()
        )));
    }
    let root = top.out().to_string();
    let rootp = PathBuf::from(&root);
    let lines = |a: &[&str]| -> Vec<String> {
        state::git(&rootp, a, Duration::from_secs(10))
            .lines()
            .into_iter()
            .map(str::to_string)
            .collect()
    };
    let pkg: serde_json::Value = serde_json::from_str(&state::read(&rootp.join("package.json")))
        .unwrap_or(serde_json::Value::Null);
    let claude = state::home().join(".claude");
    let inside = |link: PathBuf, target: PathBuf| {
        canonical(&link).is_some_and(|l| canonical(&target).is_some_and(|t| l == t))
    };
    let mut unlinked = Vec::new();
    if !inside(
        claude.join("skills").join("brain"),
        rootp.join("skills").join("brain"),
    ) {
        unlinked.push("skills/brain".to_string());
    }
    let mut commands: Vec<String> = fs::read_dir(rootp.join("commands"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    commands.sort();
    for f in commands.iter().filter(|f| f.ends_with(".md")) {
        if !inside(
            claude.join("commands").join(f),
            rootp.join("commands").join(f),
        ) {
            unlinked.push(format!("commands/{f}"));
        }
    }
    let version = pkg["version"]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let is_skeleton = pkg["name"] == "knowledge-skeleton";
    let pin_file = cfg.vault.join(".skeleton.json");
    let pin = session::json_version(&pin_file);
    let pin_ok = pin.as_ref().is_none_or(|p| Some(p) == version.as_ref());
    let opt = |v: &Option<String>| v.as_ref().map_or(J::Null, J::s);
    let mut detail = obj([
        ("root", J::s(&root)),
        ("version", opt(&version)),
        ("pin", opt(&pin)),
        (
            "head",
            lines(&["rev-parse", "--short", "HEAD"])
                .first()
                .map_or(J::Null, J::s),
        ),
        ("dirty", n(lines(&["status", "--porcelain"]).len())),
        ("unlinked", strs(&unlinked)),
    ]);
    if !is_skeleton {
        detail.set(
            "problem",
            J::s(format!(
                "{} does not resolve into a knowledge-skeleton checkout",
                state::native(&hooks)
            )),
        );
    }
    if !pin_ok {
        detail.set(
            "problem",
            J::s(format!(
                "{} pins {}, the skeleton is {}: pull the skeleton, or adapt the vault and bump the pin",
                state::native(&pin_file),
                pin.clone().unwrap_or_default(),
                version.clone().unwrap_or_else(|| "undefined".into())
            )),
        );
    }
    Ok(fail(
        "skeleton-version",
        is_skeleton && unlinked.is_empty() && pin_ok,
        detail,
    ))
}

/// Reported, not failed: normal mid-day, but worth seeing every day.
fn check_hygiene(cfg: &Config) -> Check {
    let mut d = J::Obj(Vec::new());
    // Machine-written paths are dirty by design until the next vault commit; what matters is human
    // work at risk.
    let st = state::git(
        &cfg.vault,
        &["status", "--porcelain"],
        Duration::from_secs(30),
    );
    let mut uncommitted = 0;
    if st.ok() {
        let machine = [
            ".obsidian/".to_string(),
            format!("{OPS}/"),
            format!("{SESSIONS}/"),
        ];
        uncommitted = st
            .stdout
            .split('\n')
            .filter(|l| !l.is_empty())
            .filter(|l| {
                !(l.get(2..3) == Some(" ")
                    && machine
                        .iter()
                        .any(|m| l.get(3..).is_some_and(|p| p.starts_with(m.as_str()))))
            })
            .count();
        d.set("uncommitted", n(uncommitted));
    }
    // D11: durable memory belongs in the vault. Non-empty per-cwd shards are drift.
    let mut shards = 0;
    if let Ok(rd) = fs::read_dir(state::home().join(".claude").join("projects")) {
        for e in rd.flatten() {
            let m = e.path().join("memory");
            let has = fs::read_dir(&m).is_ok_and(|rd| {
                rd.flatten().any(|f| {
                    let n = f.file_name().to_string_lossy().into_owned();
                    n.ends_with(".md") && n != "MEMORY.md"
                })
            });
            if has {
                shards += 1;
            }
        }
    }
    d.set("memoryShards", n(shards));
    // A file the checkout's own git tracks is that repo's documentation; only untracked agent
    // leftovers are strays (owner's decision, Stage 08).
    let mut strays = Vec::new();
    let mut dirs: Vec<String> = fs::read_dir(&cfg.dev)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    for dname in &dirs {
        let repo = cfg.dev.join(dname);
        for f in ["MEMORY.md", ".implementation_plan.md", ".remember"] {
            if repo.join(f).exists()
                && !state::git(
                    &repo,
                    &["ls-files", "--error-unmatch", f],
                    Duration::from_secs(10),
                )
                .ok()
            {
                strays.push(format!("{dname}/{f}"));
            }
        }
    }
    d.set("straysInCheckouts", strs(&strays));
    let unpromoted = state::read(&state::ledger_file(cfg))
        .split('\n')
        .filter(|l| l.contains("\"promoted\":null"))
        .count();
    d.set("unpromotedDecisions", n(unpromoted));
    warn(
        "hygiene",
        uncommitted == 0 && shards == 0 && strays.is_empty() && unpromoted == 0,
        d,
    )
}

fn run_check(cfg: &Config, v: &Vault, id: &'static str) -> Check {
    let r = match id {
        "vault-index" => check_index(cfg),
        "retrieval" => check_retrieval(cfg),
        "sessions" => check_sessions(cfg),
        "auth" => Ok(check_auth()),
        "frontmatter" => Ok(check_frontmatter(cfg, v)),
        "links" => Ok(check_links(cfg, v)),
        "reach" => Ok(check_reach(cfg, v)),
        "ignores" => Ok(check_ignores(cfg)),
        "commits" => Ok(check_commits(cfg)),
        "priming" => check_priming(cfg),
        "priming-size" => check_priming_size(cfg),
        "rules" => Ok(check_rules()),
        "budget" => check_budget(),
        "skeleton-version" => check_skeleton_version(cfg),
        "hygiene" => Ok(check_hygiene(cfg)),
        "pull-trust" => Ok(check_pull_trust(cfg)),
        "codex-trust" => Ok(check_codex_trust()),
        _ => check_portable(cfg),
    };
    r.unwrap_or_else(|e| fail(id, false, obj([("error", J::s(e.to_string()))])))
}

/// Every selected check, each on its own thread, reported in the fixed order.
fn run_checks(cfg: &Config, only: &Option<HashSet<String>>) -> Vec<Check> {
    let v = Vault {
        files: vault::walk(&cfg.vault).unwrap_or_default(),
    };
    let ids: Vec<&'static str> = IDS
        .iter()
        .copied()
        .filter(|id| only.as_ref().is_none_or(|o| o.contains(*id)))
        .collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = ids
            .iter()
            .map(|&id| (id, s.spawn(|| run_check(cfg, &v, id))))
            .collect();
        handles
            .into_iter()
            .map(|(id, h)| {
                h.join().unwrap_or_else(|_| {
                    fail(id, false, obj([("error", J::s("the check panicked"))]))
                })
            })
            .collect()
    })
}

// ---- repair (derived artifacts only) -------------------------------------------------------------

/// One repair step: it runs, and says why when it fails.
type Repair<'a> = Box<dyn Fn() -> Result<(), String> + 'a>;

fn run_repairs(cfg: &Config) -> Vec<J> {
    // checkCommits' window, as a UTC date.
    let since = state::iso(state::now_ms() - 3 * 86_400_000)[..10].to_string();
    let steps: [(&str, Repair<'_>); 3] = [
        // Commits made outside a Claude Bash call get no PostToolUse note: recorded here, before
        // the reindex, so the new notes are indexed.
        (
            "commits",
            Box::new(|| {
                recorder::record(cfg, &since, false);
                Ok(())
            }),
        ),
        // Hubs write MOC links the index must see, so the reindex runs after them.
        (
            "hubs",
            Box::new(|| crate::hubs::write(cfg).map_err(|e| e.to_string())),
        ),
        (
            "reindex",
            Box::new(|| {
                index::open_rw(cfg)
                    .and_then(|mut c| index::sync(&mut c, cfg))
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }),
        ),
    ];
    steps
        .iter()
        .map(|(step, f)| {
            let t = Instant::now();
            let r = f();
            let ms = J::n(t.elapsed().as_millis() as i64);
            match r {
                Ok(()) => obj([("step", J::s(*step)), ("ok", J::Bool(true)), ("ms", ms)]),
                Err(e) => obj([
                    ("step", J::s(*step)),
                    ("ok", J::Bool(false)),
                    ("ms", ms),
                    ("error", J::s(e.lines().next().unwrap_or(""))),
                ]),
            }
        })
        .collect()
}

// ---- record --------------------------------------------------------------------------------------

/// Today's line in 50-Ops/Health/health-<month>.md; a re-run on one day replaces it, keeping the
/// explanation indented under it and a replaced FAIL line as a bullet.
fn write_record(cfg: &Config, checks: &[Check], ok: bool) -> String {
    let now = state::now_ms();
    let d = state::logical(cfg, now);
    let month = state::ym(&d);
    let day = state::ymd(&d);
    let ids = |level: &str| -> String {
        let v: Vec<&str> = checks
            .iter()
            .filter(|c| c.level == level && !c.ok)
            .map(|c| c.id)
            .collect();
        if v.is_empty() {
            "none".into()
        } else {
            v.join(", ")
        }
    };
    let notes = checks
        .iter()
        .find(|c| c.id == "vault-index")
        .and_then(|c| c.detail.get("notes"))
        .map_or("?".to_string(), J::text);
    let line = format!(
        "- {day} {} **{}** · failing: {} · warn: {} · notes={notes}",
        state::hm(&state::local(now)),
        if ok { "PASS" } else { "FAIL" },
        ids("fail"),
        ids("warn")
    );
    let dir = cfg.vault.join(OPS).join("Health");
    let file = dir.join(format!("health-{month}.md"));
    let created = !file.exists();
    let _ = fs::create_dir_all(&dir);
    let head = [
        "---".to_string(),
        "type: usage".into(),
        "project: knowledge".into(),
        "tags: [health, ops, generated]".into(),
        format!("created: {month}-01"),
        format!("updated: {month}-01"),
        "status: active".into(),
        "---".into(),
        String::new(),
        format!("# Vault health — {month}"),
        String::new(),
        "One line per logical day from `knowledge doctor --record`, which the session start runs once a day. The agent explains a FAIL in the session that sees it.".into(),
        String::new(),
    ]
    .join("\n");
    let mut body = if created { head } else { state::read(&file) };
    let mut lines: Vec<String> = body.split('\n').map(str::to_string).collect();
    if let Some(at) = lines
        .iter()
        .position(|l| l.starts_with(&format!("- {day} ")))
    {
        let old = lines[at].clone();
        let mut with = vec![line];
        if old.contains("**FAIL**") {
            with.push(format!("  - replaced: {}", &old[2..]));
        }
        lines.splice(at..=at, with);
        body = lines.join("\n");
    } else {
        body = format!("{}\n{line}\n", body.trim_end_matches('\n'));
    }
    // `updated` stays fixed: the file merges by union across the boots.
    let _ = fs::write(&file, body);
    // A script write fires no reindex hook: a month's first file needs the hubs, and every line
    // the index, or it goes `stale` 10 minutes later.
    if created {
        let _ = crate::hubs::write(cfg);
    }
    if let Ok(mut c) = index::open_rw(cfg) {
        let _ = index::sync(&mut c, cfg);
    }
    rel(&format!("{OPS}/Health/health-{month}.md"))
}

// ---- main ----------------------------------------------------------------------------------------

fn show(v: &J) -> String {
    match v {
        J::Str(s) => s.clone(),
        J::Obj(_) | J::Arr(_) | J::Null => v.text(),
        other => other.text(),
    }
}

pub fn run(args: &crate::Args) -> Result<u8, Fault> {
    let cfg = crate::config::load()?;
    let (pretty, repair, record) = (args.has("pretty"), args.has("repair"), args.has("record"));
    let only: Option<HashSet<String>> = args.get("only").map(|o| {
        o.split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    });
    if let Some(o) = &only {
        let unknown: Vec<&String> = o.iter().filter(|id| !IDS.contains(&id.as_str())).collect();
        if o.is_empty() || !unknown.is_empty() {
            let mut u: Vec<&str> = unknown.iter().map(|s| s.as_str()).collect();
            u.sort();
            eprintln!(
                "--only wants check ids from: {}{}",
                IDS.join(", "),
                if u.is_empty() {
                    String::new()
                } else {
                    format!(" (unknown: {})", u.join(", "))
                }
            );
            return Ok(1);
        }
        if record {
            eprintln!("--only and --record do not mix: a health line must cover every check");
            return Ok(1);
        }
    }
    let before = repair.then(|| run_checks(&cfg, &only));
    let repairs = repair.then(|| run_repairs(&cfg));
    let checks = run_checks(&cfg, &only);
    let ok = checks.iter().all(|c| c.level != "fail" || c.ok);
    let mut report = obj([
        ("ok", J::Bool(ok)),
        ("at", J::s(state::iso(state::now_ms()))),
        ("checks", J::Arr(checks.iter().map(Check::json).collect())),
    ]);
    let repaired: Vec<String> = before
        .iter()
        .flatten()
        .filter(|b| !b.ok && checks.iter().find(|c| c.id == b.id).is_some_and(|c| c.ok))
        .map(|b| b.id.to_string())
        .collect();
    if let Some(r) = &repairs {
        report.set("repairs", J::Arr(r.clone()));
        report.set("repaired", strs(&repaired));
    }
    let recorded = record.then(|| write_record(&cfg, &checks, ok));
    if let Some(r) = &recorded {
        report.set("recorded", J::s(r));
    }
    if !pretty {
        emit(&report.text());
        return Ok(0);
    }
    let mut out = format!(
        "knowledge doctor  {}  {}\n\n",
        if ok { "PASS" } else { "FAIL" },
        report.get("at").and_then(J::str).unwrap_or("")
    );
    if let Some(r) = &repairs {
        let steps: Vec<String> = r
            .iter()
            .map(|s| {
                let ok = matches!(s.get("ok"), Some(J::Bool(true)));
                format!(
                    "{}={} {}ms",
                    s.get("step").and_then(J::str).unwrap_or(""),
                    if ok { "ok" } else { "FAILED" },
                    s.get("ms").and_then(J::i64).unwrap_or(0)
                )
            })
            .collect();
        let fixed = if repaired.is_empty() {
            String::new()
        } else {
            format!("  -> fixed: {}", repaired.join(", "))
        };
        out.push_str(&format!("  repairs: {}{fixed}\n\n", steps.join("  ")));
    }
    for c in &checks {
        let mark = if c.ok {
            "PASS"
        } else if c.level == "warn" {
            "WARN"
        } else {
            "FAIL"
        };
        out.push_str(&format!("  {mark}  {}\n", c.id));
        if let J::Obj(items) = &c.detail {
            for (k, v) in items {
                match v {
                    J::Arr(xs) => xs
                        .iter()
                        .for_each(|x| out.push_str(&format!("          {}\n", show(x)))),
                    _ => out.push_str(&format!("          {k}: {}\n", show(v))),
                }
            }
        }
    }
    if let Some(r) = &recorded {
        out.push_str(&format!("\n  recorded: {r}\n"));
    }
    emit(&out);
    Ok(0)
}
