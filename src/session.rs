//! SessionStart: the always-true core of the brain, plus every check that must run before the
//! session reads the vault (brain-session.mjs, ported in Stage 6).
//!
//!   knowledge hook session --harness claude [--dry]
//!
//! Keyword search cannot answer "who am I" (no shared term with Identity.md), and a session started
//! outside the vault never loads its CLAUDE.md, so this carries the small tier that is always
//! relevant. Priming (Identity, Hard-Rules) is NOT injected here: ~/.claude/CLAUDE.md imports both
//! whole, and a hook's context over 10,000 chars is cut to a 2,000-char preview (2026-10-09).
//! `--dry` (the doctor measuring this output) pulls, commits and spawns nothing.
use crate::config::Config;
use crate::instructions::Form;
use crate::records;
use crate::state::{self, DECISIONS, J, META, OPS, SESSIONS};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Slack for a weekend off, tight enough to catch a real break early.
const STALE_DAYS: i64 = 3;
const DAY_MS: i64 = 86_400_000;

/// `Windows (NT 10.0.19045)`, as bun's `os.release()` printed it. `ver` is the one way to read it
/// without the registry or an `unsafe` call; it runs on its own thread while the pulls run.
#[cfg(windows)]
fn os_now() -> String {
    let o = state::run(
        Path::new("cmd"),
        &["/d", "/c", "ver"],
        None,
        None,
        &[],
        Duration::from_secs(5),
    );
    let v = o
        .stdout
        .split("Version ")
        .nth(1)
        .unwrap_or("")
        .trim_end_matches(|c: char| c == ']' || c.is_whitespace());
    let nt: Vec<&str> = v.split('.').take(3).collect();
    format!("Windows (NT {})", nt.join("."))
}

#[cfg(target_os = "linux")]
fn os_now() -> String {
    let distro = fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| {
            t.lines()
                .find_map(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| {
                    v.trim_start_matches('"')
                        .split(['"', '\n'])
                        .next()
                        .unwrap_or("")
                        .to_string()
                })
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Linux".into());
    let desktop: Vec<String> = ["XDG_CURRENT_DESKTOP", "XDG_SESSION_TYPE"]
        .iter()
        .filter_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .collect();
    if desktop.is_empty() {
        distro
    } else {
        format!("{distro} ({})", desktop.join(", "))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn os_now() -> String {
    std::env::consts::OS.to_string()
}

/// Which OS this session runs on, detected now: one machine may boot two OSes over one vault, so a
/// written "you are on Windows" would be wrong on the other boot.
fn machine(cfg: &Config, current: &str) -> String {
    let mut out = vec!["## Machine".to_string(), String::new()];
    if !cfg.machine.is_empty() {
        out.extend([cfg.machine.clone(), String::new()]);
    }
    out.push(format!(
        "**Current OS: {current}** (records stamped `{}`). Vault {}, checkouts {}.",
        cfg.os,
        cfg.vault.display(),
        cfg.dev.display()
    ));
    out.join("\n")
}

enum Pull {
    Busy,
    Skipped,
    Failed(String),
    Refused,
    Done { n: i64, before: String },
}

/// Fast-forward one checkout from its upstream: clean tree only (untracked files never block it),
/// pull only. Fetch then merge under a lock: two sessions starting together each ran `git pull`,
/// and the two fetches left FETCH_HEAD with two merge lines (2026-11-02).
fn ff_pull(cfg: &Config, cwd: &Path) -> Pull {
    let git = |a: &[&str]| state::git(cwd, a, Duration::from_secs(8));
    let name = cwd
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    state::with_lock(&format!("pull-{name}"), 60_000, || {
        if !git(&["status", "--porcelain", "--untracked-files=no"])
            .out()
            .is_empty()
        {
            return Pull::Skipped;
        }
        let before = git(&["rev-parse", "HEAD"]).out().to_string();
        // The records sync just before this fetched the vault already (it writes FETCH_HEAD): a
        // second fetch would be a second network round trip in the session start's path.
        let fresh = mtime_ms(&cwd.join(".git").join("FETCH_HEAD"))
            .is_some_and(|t| state::now_ms() - t < 10_000);
        if !fresh {
            let f = state::fetch(cwd, Duration::from_secs(8));
            if !f.ok() {
                return Pull::Failed(state::why(&f, false));
            }
        }
        if !records::tip_trusted(cfg, &git) {
            return Pull::Refused;
        }
        let m = git(&["merge", "--ff-only", "--quiet", "@{u}"]);
        if !m.ok() {
            return Pull::Failed(state::why(&m, false));
        }
        let n = git(&["rev-list", "--count", &format!("{before}..HEAD")])
            .out()
            .parse()
            .unwrap_or(0);
        Pull::Done { n, before }
    })
    .unwrap_or(Pull::Busy)
}

/// A refused pull: one line, and a failure the doctor's `auth` check reports until a good pull clears it.
fn refused(tag: &str, what: &str) -> String {
    state::record_failure(tag, records::UNTRUSTED_TIP);
    format!(
        "!! {what} PULL refused: {}. Nothing was merged; check who pushed it.",
        records::UNTRUSTED_TIP
    )
}

/// Records an earlier session left behind (its SessionEnd sync never ran, or the machine went down
/// first) go out before the vault pull, so the pull meets a clean tree. Returns the line and what
/// the sync itself pulled, which vaultPull counts into its own line and reindex.
fn records_sync(cfg: &Config) -> (Option<String>, i64) {
    let r = records::sync(cfg);
    let pulled = r.get("pulled").and_then(J::i64).unwrap_or(0);
    records::note_result(&r);
    if r.get("failed").is_some() {
        return (None, pulled); // the `!! KNOWLEDGE RECORDS FAILING` line carries it
    }
    let truthy = |k: &str| match r.get(k) {
        Some(J::Bool(b)) => *b,
        Some(J::Num(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(J::Str(s)) => !s.is_empty(),
        _ => false,
    };
    let line = if truthy("held") {
        Some(format!(
            "!! RECORDS committed but not pushed: {} has your own unpushed commits, and a push sends both. Push when they are ready.",
            cfg.vault.display()
        ))
    } else {
        match r.get("skipped").and_then(J::str) {
            Some("other changes are staged") => Some("!! RECORDS not committed: other changes are staged in the vault.".into()),
            Some("gitleaks not installed") => Some("!! RECORDS not committed: gitleaks is not on PATH, and records are never pushed unscanned. Install it (see docs/install.md § Prerequisites), restart Claude Code.".into()),
            _ if truthy("pushed") => {
                let c = r.get("committed").and_then(J::i64).unwrap_or(0);
                Some(format!("Records from an earlier session committed and pushed ({c} file{}).", if c == 1 { "" } else { "s" }))
            }
            _ => None,
        }
    };
    (line, pulled)
}

/// Fast-forward the vault from origin before the session reads it: the two boots never edit at
/// once, so every conflict so far was a forgotten pull. Silent when current.
fn vault_pull(cfg: &Config, by_records: i64) -> Option<String> {
    let r = ff_pull(cfg, &cfg.vault);
    let n = by_records + if let Pull::Done { n, .. } = &r { *n } else { 0 };
    if n > 0 {
        state::reindex();
    }
    match r {
        Pull::Busy => None,
        Pull::Skipped => Some("!! VAULT PULL skipped: uncommitted changes. Commit or stash, then `git pull --ff-only`.".into()),
        Pull::Failed(f) => Some(format!("!! VAULT PULL failed: {f}. Pull by hand before editing.")),
        Pull::Refused => Some(refused("pull-vault", "VAULT")),
        Pull::Done { .. } => {
            state::clear_failure("pull-vault");
            (n > 0).then(|| {
                format!("Vault pulled {n} commit{} from origin (the other OS may have written since). Reindexed.", if n == 1 { "" } else { "s" })
            })
        }
    }
}

/// A rule edited around the guard comes back from HEAD here, after the pull so HEAD is current;
/// the restore also re-adds a missing Identity or Hard-Rules import (K40). Silent when they match.
/// A child run of this binary: `rules check` prints as it goes, and its lines are the output.
fn rules_restore(cfg: &Config) -> Option<String> {
    if !cfg.vault.join(META).join("rules.lock").exists() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let o = state::run(
        &exe,
        &["rules", "check", "--restore"],
        None,
        None,
        &[],
        Duration::from_secs(10),
    );
    let out = format!("{}{}", o.stdout, o.stderr).trim().to_string();
    if o.ok() {
        return (!out.is_empty()).then_some(out);
    }
    if out.starts_with("!!") {
        return Some(out);
    }
    let code = o.code.map_or("null".to_string(), |c| c.to_string());
    Some(format!(
        "!! RULES check failed (exit {code}): {}",
        if out.is_empty() {
            "timeout".into()
        } else {
            out
        }
    ))
}

/// The engine checkout, as `setup` finds it (S9.7: no `~/.claude/hooks` link points into it now).
pub fn skeleton_root(cfg: &Config) -> Option<PathBuf> {
    crate::setup::checkout(&cfg.dev)
}

/// The engine's version: `Cargo.toml`'s `[package] version`. package.json held it until S9.7.
pub fn engine_version(root: &Path) -> Option<String> {
    package_field(root, "version")
}

/// A `key = "value"` line of `Cargo.toml`'s `[package]` table. A line scan, not a TOML parser: the
/// manifest is ours and keeps these two keys on one line each.
pub fn package_field(root: &Path, key: &str) -> Option<String> {
    let text = state::read(&root.join("Cargo.toml"));
    let mut in_package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            if k.trim() == key {
                return Some(v.trim().trim_matches('"').to_string()).filter(|v| !v.is_empty());
            }
        }
    }
    None
}

/// Fast-forward the engine the hooks run from, so both machines run the skeleton the vault pins.
/// A detached HEAD is a checkout pinned on purpose (a tag): left alone, silent.
fn skeleton_pull(cfg: &Config) -> Option<String> {
    let root = skeleton_root(cfg)?;
    if !state::git(
        &root,
        &["symbolic-ref", "-q", "HEAD"],
        Duration::from_secs(8),
    )
    .ok()
    {
        return None;
    }
    let shown = root.display();
    match ff_pull(cfg, &root) {
        Pull::Busy => None,
        Pull::Skipped => Some(format!(
            "!! SKELETON PULL skipped: uncommitted changes in {shown}."
        )),
        Pull::Failed(f) => Some(format!(
            "!! SKELETON PULL failed: {f}. Pull {shown} by hand."
        )),
        Pull::Refused => Some(refused("pull-skeleton", "SKELETON")),
        Pull::Done { n, before } => {
            state::clear_failure("pull-skeleton");
            if n == 0 {
                return None;
            }
            let hooks = state::git(
                &root,
                &[
                    "diff",
                    "--name-only",
                    &format!("{before}..HEAD"),
                    "--",
                    "claude",
                ],
                Duration::from_secs(8),
            );
            let mut line = format!(
                "Skeleton pulled {n} commit{} into {shown}; hooks started from now on run the new code.",
                if n == 1 { "" } else { "s" }
            );
            if !hooks.out().is_empty() {
                line.push_str(" The Claude plugin changed: run `claude plugin marketplace update knowledge`, then `claude plugin update knowledge@knowledge`, then /reload-plugins (an update is never automatic, K14).");
            }
            Some(line)
        }
    }
}

/// -1, 0 or 1 for two dotted versions, numerically per part ("0.10.0" is after "0.9.1").
fn compare_versions(a: &str, b: &str) -> i32 {
    let num = |p: Option<&str>| -> i64 {
        let d: String = p
            .unwrap_or("")
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        d.parse().unwrap_or(0)
    };
    let (pa, pb): (Vec<&str>, Vec<&str>) = (a.split('.').collect(), b.split('.').collect());
    for i in 0..pa.len().max(pb.len()) {
        let d = num(pa.get(i).copied()) - num(pb.get(i).copied());
        if d != 0 {
            return d.signum() as i32;
        }
    }
    0
}

pub fn json_version(file: &Path) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(&state::read(file)).ok()?;
    match &v["version"] {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    }
}

/// One line when the running skeleton is not the version the vault pins, naming which side moves.
fn pin_check(cfg: &Config) -> Option<String> {
    let root = skeleton_root(cfg)?;
    let version = engine_version(&root)?;
    let pin_file = cfg.vault.join(".skeleton.json");
    let pin = json_version(&pin_file).filter(|v| !v.is_empty())?;
    match compare_versions(&version, &pin) {
        0 => None,
        c if c < 0 => Some(format!(
            "!! SKELETON {version} is behind the vault's pin {pin}: `git -C {} pull --ff-only`.",
            root.display()
        )),
        _ => Some(format!(
            "!! SKELETON {version} is ahead of the vault's pin {pin}: read its changes since {pin} (`git -C {} log v{pin}..`), adapt the vault, then set {} to {version} and commit it.",
            root.display(),
            state::native(&pin_file)
        )),
    }
}

/// Every daily log's date under the sessions folder. A folder that cannot be read ends the walk
/// with an error, as bun's readdirSync threw.
fn log_days(dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if e.file_type()?.is_dir() {
            log_days(&e.path(), out)?;
        } else if state::is_log_name(&name) {
            out.push(name[..10].to_string());
        }
    }
    Ok(())
}

/// `YYYY-MM-DDT12:00:00` local, as `Date.parse` reads it, in milliseconds.
fn local_noon(day: &str) -> Option<i64> {
    let dt: jiff::civil::DateTime = format!("{day}T12:00:00").parse().ok()?;
    Some(
        dt.to_zoned_datetime(jiff::tz::TimeZone::system())
            .ok()?
            .timestamp()
            .as_millisecond(),
    )
}

pub fn mtime_ms(p: &Path) -> Option<i64> {
    let t = fs::metadata(p).ok()?.modified().ok()?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as i64)
}

/// The last flush outcome from the hook event log: it names the layer, not just the symptom.
fn last_flush(cfg: &Config) -> String {
    let fallback = format!("no flush in {OPS}/hook-events.jsonl yet");
    let text = fs::read_to_string(state::events_file(cfg)).unwrap_or_default();
    let lines: Vec<&str> = text.trim_end().split('\n').collect();
    for l in lines.iter().rev().take(500) {
        // An unreadable line ends the search, as JSON.parse's throw did.
        let Ok(e) = serde_json::from_str::<serde_json::Value>(l) else {
            return fallback;
        };
        if e["hook"] == "flush" {
            let ts = match &e["ts"] {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let outcome = match &e["outcome"] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => "undefined".into(),
                other => other.to_string(),
            };
            return format!(
                "last flush: {outcome} at {}Z",
                ts.chars().take(16).collect::<String>()
            );
        }
    }
    fallback
}

/// The flush canary: sessions are being captured but no daily note has appeared for days, so the
/// flush pipeline is dropping everything. Hooks fail silent by design, so this is the memory loop's
/// only monitor (a broken flush once wrote FLUSH_OK for ten days). mtime-only: a stat per folder.
fn flush_canary(cfg: &Config) -> Option<String> {
    let now = state::now_ms();
    let days = |ms: i64| (now - ms).div_euclid(DAY_MS);
    let mut logs = Vec::new();
    log_days(&cfg.vault.join(SESSIONS), &mut logs).ok()?;
    let newest_note = logs.iter().filter_map(|d| local_noon(d)).max().unwrap_or(0);
    // A transcript is created in its project's folder, so the newest folder mtime is the newest
    // session: one stat per project instead of one per transcript.
    let projects = state::home().join(".claude").join("projects");
    let newest_capture = fs::read_dir(projects)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter_map(|e| mtime_ms(&e.path()))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    if newest_capture == 0 {
        return None;
    }
    let quiet = days(newest_note);
    if quiet < STALE_DAYS || newest_capture <= newest_note {
        return None;
    }
    Some(format!(
        "!! FLUSH CANARY: sessions captured {}d ago, but the newest note in {SESSIONS}/ is {quiet}d old. The memory loop is capturing and not writing ({}). Diagnose with: knowledge flush <session-id> \"<cwd>\" --dry-run, and read the tail of {}.",
        days(newest_capture),
        last_flush(cfg),
        state::native(&state::events_file(cfg))
    ))
}

fn health_files(cfg: &Config) -> Vec<String> {
    let mut files: Vec<String> = fs::read_dir(cfg.vault.join(OPS).join("Health"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    files.retain(|f| {
        let b = f.as_bytes();
        f.len() == 17
            && f.starts_with("health-")
            && f.ends_with(".md")
            && b[7..11].iter().all(u8::is_ascii_digit)
            && b[11] == b'-'
            && b[12..14].iter().all(u8::is_ascii_digit)
    });
    files.sort();
    files
}

fn is_record_line(l: &str) -> bool {
    let b = l.as_bytes();
    b.len() >= 13
        && l.starts_with("- ")
        && b[2..12].iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
        && b[12] == b' '
}

/// The daily sweep's verdict, only when something is wrong: a green line every session is
/// wallpaper within a week. Stale means "the vault was used on a whole day with no sweep", not "N
/// days passed": days spent on the other boot or powered off are not misses. Today is excluded,
/// its sweep may still be running.
fn health_alarm(cfg: &Config) -> Option<String> {
    let files = health_files(cfg);
    let latest = files.last()?;
    // Sorted by "YYYY-MM-DD HH:MM", not file order: each boot appends its own lines and a pull
    // merges them, so the last line is not always the newest record (11-06 landed after 11-10).
    let text = state::read(&cfg.vault.join(OPS).join("Health").join(latest));
    let mut lines: Vec<&str> = text.split('\n').filter(|l| is_record_line(l)).collect();
    lines.sort_by(|a, b| a.get(2..18.min(a.len())).cmp(&b.get(2..18.min(b.len()))));
    let last = lines.last()?;
    let day = &last[2..12];
    let today = state::ymd(&state::local(state::now_ms() - 5 * 3_600_000));
    let mut missed = Vec::new();
    if log_days(&cfg.vault.join(SESSIONS), &mut missed).is_err() {
        missed.clear();
    }
    missed.retain(|d| d.as_str() > day && d.as_str() < today.as_str());
    missed.sort();
    if !missed.is_empty() {
        return Some(format!(
            "!! VAULT HEALTH: no sweep recorded since {day}, though the vault was used on {}. The session start runs it once a day, detached; run it by hand: knowledge doctor --repair --record --pretty",
            missed.join(", ")
        ));
    }
    if last.contains("**FAIL**") {
        let what = last.split('·').nth(1).map_or("undefined", str::trim);
        return Some(format!(
            "!! VAULT HEALTH FAIL ({day}): {what}. Details: knowledge doctor --pretty. Explain the failure to the user in the first reply."
        ));
    }
    None
}

/// The health sweep, once per logical day on each boot (spec item 28): `doctor --repair --record`,
/// detached, so the session start does not wait for its 22 checks. It replaced the desktop task
/// `vault-health`, a whole agent session a day to run a zero-token script. The day marker is
/// written before the spawn, so two sessions starting together sweep once.
fn health_sweep(cfg: &Config) {
    let marker = state::state_dir().join("health-sweep.day");
    let today = state::ymd(&state::logical(cfg, state::now_ms()));
    if state::read(&marker).trim() == today {
        return;
    }
    let _ = fs::create_dir_all(state::state_dir());
    if fs::write(&marker, &today).is_ok() {
        state::detach(
            &["doctor".into(), "--repair".into(), "--record".into()],
            "brain_health",
        );
    }
}

/// The session's cwd, from the first transcript record that carries one: only the note's label uses it.
fn cwd_of(tp: &Path) -> String {
    let mut buf = vec![0u8; 65_536];
    let n = fs::File::open(tp)
        .and_then(|mut f| f.read(&mut buf))
        .unwrap_or(0);
    let text = String::from_utf8_lossy(&buf[..n]);
    let Some(i) = text.find("\"cwd\":\"") else {
        return String::new();
    };
    let rest = &text[i + 6..];
    let b = rest.as_bytes();
    let mut j = 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return serde_json::from_str::<String>(&rest[..=j]).unwrap_or_default(),
            _ => j += 1,
        }
    }
    String::new()
}

/// Re-flush sessions whose last flush never finished: closing the app ends every session at once
/// and kills each detached flush mid-`claude -p`, so the watermark never advances (two sessions'
/// tails were lost on 2026-09-22). The lock and the watermark make a duplicate harmless.
fn reflush_orphans(cfg: &Config) {
    let text = fs::read_to_string(state::events_file(cfg)).unwrap_or_default();
    let mut last: Vec<(String, serde_json::Value)> = Vec::new();
    let mut tps: Vec<(String, String)> = Vec::new();
    for l in text.trim_end().split('\n') {
        let Ok(e) = serde_json::from_str::<serde_json::Value>(l) else {
            continue;
        };
        let Some(sid) = e["sid"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
        else {
            continue;
        };
        let hook = e["hook"].as_str().unwrap_or("");
        if hook != "flush" && hook != "capture" {
            continue;
        }
        if hook == "capture"
            && let Some(tp) = e["transcript"].as_str().filter(|t| !t.is_empty())
        {
            match tps.iter_mut().find(|(k, _)| *k == sid) {
                Some(slot) => slot.1 = tp.to_string(),
                None => tps.push((sid.clone(), tp.to_string())),
            }
        }
        // A Map keeps a key where it was first set.
        match last.iter_mut().find(|(k, _)| *k == sid) {
            Some(slot) => slot.1 = e,
            None => last.push((sid, e)),
        }
    }
    let now = state::now_ms();
    let today = state::ymd(&state::logical(cfg, now));
    let mut n = 0;
    for (sid, e) in &last {
        let Some(ts) = e["ts"].as_str().and_then(state::parse_ms) else {
            continue;
        };
        let age = now - ts;
        // 180 s: flush's lock expiry.
        if n >= 5 || !(180_000..=3 * DAY_MS).contains(&age) {
            continue;
        }
        if e["hook"] == "flush"
            && !["ledger", "busy", "failed"].contains(&e["outcome"].as_str().unwrap_or(""))
        {
            continue;
        }
        let Some(tp) = tps.iter().find(|(k, _)| k == sid).map(|(_, t)| t.clone()) else {
            continue;
        };
        if !Path::new(&tp).exists() {
            continue;
        }
        let mut argv = vec![
            "flush".to_string(),
            sid.clone(),
            cwd_of(Path::new(&tp)),
            "--transcript".into(),
            tp,
        ];
        let d = state::ymd(&state::logical(cfg, ts));
        if d != today {
            argv.extend(["--date".into(), d]);
        }
        state::detach(&argv, "brain_flush");
        n += 1;
    }
}

/// The session start's context, for the harness's translator to word; None without a config.
pub fn hook(dry: bool, instructions: Option<(&str, Form)>) -> Option<String> {
    let cfg = crate::config::load().ok()?;
    let os = std::thread::spawn(os_now);
    let current = |os: std::thread::JoinHandle<String>| {
        os.join().unwrap_or_else(|_| std::env::consts::OS.into())
    };
    // The OS line matters most when the vault is missing: that is how a fresh boot into the other
    // OS usually looks.
    if !cfg.vault.exists() {
        return Some(format!(
            "{}\n\nThe vault is not at {} on this OS.",
            machine(&cfg, &current(os)),
            cfg.vault.display()
        ));
    }
    // A `claude -p` this engine started opens a session too: no pulls, commits or spawns there.
    let live = !dry && !state::invoked();
    // First, so everything below reads the pulled vault and the pulled skeleton's version. The two
    // checkouts are independent repositories, so the skeleton's fetch runs beside the vault's
    // (bun ran them one after the other: two network round trips in the session start's path).
    let sk_cfg = cfg.clone();
    let skeleton = std::thread::spawn(move || if live { skeleton_pull(&sk_cfg) } else { None });
    let (records, by_records) = if live { records_sync(&cfg) } else { (None, 0) };
    let pulled = if live {
        vault_pull(&cfg, by_records)
    } else {
        None
    };
    let rules = if live { rules_restore(&cfg) } else { None };
    let skeleton = skeleton.join().unwrap_or(None);
    let pinned = pin_check(&cfg);
    let canary = flush_canary(&cfg);
    let health = health_alarm(&cfg);
    if live {
        reflush_orphans(&cfg);
        health_sweep(&cfg);
    }
    let pending = state::ledger_read(&cfg)
        .iter()
        .filter(|e| matches!(e.get("promoted"), Some(J::Null)))
        .count();
    let decisions = (pending > 0).then(|| {
        format!(
            "{pending} captured decision{} not yet in {DECISIONS}/ — promote with /knowledge new decision --ledger",
            if pending == 1 { "" } else { "s" }
        )
    });

    // The cause, when a hook recorded one: no waiting STALE_DAYS for the symptom. A refused pull
    // already has its own line.
    let failing: Vec<String> = state::failures()
        .into_iter()
        .filter(|(tag, _)| !tag.starts_with("pull-"))
        .map(|(tag, f)| {
            let at = f.get("at").and_then(J::i64).unwrap_or(0);
            let reason = f.get("reason").and_then(J::str).unwrap_or("undefined");
            format!(
                "!! KNOWLEDGE {} FAILING since {}Z: {reason}. Tell the user in the first reply; a `claude -p` auth failure needs them to run `claude` and /login.",
                tag.to_uppercase(),
                &state::iso(at)[..16]
            )
        })
        .collect();

    let kbin = state::native(&state::knowledge_bin()).replace('\\', "/");
    let vault = cfg.vault.display();
    let dev = cfg.dev.display();
    let mut ctx: Vec<String> = Vec::new();
    for l in failing {
        ctx.extend([l, String::new()]);
    }
    for l in [
        records, pulled, rules, skeleton, pinned, canary, health, decisions,
    ]
    .into_iter()
    .flatten()
    {
        ctx.extend([l, String::new()]);
    }
    ctx.push(machine(&cfg, &current(os)));
    // Claude imports the two files; a harness without imports has them written whole into its file.
    let how = match instructions {
        Some((file, Form::Inline)) => format!("written whole into {file}"),
        _ => "through ~/.claude/CLAUDE.md imports".into(),
    };
    ctx.extend([
        String::new(),
        format!("The user's second brain is at {vault}. Its Identity.md and Hard-Rules.md are loaded in full"),
        format!("{how} and are authoritative about who the user is and the rules."),
        String::new(),
        "Retrieval order for anything else - do not skip tiers:".into(),
        "  0. Note names injected on each prompt by the knowledge hook - vault notes only. Follow one into the file.".into(),
        "  1. 00-MOC-Root -> the 00-MOC-* it points to -> the note".into(),
        format!("  2. {kbin} find \"<q>\" - ranked notes, each with its best passage"),
        format!("  3. {kbin} graph <note> [<note>] - a note's links in and out, or the path between two"),
        "  4. Read one named file".into(),
        String::new(),
        format!("Tiers 2 and 3 read the vault index, so they work from any folder. Repo docs: {kbin} find \"<q>\" --repo."),
        format!("Details: {META}/Hard-Rules.md § Tooling."),
        "Check retrieval any time, zero tokens: knowledge doctor --pretty".into(),
        String::new(),
        format!("The vault is notes only - the checkouts moved to {} on 2026-09-06.", state::native(&cfg.dev.join("<remote-name>"))),
        format!("Do NOT sweep {dev} blind: that is where node_modules and target/ live. Repo docs are indexed"),
        format!("per repo ({kbin} find \"<q>\" --repo [<name>]) and their code is in the code map"),
        format!("({kbin} find <symbol> --code, graph <symbol> --code), so a blind sweep is the wrong tool there too."),
    ]);
    Some(ctx.join("\n"))
}
