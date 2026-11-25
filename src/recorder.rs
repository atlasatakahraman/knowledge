//! One vault note per commit, in every repo, with its why (brain-commit.mjs, ported in Stage 6).
//!
//!   knowledge hook post-bash --harness claude    a Bash call that may have made a commit
//!   knowledge post-bash --backfill YYYY-MM-DD    every unrecorded commit since that date
//!   ... --dry-run                                print what would be written
//!
//! Detection is stateless: the notes are the state. Every repo's recent log is scanned and whatever
//! has no note is recorded, so how or from where a commit was made does not matter.
use crate::config::Config;
use crate::linkcheck::LinkCheck;
use crate::state::{self, OPS, SESSIONS};
use crate::{Args, Fault, emit, vault};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const GIT_WAIT: Duration = Duration::from_secs(60);

pub struct Repo {
    pub root: PathBuf,
    pub name: String,
}

#[derive(Clone)]
pub struct Commit {
    pub root: PathBuf,
    pub name: String,
    pub hash: String,
    pub iso: String,
    pub subject: String,
    pub body: String,
    /// The source of a `cherry-pick -x`: the two are one change.
    pub picked: Option<String>,
}

fn git_out(cwd: &Path, args: &[&str]) -> Option<String> {
    let o = state::git(cwd, args, GIT_WAIT);
    o.ok().then_some(o.stdout)
}

/// The vault and every checkout under devRoot, each named for its GitHub remote.
pub fn repo_list(cfg: &Config) -> Vec<Repo> {
    let mut roots = vec![cfg.vault.clone()];
    if let Ok(rd) = fs::read_dir(&cfg.dev) {
        let mut dirs: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) || e.path().is_dir())
            .map(|e| e.path())
            .filter(|p| p.join(".git").exists())
            .collect();
        dirs.sort();
        roots.extend(dirs);
    }
    roots
        .into_iter()
        .map(|root| {
            let mut name = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(url) = git_out(&root, &["remote", "get-url", "origin"]) {
                let url = url.trim();
                let url = url.strip_suffix(".git").unwrap_or(url);
                if let Some(last) = url.rsplit(['/', ':']).next().filter(|s| !s.is_empty()) {
                    name = last.to_string();
                }
            }
            Repo { root, name }
        })
        .collect()
}

/// Non-merge commits since `since`, oldest first: HEAD plus `main`/`master`, since shared changes
/// are authored on `main` in a worktree while a checkout sits on a platform branch.
pub fn recent_commits(r: &Repo, since: &str) -> Vec<Commit> {
    let since = format!("--since={since}");
    let Some(out) = git_out(
        &r.root,
        &[
            "log",
            "HEAD",
            "--branches=main",
            "--branches=master",
            &since,
            "--no-merges",
            "--reverse",
            "--format=%H\x1f%cI\x1f%s\x1f%b\x1e",
        ],
    ) else {
        return Vec::new();
    };
    out.split('\x1e')
        .map(str::trim_start)
        .filter(|s| !s.is_empty())
        .filter_map(|rec| {
            let mut f = rec.splitn(4, '\x1f');
            let hash = f.next()?.to_string();
            let iso = f.next().unwrap_or("").to_string();
            let subject = f.next().unwrap_or("").to_string();
            let body = f.next().unwrap_or("").to_string();
            let ok = hash.len() == 40
                && hash
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c));
            ok.then(|| Commit {
                root: r.root.clone(),
                name: r.name.clone(),
                picked: picked_from(&body),
                hash,
                iso,
                subject: if subject.is_empty() {
                    "(no subject)".into()
                } else {
                    subject
                },
                body: body.trim().to_string(),
            })
        })
        .collect()
}

fn picked_from(body: &str) -> Option<String> {
    let k = body.find("(cherry picked from commit ")?;
    let h = &body[k + 27..];
    let hash = h.get(..40)?;
    (hash
        .bytes()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && h[40..].starts_with(')'))
    .then(|| hash.to_string())
}

pub struct Recorded {
    pub hashes: HashSet<String>,
    pub max_ordinal: HashMap<String, u32>,
}

/// What the notes record: full hashes, and each logical day's highest ordinal.
pub fn recorded(cfg: &Config) -> Recorded {
    let root = cfg.vault.join(OPS).join("Commits");
    let mut r = Recorded {
        hashes: HashSet::new(),
        max_ordinal: HashMap::new(),
    };
    let Ok(months) = fs::read_dir(&root) else {
        return r;
    };
    for m in months.flatten() {
        let month = m.file_name().to_string_lossy().into_owned();
        if !m.path().is_dir() || !is_month(&month) {
            continue;
        }
        let Ok(files) = fs::read_dir(m.path()) else {
            continue;
        };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            let Some((dd, n)) = note_name(&name) else {
                continue;
            };
            let day = format!("{month}-{dd}");
            let slot = r.max_ordinal.entry(day).or_insert(0);
            *slot = (*slot).max(n);
            if let Ok(text) = fs::read_to_string(f.path())
                && let Some(h) = text.lines().find_map(|l| {
                    let v = l.strip_prefix("hash:")?.trim_start();
                    let h = v.get(..40)?;
                    h.bytes()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                        .then_some(h)
                })
            {
                r.hashes.insert(h.to_string());
            }
        }
    }
    r
}

fn is_month(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7
        && b[4] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || c.is_ascii_digit())
}

/// `DD-xN-<rest>.md` -> (DD, N).
fn note_name(f: &str) -> Option<(String, u32)> {
    let rest = f.strip_suffix(".md")?;
    let (dd, rest) = (rest.get(..2)?, rest.get(2..)?);
    if !dd.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let rest = rest.strip_prefix("-x")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let tail = &rest[digits.len()..];
    (!digits.is_empty() && tail.len() > 1 && tail.starts_with('-'))
        .then(|| (dd.to_string(), digits.parse().unwrap_or(0)))
}

/// The commits that still need a note. A cherry-pick and its source are one change: if either
/// has a note, neither needs another; of two unrecorded ones, the first kept wins.
pub fn unrecorded(mut scan: Vec<Commit>, hashes: &HashSet<String>) -> Vec<Commit> {
    let key = |c: &Commit| c.picked.clone().unwrap_or_else(|| c.hash.clone());
    let mut done: HashSet<String> = hashes.clone();
    for c in scan.iter().filter(|c| hashes.contains(&c.hash)) {
        done.insert(key(c));
    }
    // Stable, by commit time, as Array.prototype.sort is.
    scan.sort_by_key(|c| state::parse_ms(&c.iso).unwrap_or(0));
    let mut seen = HashSet::new();
    scan.into_iter()
        .filter(|c| {
            let k = key(c);
            if done.contains(&c.hash) || done.contains(&k) || seen.contains(&k) {
                return false;
            }
            seen.insert(k);
            true
        })
        .collect()
}

/// What the hooks write on their own, vault-relative. A commit of these alone is a record, not
/// work: no note is written for it and the doctor's `commits` check skips it.
pub fn is_record_path(f: &str) -> bool {
    let records = [
        SESSIONS.to_string(),
        format!("{OPS}/Commits"),
        format!("{OPS}/decisions.jsonl"),
        format!("{OPS}/Health"),
        format!("{OPS}/Usage"),
        format!("{OPS}/00-MOC-Ops.md"),
    ];
    records
        .iter()
        .any(|p| f == p || f.starts_with(&format!("{p}/")))
}

pub fn files_of(c: &Commit) -> Vec<String> {
    git_out(&c.root, &["show", "--name-only", "--format=", &c.hash])
        .map(|o| {
            o.lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Subject -> filename slug: the conventional-commit prefix dropped, kebab-case, at most 48 chars.
fn slug_of(s: &str) -> String {
    let lower = s.to_lowercase();
    // `type(scope)!: ` at the very start.
    let b = lower.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_lowercase() {
        i += 1;
    }
    let mut rest = lower.as_str();
    if i > 0 {
        let mut j = i;
        if b.get(j) == Some(&b'(')
            && let Some(close) = lower[j..].find(')')
        {
            j += close + 1;
        }
        if b.get(j) == Some(&b'!') {
            j += 1;
        }
        if b.get(j) == Some(&b':') {
            j += 1;
            while b.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
                j += 1;
            }
            rest = &lower[j..];
        }
    }
    let mut slug = String::new();
    let mut dash = false;
    for c in rest.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            dash = false;
        } else if !dash {
            slug.push('-');
            dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    let cut: String = slug.chars().take(48).collect();
    let cut = cut.trim_end_matches('-');
    if cut.is_empty() {
        "commit".into()
    } else {
        cut.to_string()
    }
}

/// The OS a commit was made on: this one when the HEAD reflog shows it created here.
fn os_of(cfg: &Config, c: &Commit, made: &mut HashMap<PathBuf, HashSet<String>>) -> String {
    let set = made.entry(c.root.clone()).or_insert_with(|| {
        git_out(&c.root, &["reflog", "--format=%H %gs"])
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.contains("Fast-forward"))
            .filter(|l| creates_commit(l))
            .map(|l| l[..l.find(' ').unwrap_or(l.len())].to_string())
            .collect()
    });
    if set.contains(&c.hash) {
        cfg.os.clone()
    } else {
        "unknown".into()
    }
}

/// A reflog subject that made a commit here: commit, cherry-pick, merge, revert, am, or a rebase's
/// pick/reword/edit/squash/fixup (`rebase (start)` and a fast-forward only move to others' work).
fn creates_commit(line: &str) -> bool {
    let Some((hash, gs)) = line.split_once(' ') else {
        return false;
    };
    if hash.is_empty() || hash.contains(char::is_whitespace) {
        return false;
    }
    let word = |w: &str| {
        gs.strip_prefix(w).is_some_and(|r| {
            r.is_empty() || !r.starts_with(|c: char| c.is_alphanumeric() || c == '_')
        })
    };
    if ["commit", "cherry-pick", "merge", "revert", "am"]
        .iter()
        .any(|w| word(w))
    {
        return true;
    }
    for p in ["rebase -i (", "rebase (", "pull --rebase ("] {
        if let Some(r) = gs.strip_prefix(p)
            && ["pick)", "reword)", "edit)", "squash)", "fixup)"]
                .iter()
                .any(|k| r.starts_with(k))
        {
            return true;
        }
    }
    false
}

/// `day` is the commit's month folder and day of month, the two halves of its note's path.
fn note_for(
    cfg: &Config,
    links: &LinkCheck,
    c: &Commit,
    files: &[String],
    day: (&str, &str),
    n: u32,
    made: &mut HashMap<PathBuf, HashSet<String>>,
) -> (PathBuf, String) {
    let (month, dd) = day;
    let stat = git_out(&c.root, &["show", "--shortstat", "--format=", &c.hash])
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let mut areas: Vec<String> = Vec::new();
    for f in files {
        let a = match f.split_once('/') {
            Some((head, _)) => format!("{head}/"),
            None => f.clone(),
        };
        if !areas.contains(&a) {
            areas.push(a);
        }
    }
    let areas = areas.into_iter().take(6).collect::<Vec<_>>().join(" ");
    let day = format!("{month}-{dd}");
    let t = state::local(state::parse_ms(&c.iso).unwrap_or(0));
    let logged = cfg
        .vault
        .join(SESSIONS)
        .join(&month[..4])
        .join(&month[5..])
        .join(format!("{day}.md"))
        .exists();
    let why = match links.quote_dead(&c.body) {
        b if b.is_empty() => "_No commit body. Under D5 option A a commit record carries only what its message says — the why belongs in the body._".to_string(),
        b => b,
    };
    let file = cfg
        .vault
        .join(OPS)
        .join("Commits")
        .join(month)
        .join(format!("{dd}-x{n}-{}-{}.md", c.name, slug_of(&c.subject)));
    let lower = c.name.to_lowercase();
    let change = format!(
        "{}{}",
        if stat.is_empty() { "(no stat)" } else { &stat },
        if areas.is_empty() {
            String::new()
        } else {
            format!(" · {areas}")
        }
    );
    let text = [
        "---".to_string(),
        "type: commit".into(),
        format!("project: {lower}"),
        format!("tags: [commit, {lower}]"),
        format!("repo: {}", c.name),
        format!("hash: {}", c.hash),
        format!("committed: {}", c.iso),
        format!("os: {}", os_of(cfg, c, made)),
        format!("created: {day}"),
        format!("updated: {day}"),
        "status: active".into(),
        "---".into(),
        String::new(),
        format!("# {}", links.quote_dead(&c.subject)),
        String::new(),
        format!(
            "`{}` · {} · {} · {}",
            &c.hash[..7],
            c.name,
            state::hm(&t),
            if logged {
                format!("session log [[{day}]]")
            } else {
                "no session log".into()
            }
        ),
        String::new(),
        "## Why".into(),
        String::new(),
        why,
        String::new(),
        "## Change".into(),
        String::new(),
        change,
        String::new(),
    ]
    .join("\n");
    (file, text)
}

/// The month index, regenerated from the notes on disk.
fn write_index(cfg: &Config, month: &str) {
    let root = cfg.vault.join(OPS).join("Commits");
    let dir = root.join(month);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.retain(|f| note_name(f).is_some());
    names.sort_by(|a, b| numeric_cmp(a, b));
    let mut out = vec![
        "---".to_string(),
        "type: commits".into(),
        "project: knowledge".into(),
        "tags: [commits, git, ops, index]".into(),
        format!("created: {month}-01"),
        format!("updated: {month}-01"),
        "status: active".into(),
        "---".into(),
        String::new(),
        format!("# Commits — {month}"),
        String::new(),
        format!(
            "One note per commit, generated by `knowledge record` — {} commits. Rewritten on every record; do not edit by hand.",
            names.len()
        ),
    ];
    let mut last = String::new();
    for f in &names {
        let t = state::read(&dir.join(f));
        let title = t
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .map_or_else(|| f.clone(), str::to_string)
            .replace(['|', '[', ']'], "-");
        let repo = t
            .lines()
            .find_map(|l| l.strip_prefix("repo:"))
            .map(|r| r.trim().to_string())
            .unwrap_or_default();
        let dd = &f[..2];
        if dd != last {
            out.extend([String::new(), format!("## {month}-{dd}"), String::new()]);
            last = dd.to_string();
        }
        out.push(format!("- [[{}|{repo} · {title}]]", &f[..f.len() - 3]));
    }
    let _ = fs::write(
        root.join(format!("commits-{month}.md")),
        format!("{}\n", out.join("\n")),
    );
}

/// `localeCompare(b, undefined, { numeric: true })` for these names: digit runs compare as numbers.
fn numeric_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let o = m
                    .trim_start_matches('0')
                    .len()
                    .cmp(&n.trim_start_matches('0').len())
                    .then_with(|| m.trim_start_matches('0').cmp(n.trim_start_matches('0')));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(p), Some(q)) => {
                let o = locale_key(p).cmp(&locale_key(q));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// ICU's root order for what slugs hold: punctuation before digits before letters.
fn locale_key(c: char) -> (u8, char) {
    if c.is_ascii_alphabetic() {
        (2, c.to_ascii_lowercase())
    } else if c.is_ascii_digit() {
        (1, c)
    } else {
        (0, c)
    }
}

/// Record every unrecorded commit since `since`. The count written, and the dry-run lines.
pub fn record(cfg: &Config, since: &str, dry: bool) -> usize {
    let rec = recorded(cfg);
    let mut max = rec.max_ordinal;
    let scan: Vec<Commit> = repo_list(cfg)
        .iter()
        .flat_map(|r| recent_commits(r, since))
        .collect();
    let fresh = unrecorded(scan, &rec.hashes);
    if fresh.is_empty() {
        if dry {
            emit("[dry-run] 0 new commit notes\n");
        }
        return 0;
    }
    let links = LinkCheck::new(&cfg.vault, &vault::walk(&cfg.vault).unwrap_or_default());
    let mut made = HashMap::new();
    let mut months = BTreeSet::new();
    let mut md_repos = BTreeSet::new();
    let mut written = 0;
    for c in &fresh {
        let files = files_of(c);
        if c.root == cfg.vault && !files.is_empty() && files.iter().all(|f| is_record_path(f)) {
            continue;
        }
        let d = state::logical(cfg, state::parse_ms(&c.iso).unwrap_or(0));
        let (month, dd) = (state::ym(&d), format!("{:02}", d.day()));
        let slot = max.entry(format!("{month}-{dd}")).or_insert(0);
        *slot += 1;
        let n = *slot;
        let (file, text) = note_for(cfg, &links, c, &files, (&month, &dd), n, &mut made);
        written += 1;
        if dry {
            let rel = file
                .strip_prefix(&cfg.vault)
                .map_or(file.clone(), Path::to_path_buf);
            emit(&format!("[dry-run] {}\n", state::native(&rel)));
            continue;
        }
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(&file, text);
        months.insert(month);
        if c.root != cfg.vault && files.iter().any(|f| f.ends_with(".md")) {
            md_repos.insert(c.root.clone());
        }
    }
    if dry {
        emit(&format!("[dry-run] {written} new commit notes\n"));
        return written;
    }
    for m in &months {
        write_index(cfg, m);
    }
    // These notes are written here, not by Edit or Write, so the post-edit hook never sees them:
    // the vault index and the repo docs are brought level in-process.
    if written > 0 {
        if let Ok(mut conn) = crate::index::open_rw(cfg) {
            let _ = crate::index::sync(&mut conn, cfg);
        }
        let repos = cfg.repos();
        for r in &md_repos {
            if let Some(name) = repos.rel_of(r).filter(|n| !n.contains('/'))
                && let Ok(mut conn) = crate::index::open_rw(&repos)
            {
                let _ = crate::index::sync_repo(&mut conn, &repos, &name);
            }
        }
    }
    written
}

/// The PostToolUse hook: a loose prefilter (any `git` or `commit` word), then a three-day scan.
/// A false positive costs one scan that finds nothing; a false negative is the only real failure.
pub fn hook(cmd: &str) {
    if !has_word(cmd, "git") && !has_word(cmd, "commit") {
        return;
    }
    if let Ok(cfg) = crate::config::load() {
        record(&cfg, "3.days.ago", false);
    }
}

/// `\bword\b` as JavaScript reads it: word characters are ASCII letters, digits and `_`.
fn has_word(text: &str, word: &str) -> bool {
    let w = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    text.match_indices(word).any(|(i, _)| {
        !w(text[..i].chars().next_back()) && !w(text[i + word.len()..].chars().next())
    })
}

/// `knowledge post-bash --backfill YYYY-MM-DD [--dry-run]`.
pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &["backfill"])?;
    let cfg = crate::config::load()?;
    let since = match args.get("backfill") {
        // A date-only --since is read with the current time of day, so it would miss today.
        Some(d) => format!("{d} 00:00"),
        None => "3.days.ago".into(),
    };
    record(&cfg, &since, args.has("dry-run"));
    Ok(0)
}
