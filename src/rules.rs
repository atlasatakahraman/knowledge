//! `knowledge rules`: the rules system (spec D15, K44). Hard-Rules stays one note: each `##`
//! section is a pack, each top-level bullet opens with a bold line that is its rule, and that
//! line's slug is the rule's id. A rule runs to the next top-level bullet or heading. Text before a
//! section's first bullet is the pack's preamble. `00-Meta/rules.lock` holds what the text does
//! not: packs, kinds, enforcement points, guards and a hash of every unit. Nothing is added to the
//! primed text, so the metadata costs no tokens.
use crate::config::{self, Config};
use crate::{Args, Fault, emit, vault};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const RULES: &str = "00-Meta/Hard-Rules.md";
pub const LOCK: &str = "00-Meta/rules.lock";
const PRIMED: [&str; 2] = ["Identity.md", "Hard-Rules.md"];

/// Code, not text: changing one is a new release (D15). The spec's nine.
const KERNEL: [&str; 9] = [
    "Notes are the truth; nothing lives only in the database.",
    "No telemetry. No network call except `fetch` on a URL the caller gave, and the git commands the engine is told to run.",
    "`fetch` refuses private, loopback and link-local targets, and checks again on every redirect.",
    "Nothing read from a note, a page or a capture is executed. Injected text is marked as data and carries its source.",
    "Nothing is written outside the vault and the knowledge home.",
    "Records and commits pass the secret scan or fail.",
    "With trust on, an unsigned pull is refused.",
    "Every output has a budget.",
    "A locked rule cannot be removed or weakened by any verb an agent runs.",
];

/// A unit of the rules note: the head, a pack's preamble, or a rule.
pub struct Unit {
    pub id: String,
    pub section: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Lock {
    pub packs: Vec<Pack>,
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub guards: Vec<Guard>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Pack {
    pub id: String,
    pub section: String,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Rule {
    pub id: String,
    pub pack: String,
    pub kind: String,
    pub enforce: Vec<String>,
    /// FNV-1a of the rule's text, for change detection. Not a defence against forgery: whoever can
    /// write a colliding rule can as easily rewrite this file, and the guard, the restore from
    /// `HEAD` and the owner's commit go are what stand in that way (plan change 5).
    pub hash: String,
}

/// A guard (S5.3): what it matches, what it answers, and the backstop the commit check runs.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Guard {
    pub id: String,
    pub rule: String,
    /// `deny` or `ask`.
    pub action: String,
    pub reason: String,
    /// Shell segments whose words start with one of these sequences. `*` matches any one word.
    pub words: Vec<Vec<String>>,
    /// Words that, when present, keep a `words` match from firing (`bunx --bun`).
    pub unless: Vec<String>,
    /// Paths an Edit/Write may not touch, and that a shell writer may not name. A leading `*`
    /// matches any prefix; `~/` is the home folder.
    pub paths: Vec<String>,
    /// MCP tool names. A leading or trailing `*` matches any prefix or suffix.
    pub tools: Vec<String>,
    /// The backstop: staged file names that fail a commit.
    pub staged: Vec<String>,
    /// Only in a checkout whose folder is named this (`AtlasAta-DotFiles`), and only on this branch.
    pub repo: String,
    pub branch: String,
    /// Only when the engine runs on this OS (`windows`, `linux`, `macos`).
    pub os: String,
}

/// What `rules lock --seed` reads once: section → pack id, rule id → kind and enforcement, and
/// the guards. After the first lock, `rules lock` keeps all of it and only re-stamps the hashes.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Seed {
    packs: HashMap<String, String>,
    rules: HashMap<String, Meta>,
    guards: Vec<Guard>,
}

#[derive(Deserialize, Clone)]
struct Meta {
    kind: String,
    enforce: Vec<String>,
}

const USAGE: &str = "usage: knowledge rules list [--json] | kernel | check [--staged] [--restore] | lock [--seed <file>] | enable|disable <pack>";

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &["seed"])?;
    let verb = args.words.first().map(String::as_str);
    if verb == Some("kernel") {
        emit(&KERNEL.map(|k| format!("- {k}\n")).concat());
        return Ok(0);
    }
    let cfg = config::load()?;
    match verb {
        Some("list") => list(&cfg, &args),
        Some("check") => check(&cfg, &args),
        Some("lock") => lock(&cfg, &args),
        Some(v @ ("enable" | "disable")) => toggle(&cfg, &args, v == "enable"),
        _ => Err(Fault::Usage(USAGE.into())),
    }
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out = out.trim_matches('-').to_string();
    out.truncate(48);
    out.trim_end_matches('-').to_string()
}

pub fn parse(src: &str) -> Result<Vec<Unit>, Fault> {
    let text = src.replace("\r\n", "\n");
    let body = match text
        .strip_prefix("---\n")
        .and_then(|t| t.split_once("\n---\n"))
    {
        Some((_, rest)) => rest,
        None => &text,
    };
    let mut units = Vec::new();
    let mut section = String::new();
    let mut cur = Unit {
        id: "head".into(),
        section: String::new(),
        text: String::new(),
    };
    let mut fence = false;
    for line in body.lines() {
        if !fence {
            if let Some(h) = line.strip_prefix("## ") {
                section = h.trim().to_string();
                let next = Unit {
                    id: format!("{}.preamble", slug(&section)),
                    section: section.clone(),
                    text: String::new(),
                };
                push(&mut units, std::mem::replace(&mut cur, next))?;
            } else if line.starts_with("- ") {
                // The id comes from the bold lead, which may wrap; push() reads it from the text.
                let next = Unit {
                    id: String::new(),
                    section: section.clone(),
                    text: String::new(),
                };
                push(&mut units, std::mem::replace(&mut cur, next))?;
            }
        }
        if line.trim_start().starts_with("```") {
            fence = !fence;
        }
        cur.text.push_str(line);
        cur.text.push('\n');
    }
    push(&mut units, cur)?;
    let mut seen = HashMap::new();
    for u in &units {
        if let Some(prev) = seen.insert(u.id.as_str(), u.section.as_str()) {
            return Err(Fault::Usage(format!(
                "{RULES}: two rules share the id `{}` (in \"{prev}\" and \"{}\"): change one bold line",
                u.id, u.section
            )));
        }
    }
    Ok(units)
}

fn push(units: &mut Vec<Unit>, u: Unit) -> Result<(), Fault> {
    let text = u.text.trim();
    if text.is_empty() {
        return Ok(());
    }
    let id = if u.id.is_empty() {
        let lead = text
            .strip_prefix("- **")
            .and_then(|r| r.split_once("**"))
            .map(|(l, _)| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .ok_or_else(|| {
                let start: String = text.chars().take(50).collect();
                Fault::Usage(format!(
                    "{RULES}: a rule opens with a bold line: \"{start}\""
                ))
            })?;
        slug(&lead)
    } else {
        u.id
    };
    units.push(Unit {
        id,
        section: u.section,
        text: text.to_string(),
    });
    Ok(())
}

/// Every unit that differs from the lock, as "<id> changed|added|removed".
pub fn diff(units: &[Unit], lock: &Lock) -> Vec<String> {
    let have: HashMap<&str, &Unit> = units.iter().map(|u| (u.id.as_str(), u)).collect();
    let mut out = Vec::new();
    for r in &lock.rules {
        match have.get(r.id.as_str()) {
            None => out.push(format!("{} removed", r.id)),
            Some(u) if vault::hash(u.text.as_bytes()) != r.hash => {
                out.push(format!("{} changed", r.id))
            }
            Some(_) => {}
        }
    }
    for u in units {
        if !lock.rules.iter().any(|r| r.id == u.id) {
            out.push(format!("{} added", u.id));
        }
    }
    out
}

pub fn read_lock(path: &Path) -> Result<Option<Lock>, Fault> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(serde_json::from_str(&s)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Fault::io(path, e)),
    }
}

fn need_lock(cfg: &Config) -> Result<Lock, Fault> {
    read_lock(&cfg.abs_of(LOCK))?.ok_or_else(|| {
        Fault::Config(format!(
            "no {LOCK}: the owner runs `knowledge rules lock` first"
        ))
    })
}

fn write_lock(cfg: &Config, lock: &Lock) -> Result<(), Fault> {
    let path = cfg.abs_of(LOCK);
    let text = serde_json::to_string_pretty(lock)? + "\n";
    std::fs::write(&path, text).map_err(|e| Fault::io(&path, e))
}

fn read(path: &Path) -> Result<String, Fault> {
    std::fs::read_to_string(path).map_err(|e| Fault::io(path, e))
}

/// `git show <spec>` in the vault: `HEAD:<path>` or `:<path>` (the staged copy).
fn git_show(cfg: &Config, spec: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(&cfg.vault)
        .args(["show", spec])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn build(units: &[Unit], old: Option<&Lock>, seed: &Seed) -> Lock {
    let mut packs: Vec<Pack> = Vec::new();
    for u in units {
        if packs.iter().any(|p| p.section == u.section) {
            continue;
        }
        let was = old.and_then(|l| l.packs.iter().find(|p| p.section == u.section));
        let id = seed
            .packs
            .get(&u.section)
            .cloned()
            .or_else(|| was.map(|p| p.id.clone()))
            .unwrap_or_else(|| {
                if u.section.is_empty() {
                    "head".into()
                } else {
                    slug(&u.section)
                }
            });
        packs.push(Pack {
            id,
            section: u.section.clone(),
            enabled: was.is_none_or(|p| p.enabled),
        });
    }
    let rules = units
        .iter()
        .map(|u| {
            let pack = packs
                .iter()
                .find(|p| p.section == u.section)
                .map(|p| p.id.clone())
                .unwrap_or_default();
            let was = old.and_then(|l| l.rules.iter().find(|r| r.id == u.id));
            let meta = seed.rules.get(&u.id).cloned().or_else(|| {
                was.map(|r| Meta {
                    kind: r.kind.clone(),
                    enforce: r.enforce.clone(),
                })
            });
            let preamble = u.id == "head" || u.id.ends_with(".preamble");
            let meta = meta.unwrap_or_else(|| Meta {
                kind: if preamble { "preamble" } else { "agent" }.into(),
                enforce: vec!["P".into(), "R".into()],
            });
            Rule {
                id: u.id.clone(),
                pack,
                kind: meta.kind,
                enforce: meta.enforce,
                hash: vault::hash(u.text.as_bytes()),
            }
        })
        .collect();
    let guards = if seed.guards.is_empty() {
        old.map(|l| l.guards.clone()).unwrap_or_default()
    } else {
        seed.guards.clone()
    };
    Lock {
        packs,
        rules,
        guards,
    }
}

fn lock(cfg: &Config, args: &Args) -> Result<u8, Fault> {
    let units = parse(&read(&cfg.abs_of(RULES))?)?;
    let seed: Seed = match args.get("seed") {
        Some(p) => serde_json::from_str(&read(Path::new(p))?)?,
        None => Seed::default(),
    };
    let old = read_lock(&cfg.abs_of(LOCK))?;
    let new = build(&units, old.as_ref(), &seed);
    // A guard names a rule in this note, or a rule elsewhere as `<Note>#<heading>` (`F:` lives in
    // Identity § Machine).
    let orphans: Vec<&str> = new
        .guards
        .iter()
        .filter(|g| !g.rule.contains('#') && !new.rules.iter().any(|r| r.id == g.rule))
        .map(|g| g.id.as_str())
        .collect();
    if !orphans.is_empty() {
        return Err(Fault::Usage(format!(
            "guards name no rule in {RULES}: {}",
            orphans.join(", ")
        )));
    }
    let changed = old
        .as_ref()
        .map_or_else(|| vec!["(first lock)".to_string()], |o| diff(&units, o));
    write_lock(cfg, &new)?;
    emit(&format!(
        "rules: locked {} units in {} packs, {} guards{}\n",
        new.rules.len(),
        new.packs.len(),
        new.guards.len(),
        if changed.is_empty() {
            String::new()
        } else {
            format!("; {}", changed.join(", "))
        }
    ));
    Ok(0)
}

/// The commit-time check (K40), run by `githooks/pre-commit` in whatever checkout is committing:
/// every guard's backstop against the staged names, and in the vault, the staged Hard-Rules
/// against the staged lock. No lock on this machine: one stderr line, and the commit goes on.
fn staged(cfg: &Config) -> Result<u8, Fault> {
    let lock = read_lock(&cfg.abs_of(LOCK))?;
    if lock.is_none() {
        eprintln!("rules: no {LOCK}; the guards' commit backstops were not checked");
    }
    let here = std::env::current_dir().map_err(|e| Fault::Io(".".into(), e))?;
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    let names = git(&["diff", "--cached", "--name-only", "--diff-filter=ACMR"]);
    let place = &crate::guard::place_of(&here);
    let mut code = 0;
    for g in lock.iter().flat_map(|l| {
        l.guards
            .iter()
            .filter(move |g| !g.staged.is_empty() && crate::guard::applies(l, g, place))
    }) {
        let hits: Vec<&str> = names
            .lines()
            .filter(|n| {
                let base = n.rsplit('/').next().unwrap_or(n).to_lowercase();
                g.staged
                    .iter()
                    .any(|p| crate::guard::wild(&p.to_lowercase(), &base))
            })
            .collect();
        if !hits.is_empty() {
            emit(&format!(
                "rules: commit refused by `{}` ({}): {}\n  staged: {}\n",
                g.id,
                g.rule,
                g.reason,
                hits.join(", ")
            ));
            code = 1;
        }
    }
    let top = git(&["rev-parse", "--show-toplevel"]);
    let in_vault = std::fs::canonicalize(top.trim()).ok() == std::fs::canonicalize(&cfg.vault).ok();
    // Every staged change counts here, deletions included: removing the lock is weakening it.
    if in_vault
        && git(&["diff", "--cached", "--name-only"])
            .lines()
            .any(|n| n == RULES || n == LOCK)
    {
        let lock: Lock = match git_show(cfg, &format!(":{LOCK}")) {
            Some(s) => serde_json::from_str(&s)?,
            None => {
                emit(&format!(
                    "rules: commit refused: the commit would leave {LOCK} out of the vault\n"
                ));
                return Ok(1);
            }
        };
        let text = git_show(cfg, &format!(":{RULES}")).unwrap_or_default();
        let d = diff(&parse(&text)?, &lock);
        if !d.is_empty() {
            emit(&format!(
                "rules: commit refused: the staged {RULES} and {LOCK} disagree: {}\n  The owner runs `knowledge rules lock`, then stages both.\n",
                few(&d)
            ));
            code = 1;
        }
    }
    Ok(code)
}

fn check(cfg: &Config, args: &Args) -> Result<u8, Fault> {
    if args.has("staged") {
        return staged(cfg);
    }
    let (text, lock) = (read(&cfg.abs_of(RULES))?, need_lock(cfg)?);
    let units = parse(&text)?;
    let d = diff(&units, &lock);
    if args.has("restore") {
        let mut code = imports(cfg, true);
        if !d.is_empty() {
            let head_ok = git_show(cfg, &format!("HEAD:{RULES}"))
                .and_then(|h| parse(&h).ok())
                .is_some_and(|u| diff(&u, &lock).is_empty());
            if head_ok && restore(cfg) {
                emit(&format!("rules: restored from HEAD: {}\n", few(&d)));
            } else {
                emit(&format!(
                    "!! RULES: {RULES} differs from {LOCK} and from HEAD ({}); the owner runs `knowledge rules lock` or reverts\n",
                    few(&d)
                ));
                code = 1;
            }
        }
        return Ok(code);
    }
    let mut code = imports(cfg, false);
    if d.is_empty() {
        emit(&format!("rules: {} units match {LOCK}\n", lock.rules.len()));
    } else {
        emit(&format!(
            "rules: {RULES} differs from {LOCK}: {}\n",
            few(&d)
        ));
        code = 1;
    }
    Ok(code)
}

/// The first five changes, then a count: a deleted file would otherwise name all 150 units.
fn few(d: &[String]) -> String {
    let more = if d.len() > 5 {
        format!(" and {} more", d.len() - 5)
    } else {
        String::new()
    };
    format!("{}{more}", d[..d.len().min(5)].join(", "))
}

/// The working copy back to `HEAD`'s, leaving the index alone.
fn restore(cfg: &Config) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(&cfg.vault)
        .args(["restore", "--source=HEAD", "--worktree", "--", RULES])
        .status()
        .is_ok_and(|s| s.success())
}

/// Claude Code's target: `~/.claude/CLAUDE.md` must import Identity and Hard-Rules. With `fix`, a
/// missing import is put back. A machine without that file is not a Claude Code machine: nothing to
/// check. Returns 1 when an import is missing and was not put back.
fn imports(cfg: &Config, fix: bool) -> u8 {
    let Some(path) = std::env::home_dir().map(|h| h.join(".claude").join("CLAUDE.md")) else {
        return 0;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return 0;
    };
    let has = |name: &str| {
        text.lines().any(|l| {
            let l = l.trim().replace('\\', "/");
            l.starts_with('@') && l.ends_with(&format!("/00-Meta/{name}"))
        })
    };
    let missing: Vec<&str> = PRIMED.into_iter().filter(|n| !has(n)).collect();
    if missing.is_empty() {
        return 0;
    }
    let root = cfg.vault.display().to_string().replace('\\', "/");
    let add: String = missing
        .iter()
        .map(|n| format!("@{root}/00-Meta/{n}\n"))
        .collect();
    if fix && append(&path, &text, &add) {
        emit(&format!(
            "rules: restored the import of {} in ~/.claude/CLAUDE.md\n",
            missing.join(" and ")
        ));
        return 0;
    }
    emit(&format!(
        "!! RULES: ~/.claude/CLAUDE.md does not import {}\n",
        missing.join(" and ")
    ));
    1
}

fn append(path: &PathBuf, text: &str, add: &str) -> bool {
    let sep = if text.ends_with('\n') { "" } else { "\n" };
    std::fs::write(path, format!("{text}{sep}{add}")).is_ok()
}

fn list(cfg: &Config, args: &Args) -> Result<u8, Fault> {
    let lock = need_lock(cfg)?;
    if args.has("json") {
        emit(&(serde_json::to_string_pretty(&lock)? + "\n"));
        return Ok(0);
    }
    let mut out = String::new();
    for p in &lock.packs {
        let rules: Vec<&Rule> = lock
            .rules
            .iter()
            .filter(|r| r.pack == p.id && r.kind != "preamble")
            .collect();
        if rules.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "{}{} ({})\n",
            p.id,
            if p.enabled { "" } else { " [off]" },
            rules.len()
        ));
        for r in rules {
            out.push_str(&format!(
                "  {:<9} {:<10} {}\n",
                r.kind,
                r.enforce.join("·"),
                r.id
            ));
        }
    }
    out.push_str(&format!(
        "{} guards: {}\n",
        lock.guards.len(),
        lock.guards
            .iter()
            .map(|g| g.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    emit(&out);
    Ok(0)
}

fn toggle(cfg: &Config, args: &Args, on: bool) -> Result<u8, Fault> {
    let name = args
        .words
        .get(1)
        .ok_or_else(|| Fault::Usage(USAGE.into()))?;
    let mut lock = need_lock(cfg)?;
    let pack = lock
        .packs
        .iter_mut()
        .find(|p| &p.id == name)
        .ok_or_else(|| Fault::Usage(format!("no pack `{name}` in {LOCK}")))?;
    pack.enabled = on;
    write_lock(cfg, &lock)?;
    emit(&format!(
        "rules: pack `{name}` {}\n",
        if on { "enabled" } else { "disabled" }
    ));
    Ok(0)
}
