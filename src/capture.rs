//! Captures (Stage 4, D7): what `keep` and `fetch` store, in KNOWLEDGE_HOME/captures.db, apart from
//! the vault index. Rows older than 14 days are deleted whenever the file is opened for a write, so
//! no sweeper runs. A capture is never injected: it is reached by `keep --q`, `fetch --q` and
//! `find --captures`.
use crate::config::{self, Config};
use crate::vault::{self, SLOTS};
use crate::{Args, Fault, emit, index, search};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i32 = 1;
const SCHEMA: &str = "
CREATE TABLE captures (id INTEGER PRIMARY KEY, label TEXT NOT NULL UNIQUE, size INTEGER NOT NULL,
                       created INTEGER NOT NULL);
CREATE INDEX captures_created ON captures (created);
CREATE VIRTUAL TABLE chunks USING fts5(title, body, capture UNINDEXED, tokenize = 'porter unicode61');
PRAGMA user_version = 1;";

const TTL_SECS: i64 = 14 * 24 * 3600;
/// What one call prints at most, in chars (spec D7's proposal).
pub const BUDGET: usize = 2000;
/// Room kept for the closing lines when sections are printed.
const RESERVE: usize = 220;
/// The titles line's length when no query was given.
const TITLES_NO_QUERY: usize = 500;

fn db_path(cfg: &Config) -> PathBuf {
    cfg.home.join("captures.db")
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn delete_capture(conn: &Connection, id: i64) -> Result<(), Fault> {
    let base = id * SLOTS as i64;
    conn.execute(
        "DELETE FROM chunks WHERE rowid >= ?1 AND rowid < ?2",
        [base, base + SLOTS as i64],
    )?;
    conn.execute("DELETE FROM captures WHERE id = ?1", [id])?;
    Ok(())
}

/// Open for a write, deleting every capture past its 14 days first.
fn open_rw(cfg: &Config) -> Result<Connection, Fault> {
    let mut conn = index::open_db(&db_path(cfg), SCHEMA, SCHEMA_VERSION)?;
    let tx = conn.transaction()?;
    let old: Vec<i64> = tx
        .prepare("SELECT id FROM captures WHERE created < ?1")?
        .query_map([now() - TTL_SECS], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for id in old {
        delete_capture(&tx, id)?;
    }
    tx.commit()?;
    Ok(conn)
}

/// A section: its title (the heading, or the line range it covers) and its text.
pub struct Section {
    pub title: String,
    pub body: String,
    /// Under a heading: its title says something; a bare line range does not.
    pub named: bool,
}

/// The text split at headings outside fenced code, then into pieces of at most CHUNK_MAX chars,
/// cut at a blank line once a piece is half full, else at any line; a line longer than CHUNK_MAX
/// is cut by chars. A section with no heading is named by its lines, `L12-40`, so a title never
/// repeats the text it stands for.
pub fn sections(text: &str) -> Vec<Section> {
    let max = vault::CHUNK_MAX;
    // (heading, first line, last line, text)
    let mut parts: Vec<(String, usize, usize, String)> = Vec::new();
    let mut heading = String::new();
    let mut fence = false;
    let push = |parts: &mut Vec<(String, usize, usize, String)>,
                heading: &str,
                n: usize,
                line: &str,
                cut: bool| {
        match parts.last_mut() {
            Some(p) if !cut => {
                p.2 = n;
                p.3.push_str(line);
                p.3.push('\n');
            }
            _ => parts.push((heading.to_string(), n, n, format!("{line}\n"))),
        }
    };
    let mut cut_next = true;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.trim_start().starts_with("```") {
            fence = !fence;
        }
        if !fence && let Some(h) = vault::heading_line(line) {
            heading = h.to_string();
            cut_next = true;
            continue;
        }
        let mut rest = line;
        while rest.len() > max {
            let at = (0..=max)
                .rev()
                .find(|&k| rest.is_char_boundary(k))
                .unwrap_or(0);
            push(&mut parts, &heading, n, &rest[..at], true);
            rest = &rest[at..];
            cut_next = true;
        }
        let cur = parts.last().map_or(0, |p| p.3.len());
        let cut =
            cut_next || cur + rest.len() + 1 > max || (rest.trim().is_empty() && cur > max / 2);
        if cut && rest.trim().is_empty() {
            cut_next = true; // a blank line opens nothing
            continue;
        }
        push(&mut parts, &heading, n, rest, cut);
        cut_next = false;
    }
    let mut pieces: HashMap<&str, usize> = HashMap::new();
    for p in &parts {
        *pieces.entry(p.0.as_str()).or_default() += 1;
    }
    let mut out: Vec<Section> = parts
        .iter()
        .filter(|p| !p.3.trim().is_empty())
        .map(|(h, a, b, t)| Section {
            title: match (h.is_empty(), pieces[h.as_str()] > 1) {
                (true, _) => format!("L{a}-{b}"),
                (false, true) => format!("{h} L{a}-{b}"),
                (false, false) => h.clone(),
            },
            body: t.clone(),
            named: !h.is_empty(),
        })
        .collect();
    if out.len() > SLOTS {
        let tail: String = out
            .drain(SLOTS - 1..)
            .map(|s| format!("{}\n{}", s.title, s.body))
            .collect();
        out.push(Section {
            title: "rest".into(),
            body: tail,
            named: false,
        });
    }
    out
}

/// Store `text` under `label`, replacing a capture of that label. Returns its id and sections.
pub fn store(cfg: &Config, label: &str, text: &str) -> Result<(i64, Vec<Section>), Fault> {
    let secs = sections(text);
    let mut conn = open_rw(cfg)?;
    let tx = conn.transaction()?;
    if let Some(old) = tx
        .query_row("SELECT id FROM captures WHERE label = ?1", [label], |r| {
            r.get::<_, i64>(0)
        })
        .optional()?
    {
        delete_capture(&tx, old)?;
    }
    let id: i64 = tx.query_row(
        "INSERT INTO captures (label, size, created) VALUES (?1, ?2, ?3) RETURNING id",
        params![label, text.len() as i64, now()],
        |r| r.get(0),
    )?;
    let base = id * SLOTS as i64;
    {
        let mut ins =
            tx.prepare("INSERT INTO chunks (rowid, title, body, capture) VALUES (?1, ?2, ?3, ?4)")?;
        for (n, s) in secs.iter().enumerate() {
            ins.execute(params![base + n as i64, s.title, s.body, id])?;
        }
    }
    tx.commit()?;
    Ok((id, secs))
}

/// `"a" OR "b"`: a query's words, quoted, so FTS5 syntax in them is inert. None when no word is left.
fn matcher(q: &str) -> Option<String> {
    let toks = search::tokens(q);
    (!toks.is_empty()).then(|| {
        toks.iter()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}

fn size_of(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / 1048576.0)
    }
}

/// What `keep` and `fetch` print: one header line, each section a query matched printed once (the
/// queries take turns, best section first), the other titles on one line, one overflow line. With
/// no query, a text that fits the budget is printed as it is; a larger one prints its titles.
pub fn print(
    cfg: &Config,
    id: i64,
    label: &str,
    text: &str,
    secs: &[Section],
    queries: &[String],
    budget: usize,
) -> Result<String, Fault> {
    if queries.is_empty() && text.len() <= budget {
        return Ok(text.to_string());
    }
    let conn = index::open_existing(&db_path(cfg), SCHEMA_VERSION, "knowledge keep")?;
    let base = id * SLOTS as i64;
    let mut ranked: Vec<Vec<usize>> = Vec::new();
    for q in queries {
        let Some(m) = matcher(q) else { continue };
        let mut stmt = conn.prepare(
            "SELECT rowid FROM chunks WHERE chunks MATCH ?1 AND rowid >= ?2 AND rowid < ?3
             ORDER BY bm25(chunks, 2.0, 1.0) LIMIT 8",
        )?;
        let rows = stmt
            .query_map(params![m, base, base + SLOTS as i64], |r| {
                r.get::<_, i64>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        ranked.push(rows.into_iter().map(|r| (r - base) as usize).collect());
    }
    let mut order = Vec::new();
    let mut seen = HashSet::new();
    for round in 0..ranked.iter().map(Vec::len).max().unwrap_or(0) {
        for list in &ranked {
            if let Some(&n) = list.get(round)
                && n < secs.len()
                && seen.insert(n)
            {
                order.push(n);
            }
        }
    }
    let mut out = format!(
        "{label} — {}, {} sections\n",
        size_of(text.len()),
        secs.len()
    );
    let mut printed = HashSet::new();
    let mut skipped = 0;
    for &n in &order {
        let s = &secs[n];
        let block = format!("## {}\n{}\n", s.title, s.body.trim_end());
        if out.len() + block.len() + RESERVE > budget {
            skipped += 1;
            continue;
        }
        out.push_str(&block);
        printed.insert(n);
    }
    if queries.iter().any(|q| matcher(q).is_some()) && order.is_empty() {
        out.push_str("No section matched.\n");
    }
    let hidden = secs.len() - printed.len();
    let others: Vec<&str> = (0..secs.len())
        .filter(|n| !printed.contains(n) && secs[*n].named)
        .map(|n| secs[n].title.as_str())
        .collect();
    let tail = format!(
        "{} sections not shown{}. Ask again: knowledge find \"<q>\" --captures\n",
        hidden,
        if skipped > 0 {
            format!(", {skipped} of them matched")
        } else {
            String::new()
        }
    );
    if hidden > 0 {
        // With no query the titles are the whole answer, and on a docs page they filled the budget
        // (1,816 of 1,970 chars): a batch of 20 pages then cost more than context-mode did (gate 1).
        // The first ~500 chars say what is there; the rest is one query away.
        let cap = if queries.is_empty() {
            TITLES_NO_QUERY
        } else {
            budget
        };
        let room = budget.saturating_sub(out.len() + tail.len() + 10).min(cap);
        let mut line = String::from("Other: ");
        for (i, t) in others.iter().enumerate() {
            let sep = if i == 0 { "" } else { " · " };
            if line.len() + sep.len() + t.len() + 2 > room {
                line.push_str(" …");
                break;
            }
            line.push_str(sep);
            line.push_str(t);
        }
        if line.len() > "Other: ".len() {
            out.push_str(&line);
            out.push('\n');
        }
        out.push_str(&tail);
    }
    Ok(out)
}

fn queries(raw: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = raw.iter();
    while let Some(a) = it.next() {
        if a == "--q"
            && let Some(v) = it.next()
        {
            out.push(v.clone());
        }
    }
    out
}

/// `--q` may repeat, which Args keeps once; the rest of the line goes through Args.
pub fn split_args(raw: &[String], valued: &[&str]) -> Result<(Args, Vec<String>), Fault> {
    let mut rest = Vec::new();
    let mut it = raw.iter();
    while let Some(a) = it.next() {
        if a == "--q" {
            it.next();
        } else {
            rest.push(a.clone());
        }
    }
    Ok((Args::parse(&rest, valued)?, queries(raw)))
}

/// `knowledge keep "<label>" [--q "<q>"]... [--budget N]`: stdin is stored and answered within the budget.
pub fn run_keep(raw: &[String]) -> Result<u8, Fault> {
    let (args, queries) = split_args(raw, &["budget"])?;
    let label = args.words.join(" ");
    if label.trim().is_empty() {
        return Err(Fault::Usage(
            "knowledge keep \"<label>\" [--q \"<q>\"]... [--budget N] < input".into(),
        ));
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|e| Fault::Io("stdin".into(), e))?;
    let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
    let cfg = config::load()?;
    let label = format!("keep:{}", label.trim());
    let (id, secs) = store(&cfg, &label, &text)?;
    let budget = args.num("budget", BUDGET)?.max(200);
    emit(&print(&cfg, id, &label, &text, &secs, &queries, budget)?);
    Ok(0)
}

/// `find "<q>" --captures`: the best section of each capture a query matches, best first, within
/// the budget.
pub fn find(query: &str, limit: usize, budget: usize) -> Result<String, Fault> {
    let cfg = config::load()?;
    let path = db_path(&cfg);
    let Some(m) = matcher(query) else {
        return Ok(String::new());
    };
    if !path.exists() {
        return Ok(String::new());
    }
    let conn = index::open_existing(&path, SCHEMA_VERSION, "knowledge keep")?;
    let mut stmt = conn.prepare(
        "SELECT captures.label, chunks.title, chunks.body
         FROM chunks JOIN captures ON captures.id = chunks.capture
         WHERE chunks MATCH ?1 ORDER BY bm25(chunks, 2.0, 1.0) LIMIT 64",
    )?;
    let rows = stmt
        .query_map([m], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = String::new();
    let mut seen = HashSet::new();
    for (label, title, body) in rows {
        if seen.len() >= limit || !seen.insert(label.clone()) {
            continue;
        }
        let mut block = format!("{label} § {title}\n{}\n", body.trim_end());
        if out.len() + block.len() > budget {
            let room = budget.saturating_sub(out.len());
            if room < 120 {
                break;
            }
            let at = (0..=room - "…\n".len())
                .rev()
                .find(|&k| block.is_char_boundary(k))
                .unwrap_or(0);
            block.truncate(at);
            block.push_str("…\n");
        }
        out.push_str(&block);
    }
    Ok(out)
}
