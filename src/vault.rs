//! The vault on disk: walking it, and reading one note's frontmatter, links, headings and chunks.
//! Written in the crate, not pulled in (K30): the rules were ported from brain-lib.mjs, so a note,
//! a link and a heading mean what they meant to the bun hooks before them.
use crate::Fault;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

/// Directories never indexed and never counted as notes. brain-lib's INDEX_EXCLUDES, same list.
pub const EXCLUDES: [&str; 9] = [
    "node_modules",
    "out",
    "target",
    ".git",
    "graphify-out",
    ".obsidian",
    ".remember",
    ".claude",
    ".brain",
];

/// One file under the vault, by its stored form: vault-relative, forward slashes.
pub struct File {
    pub rel: String,
    pub size: i64,
    pub mtime: i64,
}

impl File {
    pub fn is_note(&self) -> bool {
        self.rel.ends_with(".md")
    }
}

/// Every file under `root`, skipping EXCLUDES, sorted by path. An entry that vanishes mid-walk is
/// passed over; an unreadable folder is a fault.
pub fn walk(root: &Path) -> Result<Vec<File>, Fault> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| Fault::io(&dir, e))? {
            let Ok(entry) = entry else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            if EXCLUDES.contains(&name.as_str()) {
                continue;
            }
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push((entry.path(), rel));
            } else if let Ok(meta) = entry.metadata() {
                out.push(File {
                    rel,
                    size: meta.len() as i64,
                    mtime: mtime(&meta),
                });
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

pub fn mtime(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i64)
}

/// A note the index holds: a `.md` file outside every EXCLUDES folder.
pub fn indexable(rel: &str) -> bool {
    rel.ends_with(".md") && !rel.split('/').any(|c| EXCLUDES.contains(&c))
}

/// `20-Knowledge/Rust/Ownership.md` -> `Ownership`.
pub fn name_of(rel: &str) -> &str {
    let base = rel.rsplit('/').next().unwrap_or(rel);
    base.strip_suffix(".md").unwrap_or(base)
}

/// `20-Knowledge/Rust/Ownership.md` -> `20-Knowledge/Rust`; a note at the root -> "".
pub fn dir_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(d, _)| d)
}

/// Files whose top lines a tool or a reader consumes verbatim: no frontmatter, not checked.
pub fn exempt(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".md") else {
        return false;
    };
    stem.starts_with("license") || matches!(stem, "claude" | "agents" | "readme")
}

/// FNV-1a, 64 bit: change detection, not security. Size and mtime are compared first; this
/// decides whether a touched file really changed.
pub fn hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A name in NFC carries no combining mark. ponytail: without Unicode tables (no extra crate) this
/// flags a combining mark instead of composing the name; a decomposed name (macOS, some pastes)
/// always has one, a precomposed name never does. A mark with no precomposed form is reported too,
/// which is a false alarm the owner can see and dismiss.
pub fn looks_nfc(s: &str) -> bool {
    !s.chars().any(|c| {
        matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F | 0x3099..=0x309A | 0x1160..=0x11FF)
    })
}

/// The frontmatter block and the body after it, as brain-lib reads them: `---` on the first line,
/// closed by the next line that starts with `---`. Text is LF here (callers drop `\r`).
pub fn split_front(text: &str) -> (Option<&str>, &str) {
    if let Some(rest) = text.strip_prefix("---\n")
        && let Some(k) = rest.find("\n---")
    {
        let after = &rest[k + 4..];
        let body = after.find('\n').map_or("", |i| &after[i + 1..]);
        return (Some(&rest[..k]), body);
    }
    (None, text)
}

/// brain-lib's frontmatterOk: a `type:` and a `created:` line, each followed by a value.
pub fn front_ok(block: &str) -> bool {
    let has = |key: &str| {
        let mut off = 0;
        for line in block.split('\n') {
            if line.starts_with(key) && !block[off + key.len()..].trim_start().is_empty() {
                return true;
            }
            off += line.len() + 1;
        }
        false
    };
    has("type:") && has("created:")
}

/// `key: value`, `key: [a, b]` and `key:` followed by `- item` lines. Quotes are dropped.
pub fn fields(block: &str) -> Vec<(String, Vec<String>)> {
    let unquote = |s: &str| s.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in block.lines() {
        let item = line.trim_start().strip_prefix("- ");
        if let (Some(item), true, Some(last)) =
            (item, line.starts_with([' ', '\t', '-']), out.last_mut())
        {
            last.1.push(unquote(item));
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.is_empty() || key.starts_with([' ', '\t']) {
            continue;
        }
        let value = value.trim();
        let values = match value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            Some(list) => list
                .split(',')
                .map(unquote)
                .filter(|v| !v.is_empty())
                .collect(),
            None if value.is_empty() => Vec::new(),
            None => vec![unquote(value)],
        };
        out.push((key.to_string(), values));
    }
    out
}

pub fn field<'a>(fields: &'a [(String, Vec<String>)], key: &str) -> &'a [String] {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map_or(&[], |(_, v)| v.as_slice())
}

/// Text with fenced code blocks removed: a link or a heading there is an example.
fn strip_fenced(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("```") {
        let Some(j) = rest[i + 3..].find("```") else {
            break;
        };
        out.push_str(&rest[..i]);
        rest = &rest[i + 3 + j + 3..];
    }
    out.push_str(rest);
    out
}

/// Fenced blocks, then `inline code` (closed on the same line), removed.
pub fn strip_code(text: &str) -> String {
    let fenced = strip_fenced(text);
    let mut out = String::with_capacity(fenced.len());
    let mut rest = fenced.as_str();
    while let Some(i) = rest.find('`') {
        let after = &rest[i + 1..];
        let line = &after[..after.find('\n').unwrap_or(after.len())];
        match line.find('`') {
            Some(j) => {
                out.push_str(&rest[..i]);
                rest = &after[j + 1..];
            }
            None => {
                out.push_str(&rest[..=i]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub struct Link {
    /// What was written: `folder/Note` in `[[folder/Note#H|alias]]`.
    pub raw: String,
    /// The note name it resolves by: `Note`.
    pub name: String,
    /// `H`, when the link names a heading (not a nested `#A#B`, not a block ref `#^id`).
    pub heading: Option<String>,
}

/// Every `[[link]]` outside code, closed on its own line as Obsidian requires. brain-lib's
/// linksOf; an embed `![[x]]` counts, as it does there.
pub fn links_of(text: &str) -> Vec<Link> {
    let s = strip_code(text);
    let b = s.as_bytes();
    let stop = |from: usize, set: &[u8]| {
        from + b[from..]
            .iter()
            .position(|c| set.contains(c))
            .unwrap_or(b.len() - from)
    };
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(k) = s[i..].find("[[") {
        let start = i + k + 2;
        let name_end = stop(start, b"]|#\n");
        let end = stop(name_end, b"]\n");
        if name_end == start || !b[end..].starts_with(b"]]") {
            i = i + k + 1;
            continue;
        }
        let raw = s[start..name_end].trim();
        let heading = (b[name_end] == b'#')
            .then(|| s[name_end + 1..end].split('|').next().unwrap_or(""))
            .filter(|h| !h.is_empty() && !h.contains(['#', '^']))
            .map(|h| h.trim().to_string());
        out.push(Link {
            raw: raw.to_string(),
            name: raw.rsplit(['/', '\\']).next().unwrap_or(raw).to_string(),
            heading,
        });
        i = end + 2;
    }
    out
}

/// `## Text` -> `Text`: one to six `#`, whitespace, then something.
pub fn heading_line(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|&c| c == b'#').count();
    let rest = line.get(hashes..)?;
    let h = rest.trim();
    ((1..=6).contains(&hashes) && rest.starts_with(char::is_whitespace) && !h.is_empty())
        .then_some(h)
}

/// Every heading outside fenced code, lower-cased: what `[[Note#Heading]]` is checked against.
pub fn headings_of(text: &str) -> HashSet<String> {
    strip_fenced(text)
        .lines()
        .filter_map(heading_line)
        .map(str::to_lowercase)
        .collect()
}

/// A chunk's rowid is `note id * SLOTS + n`, so a note's chunks are deleted by a rowid range
/// instead of a scan of the unindexed `note` column. A note past SLOTS chunks keeps the rest in
/// its last one.
pub const SLOTS: usize = 4096;

/// The body split at headings outside fenced code: (heading, text). Text before the first heading
/// is a chunk with an empty heading. A note with no text still gets one chunk, so its title is found.
pub fn chunks(body: &str) -> Vec<(String, String)> {
    let mut out = vec![(String::new(), String::new())];
    let mut fence = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") {
            fence = !fence;
        }
        if !fence && let Some(h) = heading_line(line) {
            out.push((h.to_string(), String::new()));
            continue;
        }
        if let Some((_, text)) = out.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    out.retain(|(h, t)| !h.is_empty() || !t.trim().is_empty());
    let mut out: Vec<(String, String)> = out
        .into_iter()
        .flat_map(|(h, t)| split_long(&t).into_iter().map(move |p| (h.clone(), p)))
        .collect();
    if out.len() > SLOTS {
        let tail: String = out
            .drain(SLOTS - 1..)
            .map(|(h, t)| format!("{h}\n{t}"))
            .collect();
        out.push((String::new(), tail));
    }
    if out.is_empty() {
        out.push((String::new(), String::new()));
    }
    out
}

/// A section's text, in pieces of at most CHUNK_MAX chars, cut where a paragraph or a bullet
/// starts. bm25 normalises by length, so one 20 KB section (Hard-Rules' Working preferences) lost
/// to every short note that shared a word; a piece that size also fits the passage rule (G16).
/// A single line longer than CHUNK_MAX stays whole.
pub const CHUNK_MAX: usize = 1200;

fn split_long(text: &str) -> Vec<String> {
    if text.len() <= CHUNK_MAX {
        return vec![text.to_string()];
    }
    let mut out = vec![String::new()];
    let mut fence = false;
    for line in text.lines() {
        let boundary = !fence
            && (line.trim().is_empty()
                || line.starts_with("- ")
                || line.starts_with("* ")
                || line.starts_with("| "));
        if line.trim_start().starts_with("```") {
            fence = !fence;
        }
        let cur = out.last().map_or(0, String::len);
        if boundary && cur > 0 && cur + line.len() + 1 > CHUNK_MAX {
            out.push(String::new());
        }
        if let Some(piece) = out.last_mut() {
            piece.push_str(line);
            piece.push('\n');
        }
    }
    out.retain(|p| !p.trim().is_empty());
    out
}

/// Intentional forward links, lower-cased: each bullet's leading `` `[[name ``.
pub fn forward_links(vault: &Path) -> HashSet<String> {
    let Ok(text) = fs::read_to_string(vault.join(crate::config::META).join("Forward-Links.md"))
    else {
        return HashSet::new();
    };
    text.lines()
        .filter_map(|l| {
            let r = l.strip_prefix('-')?;
            let r = r
                .starts_with(char::is_whitespace)
                .then(|| r.trim_start())?
                .strip_prefix("`[[")?;
            let n = r[..r.find([']', '|', '#']).unwrap_or(r.len())].trim();
            (!n.is_empty()).then(|| n.to_lowercase())
        })
        .collect()
}
