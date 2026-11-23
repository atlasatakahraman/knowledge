//! The link graph (D11): a note's neighbours, and the shortest path between two notes, from the
//! `links` table the index fills. graphify answered this from a 54 s rebuild; this is a query.
use crate::config::{self, Config};
use crate::index;
use crate::{Args, Fault, emit};
use rusqlite::Connection;
use std::collections::HashMap;

/// How far `graph <a> <b>` looks. Past four hops a "connection" is through a hub and says nothing.
const DEPTH: i64 = 4;
/// Neighbours listed per direction before the rest is counted.
const SHOWN: usize = 40;

/// A note by name, or by vault-relative path when it has a `/` or ends in `.md`. A name two notes
/// share resolves to the shorter path, as a link would from the vault root.
fn resolve(conn: &Connection, cfg: &Config, arg: &str) -> Result<(i64, String), Fault> {
    let found = if arg.contains('/') || arg.contains('\\') || arg.ends_with(".md") {
        let rel = cfg
            .rel_of(std::path::Path::new(arg))
            .unwrap_or_else(|| arg.replace('\\', "/"));
        conn.query_row("SELECT id, path FROM notes WHERE path = ?1", [&rel], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok()
    } else {
        index::pick("", &index::notes_named(conn, arg)?).cloned()
    };
    found.ok_or_else(|| Fault::Usage(format!("no note named {arg}")))
}

fn paths(conn: &Connection, sql: &str, id: i64) -> Result<Vec<String>, Fault> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([id], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn outgoing(conn: &Connection, id: i64) -> Result<Vec<String>, Fault> {
    paths(
        conn,
        "SELECT n.path FROM links l JOIN notes n ON n.id = l.dst WHERE l.src = ?1 ORDER BY n.path",
        id,
    )
}

fn incoming(conn: &Connection, id: i64) -> Result<Vec<String>, Fault> {
    paths(
        conn,
        "SELECT n.path FROM links l JOIN notes n ON n.id = l.src WHERE l.dst = ?1 ORDER BY n.path",
        id,
    )
}

/// Hops from `from` to every note within DEPTH, links read both ways. A recursive query with UNION
/// (not UNION ALL) keeps one row per note and distance, so a hub linked from hundreds of notes is
/// expanded once per depth instead of once per path through it.
fn distances(conn: &Connection, from: i64) -> Result<HashMap<i64, i64>, Fault> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE reach(id, d) AS (
           SELECT ?1, 0
           UNION SELECT l.dst, reach.d + 1 FROM reach JOIN links l ON l.src = reach.id WHERE reach.d < ?2
           UNION SELECT l.src, reach.d + 1 FROM reach JOIN links l ON l.dst = reach.id WHERE reach.d < ?2
         )
         SELECT id, min(d) FROM reach GROUP BY id",
    )?;
    let rows = stmt.query_map([from, DEPTH], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The shortest path from `a` to `b`, walked back from `b` one hop closer each step.
fn shortest(conn: &Connection, a: i64, b: i64) -> Result<Option<Vec<i64>>, Fault> {
    let dist = distances(conn, a)?;
    let Some(&d) = dist.get(&b) else {
        return Ok(None);
    };
    let mut stmt = conn.prepare("SELECT dst FROM links WHERE src = ?1 UNION SELECT src FROM links WHERE dst = ?1 ORDER BY 1")?;
    let mut path = vec![b];
    let mut at = b;
    for step in (0..d).rev() {
        let next: Vec<i64> = stmt
            .query_map([at], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        at = *next
            .iter()
            .find(|n| dist.get(n) == Some(&step))
            .ok_or_else(|| Fault::Config("the link graph changed mid-query".into()))?;
        path.push(at);
    }
    path.reverse();
    Ok(Some(path))
}

fn path_of(conn: &Connection, id: i64) -> Result<String, Fault> {
    Ok(conn.query_row("SELECT path FROM notes WHERE id = ?1", [id], |r| r.get(0))?)
}

fn list(label: &str, items: &[String]) -> String {
    let names: Vec<&str> = items
        .iter()
        .take(SHOWN)
        .map(|p| crate::vault::name_of(p))
        .collect();
    let more = items.len().saturating_sub(SHOWN);
    let tail = if more > 0 {
        format!(", and {more} more")
    } else {
        String::new()
    };
    format!("{label} ({}): {}{tail}\n", items.len(), names.join(", "))
}

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &[])?;
    let cfg = config::load()?;
    let conn = index::open_ro(&cfg)?;
    match args.words.as_slice() {
        // The note itself, for a harness with no shell to open it (the MCP `read` tool). Only an
        // indexed path is read, so a name cannot reach a file outside the vault.
        [note] if args.has("text") => {
            let (_, path) = resolve(&conn, &cfg, note)?;
            let file = cfg.abs_of(&path);
            emit(&std::fs::read_to_string(&file).map_err(|e| Fault::io(&file, e))?);
        }
        [note] => {
            let (id, path) = resolve(&conn, &cfg, note)?;
            let (out, inc) = (outgoing(&conn, id)?, incoming(&conn, id)?);
            if args.has("json") {
                emit(&format!(
                    "{}\n",
                    serde_json::json!({"note": path, "path": cfg.abs_of(&path), "out": out, "in": inc})
                ));
            } else {
                emit(&format!(
                    "{} — {}\n{}{}",
                    crate::vault::name_of(&path),
                    cfg.abs_of(&path).display(),
                    list("links to", &out),
                    list("linked from", &inc)
                ));
            }
        }
        [a, b] => {
            let ((ia, _), (ib, _)) = (resolve(&conn, &cfg, a)?, resolve(&conn, &cfg, b)?);
            let hops = match shortest(&conn, ia, ib)? {
                Some(ids) => ids
                    .into_iter()
                    .map(|id| path_of(&conn, id))
                    .collect::<Result<Vec<_>, _>>()?,
                None => Vec::new(),
            };
            if args.has("json") {
                emit(&format!(
                    "{}\n",
                    serde_json::json!({"path": hops, "depth": DEPTH})
                ));
            } else if hops.is_empty() {
                emit(&format!("no path within {DEPTH} links\n"));
            } else {
                let names: Vec<&str> = hops.iter().map(|p| crate::vault::name_of(p)).collect();
                emit(&format!("{}\n", names.join(" → ")));
            }
        }
        _ => {
            return Err(Fault::Usage(
                "knowledge graph <note> [<other note>] [--json] | <note> --text".into(),
            ));
        }
    }
    Ok(0)
}
