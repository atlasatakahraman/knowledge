//! `knowledge doctor --only portable`: the traps that break a vault cloned onto the other boot
//! (D16). The rest of the doctor is bun's until Stage 6; brain-doctor runs this as its `portable` check.
use crate::config;
use crate::vault;
use crate::{Args, Fault, emit};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];
/// Windows' MAX_PATH is 260 for the whole path; 240 for the vault-relative part leaves the root room.
const MAX_REL: usize = 240;

/// What is wrong with one path component, if anything.
fn component(c: &str) -> Option<&'static str> {
    let stem = c.split('.').next().unwrap_or(c);
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        Some("name Windows reserves")
    } else if c
        .chars()
        .any(|ch| "<>:\"|?*".contains(ch) || ch.is_control())
    {
        Some("character Windows forbids")
    } else if c.ends_with(['.', ' ']) {
        Some("trailing dot or space")
    } else if !vault::looks_nfc(c) {
        Some("name not in NFC")
    } else {
        None
    }
}

/// A value written as a machine path: a backslash, a drive letter, `~/`, or an absolute POSIX path.
fn machine_path(v: &str) -> bool {
    let b = v.as_bytes();
    v.contains('\\')
        || v.starts_with("~/")
        || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/')
        || (v.starts_with('/') && v[1..].contains('/'))
}

/// `.gitattributes` pins LF for notes: `* ... eol=lf` or `*.md ... eol=lf`.
fn lf_pinned(text: &str) -> bool {
    text.lines().any(|l| {
        let mut parts = l.split_whitespace();
        matches!(parts.next(), Some("*" | "*.md")) && parts.any(|a| a == "eol=lf")
    })
}

pub fn portable(cfg: &config::Config) -> Result<(usize, Vec<(String, String)>), Fault> {
    let files = vault::walk(&cfg.vault)?;
    let mut found: Vec<(String, String)> = Vec::new();
    let mut say = |kind: &str, path: &str| found.push((kind.to_string(), path.to_string()));
    let mut by_lower: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut seen_dirs = BTreeSet::new();
    for f in &files {
        let parts: Vec<&str> = f.rel.split('/').collect();
        for (i, c) in parts.iter().enumerate() {
            let prefix = parts[..=i].join("/");
            if i + 1 < parts.len() && !seen_dirs.insert(prefix.clone()) {
                continue; // a folder is checked once, not once per file in it
            }
            if let Some(kind) = component(c) {
                say(kind, &prefix);
            }
            by_lower
                .entry(prefix.to_lowercase())
                .or_default()
                .insert(prefix);
        }
        if f.rel.chars().count() > MAX_REL {
            say("path over 240 chars", &f.rel);
        }
        if !f.is_note() {
            continue;
        }
        let Ok(text) = fs::read_to_string(cfg.abs_of(&f.rel)) else {
            continue;
        };
        let text = text.replace("\r\n", "\n");
        if let Some(block) = vault::split_front(&text).0
            // Aliases and tags are names a note is found by, not paths: "C:\dev migration" is a search word.
            && vault::fields(block)
                .iter()
                .filter(|(k, _)| k != "aliases" && k != "tags")
                .flat_map(|(_, v)| v)
                .any(|v| machine_path(v))
        {
            say("machine path in frontmatter", &f.rel);
        }
        if vault::links_of(&text).iter().any(|l| machine_path(&l.raw)) {
            say("machine path in a link", &f.rel);
        }
    }
    for names in by_lower.values().filter(|n| n.len() > 1) {
        say(
            "names differ only by case",
            &names.iter().cloned().collect::<Vec<_>>().join(" | "),
        );
    }
    let attrs = cfg.vault.join(".gitattributes");
    if !fs::read_to_string(&attrs).is_ok_and(|t| lf_pinned(&t)) {
        say(".gitattributes does not pin LF", ".gitattributes");
    }
    Ok((files.len(), found))
}

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    match raw.first().map(String::as_str) {
        Some("budget") => return crate::budget::run(raw),
        Some("profile") => return crate::budget::profile(raw),
        _ => {}
    }
    let args = Args::parse(raw, &["only"])?;
    // `--only portable` alone keeps its own line and exit code: checks/stage-3.sh and the README
    // use it that way. Everything else is the full doctor (Stage 6).
    if args.get("only") != Some("portable") {
        return crate::health::run(&args);
    }
    let cfg = config::load()?;
    let (files, found) = portable(&cfg)?;
    if args.has("json") {
        let items: Vec<_> = found
            .iter()
            .map(|(k, p)| serde_json::json!({"kind": k, "path": p}))
            .collect();
        emit(&format!(
            "{}\n",
            serde_json::json!({"check": "portable", "ok": found.is_empty(), "files": files, "findings": items})
        ));
    } else if found.is_empty() {
        emit(&format!("portable: PASS ({files} files)\n"));
    } else {
        let mut out = format!(
            "portable: FAIL ({} findings in {files} files)\n",
            found.len()
        );
        for (k, p) in &found {
            out.push_str(&format!("  {k}: {p}\n"));
        }
        emit(&out);
    }
    Ok(u8::from(!found.is_empty()))
}
