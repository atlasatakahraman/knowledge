//! The plumbing every hook verb shares, ported from brain-lib.mjs (Stage 6): local time and the
//! rollover, child processes with a timeout, detached re-runs of this binary, locks, the failure
//! records, the flush watermarks, the hook event log and the decision ledger. State keeps the paths
//! and formats the bun hooks used, so both can run side by side until the Arch pass.
use crate::config::Config;
use jiff::Timestamp;
use jiff::ZonedDateTime as Zoned;
use jiff::tz::TimeZone;
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const META: &str = "00-Meta";
pub const KNOWLEDGE: &str = "20-Knowledge";
pub const SESSIONS: &str = "30-Sessions";
pub const PLANS: &str = "40-Plans";
pub const OPS: &str = "50-Ops";
pub const DECISIONS: &str = "60-Decisions";

// ---- time ----------------------------------------------------------------------------------------

/// Now, in milliseconds. `KNOWLEDGE_NOW` pins it (milliseconds, an RFC 3339 instant, or a local
/// civil time like `2026-11-22T04:59`), so the rollover can be checked on both sides of 05:00.
pub fn now_ms() -> i64 {
    if let Ok(v) = std::env::var("KNOWLEDGE_NOW") {
        if let Ok(n) = v.parse::<i64>() {
            return n;
        }
        if let Ok(t) = v.parse::<Timestamp>() {
            return t.as_millisecond();
        }
        if let Ok(dt) = v.parse::<jiff::civil::DateTime>()
            && let Ok(z) = dt.to_zoned_datetime(TimeZone::system())
        {
            return z.timestamp().as_millisecond();
        }
    }
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// An instant in the system's time zone: what JavaScript's local `Date` getters read.
pub fn local(ms: i64) -> Zoned {
    Timestamp::from_millisecond(ms)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned_datetime(TimeZone::system())
}

/// The logical day an instant belongs to: the rollover hour (05:00 by default) is subtracted, so a
/// 02:00 session is still yesterday's. brain-lib's logicalDate().
pub fn logical(cfg: &Config, ms: i64) -> Zoned {
    local(ms - cfg.rollover * 3_600_000)
}

pub fn ymd(z: &Zoned) -> String {
    format!("{:04}-{:02}-{:02}", z.year(), z.month(), z.day())
}

pub fn ym(z: &Zoned) -> String {
    format!("{:04}-{:02}", z.year(), z.month())
}

pub fn hm(z: &Zoned) -> String {
    format!("{:02}:{:02}", z.hour(), z.minute())
}

/// JavaScript's `toISOString()`: UTC, always three decimals.
pub fn iso(ms: i64) -> String {
    let z = Timestamp::from_millisecond(ms)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned_datetime(TimeZone::UTC);
    format!(
        "{}T{:02}:{:02}:{:02}.{:03}Z",
        ymd(&z),
        z.hour(),
        z.minute(),
        z.second(),
        z.subsec_nanosecond() / 1_000_000
    )
}

/// An ISO-8601 instant (a transcript's `timestamp`, a commit's `%cI`) in milliseconds.
pub fn parse_ms(s: &str) -> Option<i64> {
    s.parse::<Timestamp>().ok().map(|t| t.as_millisecond())
}

/// The daily note an instant belongs to: `30-Sessions/YYYY/MM/YYYY-MM-DD.md` on its logical day.
pub fn note_path(cfg: &Config, ms: i64) -> PathBuf {
    let d = logical(cfg, ms);
    cfg.vault
        .join(SESSIONS)
        .join(format!("{:04}", d.year()))
        .join(format!("{:02}", d.month()))
        .join(format!("{}.md", ymd(&d)))
}

/// Create the daily note with its frontmatter, unless it exists.
pub fn ensure_note(cfg: &Config, file: &Path, ms: i64) {
    if file.exists() {
        return;
    }
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let stamp = ymd(&logical(cfg, ms));
    let _ = fs::write(
        file,
        format!(
            "---\ntype: session\nproject: knowledge\ntags: [session, log]\ncreated: {stamp}\nupdated: {stamp}\nstatus: active\n---\n\n# {stamp}\n\nAuto-captured. Write the real log with `/brain-log`.\n\n## Sessions\n\n"
        ),
    );
}

// ---- paths ---------------------------------------------------------------------------------------

pub fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

/// `~/.claude/hooks`: the skeleton's hooks folder, linked. Its `.state` holds the watermarks.
pub fn hooks_dir() -> PathBuf {
    home().join(".claude").join("hooks")
}

pub fn state_dir() -> PathBuf {
    hooks_dir().join(".state")
}

pub fn events_file(cfg: &Config) -> PathBuf {
    cfg.vault.join(OPS).join("hook-events.jsonl")
}

pub fn ledger_file(cfg: &Config) -> PathBuf {
    cfg.vault.join(OPS).join("decisions.jsonl")
}

/// A path as the bun hooks printed it: `path.join` on Windows writes `\`.
pub fn native(p: &Path) -> String {
    let s = p.to_string_lossy();
    if cfg!(windows) {
        s.replace('/', "\\")
    } else {
        s.into_owned()
    }
}

/// Session ids are UUIDs: anything else is dropped before an id names a file.
pub fn clean_sid(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

// ---- JSON that keeps its key order ---------------------------------------------------------------
// The state files and the event log are written by both bun and Rust during the cutover, and the
// parity gate compares them byte for byte. serde_json's map sorts keys (insertion order needs the
// indexmap crate), so this small value type keeps them in the order they were read or written.

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(serde_json::Number),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(v) => v.iter().find(|(key, _)| key == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn i64(&self) -> Option<i64> {
        match self {
            J::Num(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
            _ => None,
        }
    }
    pub fn s(v: impl Into<String>) -> J {
        J::Str(v.into())
    }
    pub fn n(v: i64) -> J {
        J::Num(v.into())
    }
    /// Set `k`, in place when present (JavaScript's `{ ...a, k }` keeps the old position).
    pub fn set(&mut self, k: &str, v: J) {
        if let J::Obj(items) = self {
            match items.iter_mut().find(|(key, _)| key == k) {
                Some(slot) => slot.1 = v,
                None => items.push((k.to_string(), v)),
            }
        }
    }
    pub fn remove(&mut self, k: &str) {
        if let J::Obj(items) = self {
            items.retain(|(key, _)| key != k);
        }
    }
    pub fn text(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// `{"k": v, ...}` in the order given.
pub fn obj<const N: usize>(items: [(&str, J); N]) -> J {
    J::Obj(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl Serialize for J {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            J::Null => s.serialize_unit(),
            J::Bool(b) => s.serialize_bool(*b),
            J::Num(n) => n.serialize(s),
            J::Str(v) => s.serialize_str(v),
            J::Arr(v) => {
                let mut seq = s.serialize_seq(Some(v.len()))?;
                for x in v {
                    seq.serialize_element(x)?;
                }
                seq.end()
            }
            J::Obj(v) => {
                let mut map = s.serialize_map(Some(v.len()))?;
                for (k, x) in v {
                    map.serialize_entry(k, x)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for J {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<J, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = J;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_unit<E: de::Error>(self) -> Result<J, E> {
                Ok(J::Null)
            }
            fn visit_none<E: de::Error>(self) -> Result<J, E> {
                Ok(J::Null)
            }
            fn visit_bool<E: de::Error>(self, b: bool) -> Result<J, E> {
                Ok(J::Bool(b))
            }
            fn visit_i64<E: de::Error>(self, n: i64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }
            fn visit_u64<E: de::Error>(self, n: u64) -> Result<J, E> {
                Ok(J::Num(n.into()))
            }
            fn visit_f64<E: de::Error>(self, n: f64) -> Result<J, E> {
                Ok(serde_json::Number::from_f64(n).map_or(J::Null, J::Num))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<J, E> {
                Ok(J::Str(v.to_string()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<J, E> {
                Ok(J::Str(v))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element()? {
                    v.push(x);
                }
                Ok(J::Arr(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<J, A::Error> {
                let mut v: Vec<(String, J)> = Vec::new();
                while let Some((k, x)) = a.next_entry::<String, J>()? {
                    // A repeated key keeps its first position and its last value, as JSON.parse does.
                    match v.iter_mut().find(|(key, _)| *key == k) {
                        Some(slot) => slot.1 = x,
                        None => v.push((k, x)),
                    }
                }
                Ok(J::Obj(v))
            }
        }
        d.deserialize_any(V)
    }
}

pub fn read_json(path: &Path) -> Option<J> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// `JSON.stringify(v, null, 1)`: one space per level, as the compile state is written.
pub fn pretty1(v: &J) -> String {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(
        &mut buf,
        serde_json::ser::PrettyFormatter::with_indent(b" "),
    );
    let _ = v.serialize(&mut ser);
    String::from_utf8(buf).unwrap_or_default()
}

// ---- processes -----------------------------------------------------------------------------------

/// A finished child: its exit, its output, and whether it ran at all.
pub struct Out {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// The program was not found (ENOENT).
    pub missing: bool,
    /// Killed at the timeout.
    pub timed_out: bool,
}

impl Out {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
    pub fn out(&self) -> &str {
        self.stdout.trim()
    }
    pub fn lines(&self) -> Vec<&str> {
        self.stdout.lines().filter(|l| !l.is_empty()).collect()
    }
}

#[cfg(windows)]
fn hide(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide(_: &mut Command) {}

/// Run a program to its end or its timeout, with no console window. stdout and stderr are read on
/// their own threads, so a chatty child cannot fill a pipe and stall; the wait polls, starting at
/// 1 ms, so a quick `git` costs no fixed sleep.
pub fn run(
    program: &Path,
    args: &[&str],
    cwd: Option<&Path>,
    input: Option<&str>,
    envs: &[(&str, &str)],
    timeout: Duration,
) -> Out {
    let mut c = Command::new(program);
    c.args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    for (k, v) in envs {
        c.env(k, v);
    }
    hide(&mut c);
    let mut child = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => {
            return Out {
                code: None,
                stdout: String::new(),
                stderr: e.to_string(),
                missing: e.kind() == std::io::ErrorKind::NotFound,
                timed_out: false,
            };
        }
    };
    if let (Some(text), Some(mut sin)) = (input, child.stdin.take()) {
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = sin.write_all(text.as_bytes());
        });
    }
    let pipe = |p: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = p {
                let _ = r.read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = pipe(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = pipe(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let start = Instant::now();
    let mut nap = Duration::from_millis(1);
    let (code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(s)) => break (s.code(), false),
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            Ok(None) => {
                std::thread::sleep(nap);
                nap = (nap * 2).min(Duration::from_millis(10));
            }
            Err(_) => break (None, false),
        }
    };
    Out {
        code,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
        missing: false,
        timed_out,
    }
}

/// `git` in `cwd`, never prompting for credentials.
pub fn git(cwd: &Path, args: &[&str], timeout: Duration) -> Out {
    run(
        Path::new("git"),
        args,
        Some(cwd),
        None,
        &[("GIT_TERMINAL_PROMPT", "0")],
        timeout,
    )
}

/// The first useful line of a failed child, colours stripped: what a failure record says.
pub fn why(o: &Out, last: bool) -> String {
    let text = if !o.stderr.trim().is_empty() {
        o.stderr.clone()
    } else if o.timed_out || o.code.is_none() {
        "timeout".to_string()
    } else {
        String::new()
    };
    let mut clean = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            for d in chars.by_ref() {
                if d == 'm' {
                    break;
                }
            }
            continue;
        }
        clean.push(c);
    }
    let t = clean.trim();
    let line = if last {
        t.lines().last()
    } else {
        t.lines().next()
    };
    line.unwrap_or("").to_string()
}

/// This binary, re-run detached: `knowledge <args>` outlives the hook that started it, with no
/// window and no stdio. `tag` goes in CLAUDE_INVOKED_BY, which every hook verb checks first, so the
/// `claude -p` a child starts cannot re-fire the hooks.
pub fn detach(args: &[String], tag: &str) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut c = Command::new(exe);
    c.args(args)
        .env("CLAUDE_INVOKED_BY", tag)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console, not attached to the hook's console, and out of its Ctrl+C group, so the
        // harness ending the hook does not end the child.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        // Else the child inherits the hook's stdout and Claude Code waits for it to end.
        crate::win::stdio_not_inherited();
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: a signal to the hook's group does not reach it.
        c.process_group(0);
    }
    let _ = c.spawn();
}

/// True inside a `claude -p` this engine started: every hook verb exits at once there.
pub fn invoked() -> bool {
    std::env::var_os("CLAUDE_INVOKED_BY").is_some_and(|v| !v.is_empty())
}

/// The knowledge binary the bun hooks would name: $KNOWLEDGE_BIN, else cargo's, else this one.
pub fn knowledge_bin() -> PathBuf {
    let exe = if cfg!(windows) {
        "knowledge.exe"
    } else {
        "knowledge"
    };
    let cargo = std::env::var_os("CARGO_HOME")
        .map_or_else(|| home().join(".cargo"), PathBuf::from)
        .join("bin")
        .join(exe);
    [
        std::env::var_os("KNOWLEDGE_BIN").map(PathBuf::from),
        Some(cargo),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.exists())
    .or_else(|| std::env::current_exe().ok())
    .unwrap_or_else(|| PathBuf::from(exe))
}

/// Bring the vault index level with the disk, detached: for writers that never reach the
/// post-edit hook (a pull, compile's `claude -p`, the commit notes).
pub fn reindex() {
    detach(&["index".to_string()], "knowledge_index");
}

// ---- locks ---------------------------------------------------------------------------------------

/// Run `f` holding an exclusive lock in the state dir; None when another process holds it. The
/// create is the check, so two sessions starting at once cannot both enter. A lock older than
/// `stale_ms` belongs to a dead process and is taken over.
pub fn with_lock<T>(name: &str, stale_ms: i64, f: impl FnOnce() -> T) -> Option<T> {
    let dir = state_dir();
    let _ = fs::create_dir_all(&dir);
    let lock = dir.join(format!("{name}.lock"));
    if !claim(&lock, stale_ms) {
        return None;
    }
    let out = f();
    let _ = fs::remove_file(&lock);
    Some(out)
}

/// Create `lock` exclusively; take it over when older than `stale_ms`.
pub fn claim(lock: &Path, stale_ms: i64) -> bool {
    let create = || {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock)
            .is_ok()
    };
    if create() {
        return true;
    }
    let age = fs::metadata(lock)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map_or(0, |d| d.as_millis() as i64);
    if age < stale_ms {
        return false;
    }
    let _ = fs::remove_file(lock);
    create()
}

// ---- failure records -----------------------------------------------------------------------------

/// Why the last background step failed, by tag; the session start prints each as
/// `!! BRAIN <TAG> FAILING`. In the temp folder, as brain-lib keeps it.
fn failures_file() -> PathBuf {
    std::env::temp_dir().join("brain-claude-failures.json")
}

pub fn failures() -> Vec<(String, J)> {
    match read_json(&failures_file()) {
        Some(J::Obj(v)) => v,
        _ => Vec::new(),
    }
}

pub fn record_failure(tag: &str, reason: &str) {
    let mut all = J::Obj(failures());
    all.set(tag, obj([("at", J::n(now_ms())), ("reason", J::s(reason))]));
    let _ = fs::write(failures_file(), all.text());
}

pub fn clear_failure(tag: &str) {
    let mut all = failures();
    let n = all.len();
    all.retain(|(k, _)| k != tag);
    if all.len() != n {
        let _ = fs::write(failures_file(), J::Obj(all).text());
    }
}

/// The reason a `claude -p` failed: its first meaningful line, stdout included (an auth failure
/// prints there), hook chatter skipped; else `message`, the error's own first line.
pub fn claude_reason(o: &Out, message: &str) -> String {
    format!("{}\n{}", o.stderr, o.stdout)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.contains("hook ["))
        .map_or_else(|| message.to_string(), str::to_string)
}

// ---- the event log -------------------------------------------------------------------------------

/// One JSON line per capture and per flush outcome, so a missing note can be traced to the layer
/// that dropped it. `ts` first, then the fields in the order given.
pub fn log_event(cfg: &Config, mut fields: Vec<(String, J)>) {
    fields.insert(0, ("ts".to_string(), J::s(iso(now_ms()))));
    let line = format!("{}\n", J::Obj(fields).text());
    if let Ok(mut f) = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(events_file(cfg))
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// `ev!("hook" => J::s("flush"), ...)` builds the field list for log_event.
#[macro_export]
macro_rules! ev {
    ($($k:expr => $v:expr),* $(,)?) => { vec![$(($k.to_string(), $v)),*] };
}

// ---- watermarks ----------------------------------------------------------------------------------

fn watermarks_file() -> PathBuf {
    state_dir().join("flush-watermarks.json")
}

/// `{ sid: { events, turns, triedAt, flushedAt } }`: how far each session has been flushed.
pub fn watermark(sid: &str) -> J {
    match read_json(&watermarks_file()) {
        Some(all) => all.get(sid).cloned().unwrap_or(J::Obj(Vec::new())),
        None => J::Obj(Vec::new()),
    }
}

/// Merge `patch` into one session's mark; sessions untouched for 30 days are pruned.
pub fn write_watermark(sid: &str, patch: Vec<(String, J)>) {
    let _ = fs::create_dir_all(state_dir());
    let mut all = match read_json(&watermarks_file()) {
        Some(j @ J::Obj(_)) => j,
        _ => J::Obj(Vec::new()),
    };
    let mut mark = all.get(sid).cloned().unwrap_or(J::Obj(Vec::new()));
    for (k, v) in patch {
        mark.set(&k, v);
    }
    all.set(sid, mark);
    let cutoff = now_ms() - 30 * 86_400_000;
    if let J::Obj(items) = &mut all {
        items.retain(|(_, v)| {
            v.get("triedAt")
                .or_else(|| v.get("flushedAt"))
                .and_then(J::i64)
                .unwrap_or(0)
                >= cutoff
        });
    }
    let _ = fs::write(watermarks_file(), all.text());
}

// ---- transcripts ---------------------------------------------------------------------------------

/// A session's transcript: the payload's path when it exists, else
/// `~/.claude/projects/<slug>/<sid>.jsonl` (the payload's path is sometimes absent or stale).
pub fn resolve_transcript(sid: &str, given: Option<&str>) -> Option<PathBuf> {
    if let Some(g) = given.filter(|g| !g.is_empty())
        && Path::new(g).exists()
    {
        return Some(PathBuf::from(g));
    }
    let sid = clean_sid(sid);
    if sid.is_empty() {
        return None;
    }
    let root = home().join(".claude").join("projects");
    fs::read_dir(root).ok()?.flatten().find_map(|d| {
        let f = d.path().join(format!("{sid}.jsonl"));
        (d.file_type().is_ok_and(|t| t.is_dir()) && f.exists()).then_some(f)
    })
}

// ---- the decision ledger -------------------------------------------------------------------------

pub fn ledger_read(cfg: &Config) -> Vec<J> {
    fs::read_to_string(ledger_file(cfg))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Append the entries whose `src` is not in the ledger yet; how many went in.
pub fn ledger_add(cfg: &Config, entries: Vec<J>) -> usize {
    let seen: std::collections::HashSet<String> = ledger_read(cfg)
        .iter()
        .filter_map(|e| e.get("src").and_then(J::str).map(str::to_string))
        .collect();
    let ts = iso(now_ms());
    let mut out = String::new();
    let mut n = 0;
    for e in entries {
        let Some(src) = e.get("src").and_then(J::str) else {
            continue;
        };
        if seen.contains(src) {
            continue;
        }
        let J::Obj(fields) = e else { continue };
        let mut line = vec![
            ("ts".to_string(), J::s(&ts)),
            ("promoted".to_string(), J::Null),
        ];
        let mut line_obj = J::Obj(std::mem::take(&mut line));
        for (k, v) in fields {
            line_obj.set(&k, v);
        }
        out.push_str(&line_obj.text());
        out.push('\n');
        n += 1;
    }
    if n > 0
        && let Ok(mut f) = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(ledger_file(cfg))
    {
        let _ = f.write_all(out.as_bytes());
    }
    n
}

/// Read stdin whole; the hook payload. Bad or absent JSON is an empty object.
pub fn payload(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or(serde_json::Value::Object(Default::default()))
}

/// The file's text, or "" when unreadable.
pub fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}
