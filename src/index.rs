//! The index: one SQLite file per machine, KNOWLEDGE_HOME/vault-index.db (K7), rebuildable, never
//! in git. Notes, their chunks in FTS5, the names each note links to, and the link graph (D11).
use crate::config::{self, Config};
use crate::vault::{self, File, SLOTS};
use crate::{Args, Fault, emit};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Bumped when the schema changes; an index from another version is rebuilt, not migrated: every
/// row is derived from files.
const SCHEMA_VERSION: i32 = 1;

const SCHEMA: &str = "
CREATE TABLE notes (id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                    hash TEXT NOT NULL, kind TEXT NOT NULL, title TEXT NOT NULL,
                    size INTEGER NOT NULL, mtime INTEGER NOT NULL);
CREATE INDEX notes_name ON notes (name);
CREATE VIRTUAL TABLE chunks USING fts5(title, heading, body, note UNINDEXED,
                                       tokenize = 'porter unicode61');
CREATE TABLE refs (src INTEGER NOT NULL, name TEXT NOT NULL, PRIMARY KEY (src, name)) WITHOUT ROWID;
CREATE TABLE links (src INTEGER NOT NULL, dst INTEGER NOT NULL, PRIMARY KEY (src, dst)) WITHOUT ROWID;
CREATE INDEX links_dst ON links (dst);
CREATE TABLE files (name TEXT PRIMARY KEY) WITHOUT ROWID;
PRAGMA user_version = 1;";

pub fn db_path(cfg: &Config) -> PathBuf {
    cfg.db.clone()
}

pub fn remove_db(path: &Path) {
    for ext in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{ext}", path.display()));
    }
}

/// Open a database for writing, creating the file and `schema` when needed. One from another
/// `version` is deleted and created again: every row in these files is derived or disposable.
pub fn open_db(path: &Path, schema: &str, version: i32) -> Result<Connection, Fault> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| Fault::io(dir, e))?;
    }
    let mut conn = Connection::open(path)?;
    let found: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if found != version && found != 0 {
        drop(conn);
        remove_db(path);
        conn = Connection::open(path)?;
    }
    conn.busy_timeout(Duration::from_secs(5))?;
    // Every write transaction here reads first. A deferred one that finds another writer at its
    // first write gets "database is locked" at once, past the busy timeout (SQLite cannot wait
    // there without risking a deadlock); IMMEDIATE takes the write lock at BEGIN, where it waits.
    // Two reindexes at once, a doctor's and a backfill's, failed this way (Arch, 2026-11-25).
    conn.set_transaction_behavior(rusqlite::TransactionBehavior::Immediate);
    conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    if found != version {
        conn.execute_batch(schema)?;
    }
    Ok(conn)
}

/// Open a database that must already exist at `version`, for reading.
pub fn open_existing(path: &Path, version: i32, how: &str) -> Result<Connection, Fault> {
    if !path.exists() {
        return Err(Fault::Config(format!(
            "no index at {}: run `{how}`",
            path.display()
        )));
    }
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let found: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if found != version {
        return Err(Fault::Config(format!(
            "{} is from another version: run `{how} --rebuild`",
            path.display()
        )));
    }
    Ok(conn)
}

pub fn open_rw(cfg: &Config) -> Result<Connection, Fault> {
    open_db(&db_path(cfg), SCHEMA, SCHEMA_VERSION)
}

pub fn open_ro(cfg: &Config) -> Result<Connection, Fault> {
    open_existing(&db_path(cfg), SCHEMA_VERSION, "knowledge index")
}

#[derive(Default)]
pub struct Stats {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
}

fn read(cfg: &Config, rel: &str) -> Option<Vec<u8>> {
    fs::read(cfg.abs_of(rel)).ok()
}

/// Write one note: its row, its chunks, the names it links to. Returns the note's id.
fn put_note(
    tx: &Transaction,
    rel: &str,
    size: i64,
    mtime: i64,
    bytes: &[u8],
) -> Result<i64, Fault> {
    let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    let (block, body) = vault::split_front(&text);
    let fields = block.map(vault::fields).unwrap_or_default();
    let kind = vault::field(&fields, "type")
        .first()
        .map(|k| k.to_lowercase())
        .unwrap_or_default();
    let name = vault::name_of(rel);
    let mut title = name.to_string();
    for extra in vault::field(&fields, "aliases")
        .iter()
        .chain(vault::field(&fields, "tags"))
    {
        title.push(' ');
        title.push_str(extra);
    }
    let id: i64 = tx.query_row(
        "INSERT INTO notes (path, name, hash, kind, title, size, mtime) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (path) DO UPDATE SET name = excluded.name, hash = excluded.hash, kind = excluded.kind,
           title = excluded.title, size = excluded.size, mtime = excluded.mtime
         RETURNING id",
        params![rel, name.to_lowercase(), vault::hash(bytes), kind, title, size, mtime],
        |r| r.get(0),
    )?;
    let base = id * SLOTS as i64;
    tx.execute(
        "DELETE FROM chunks WHERE rowid >= ?1 AND rowid < ?2",
        [base, base + SLOTS as i64],
    )?;
    let mut ins = tx.prepare(
        "INSERT INTO chunks (rowid, title, heading, body, note) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (n, (heading, chunk)) in vault::chunks(body).iter().enumerate() {
        ins.execute(params![base + n as i64, title, heading, chunk, id])?;
    }
    tx.execute("DELETE FROM refs WHERE src = ?1", [id])?;
    let mut refs = tx.prepare("INSERT OR IGNORE INTO refs (src, name) VALUES (?1, ?2)")?;
    for link in vault::links_of(&text) {
        refs.execute(params![id, link.name.to_lowercase()])?;
    }
    Ok(id)
}

fn delete_note(tx: &Transaction, id: i64) -> Result<(), Fault> {
    let base = id * SLOTS as i64;
    tx.execute(
        "DELETE FROM chunks WHERE rowid >= ?1 AND rowid < ?2",
        [base, base + SLOTS as i64],
    )?;
    tx.execute("DELETE FROM refs WHERE src = ?1", [id])?;
    tx.execute("DELETE FROM links WHERE src = ?1 OR dst = ?1", [id])?;
    tx.execute("DELETE FROM notes WHERE id = ?1", [id])?;
    Ok(())
}

/// The note a name resolves to from `from`, as Obsidian and wikilinkPairs() pick it: the one in
/// the linking note's folder, else the shortest path.
pub fn pick<'a>(from: &str, candidates: &'a [(i64, String)]) -> Option<&'a (i64, String)> {
    let dir = vault::dir_of(from);
    candidates
        .iter()
        .find(|(_, p)| vault::dir_of(p) == dir)
        .or_else(|| {
            candidates
                .iter()
                .min_by(|a, b| a.1.len().cmp(&b.1.len()).then_with(|| a.1.cmp(&b.1)))
        })
}

pub fn notes_named(conn: &Connection, name: &str) -> Result<Vec<(i64, String)>, Fault> {
    let mut stmt = conn.prepare("SELECT id, path FROM notes WHERE name = ?1")?;
    let rows = stmt.query_map([name.to_lowercase()], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Re-resolve one note's outgoing links.
fn relink(tx: &Transaction, id: i64) -> Result<(), Fault> {
    let from: String = tx.query_row("SELECT path FROM notes WHERE id = ?1", [id], |r| r.get(0))?;
    tx.execute("DELETE FROM links WHERE src = ?1", [id])?;
    let names: Vec<String> = tx
        .prepare("SELECT name FROM refs WHERE src = ?1")?
        .query_map([id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut ins = tx.prepare("INSERT OR IGNORE INTO links (src, dst) VALUES (?1, ?2)")?;
    for name in names {
        if let Some((dst, _)) = pick(&from, &notes_named(tx, &name)?)
            && *dst != id
        {
            ins.execute([id, *dst])?;
        }
    }
    Ok(())
}

/// Re-resolve every link: a note added or removed can change what any name resolves to.
fn relink_all(tx: &Transaction) -> Result<(), Fault> {
    let mut by_name: HashMap<String, Vec<(i64, String)>> = HashMap::new();
    let mut path_of: HashMap<i64, String> = HashMap::new();
    for row in tx
        .prepare("SELECT id, path, name FROM notes")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
    {
        let (id, path, name): (i64, String, String) = row?;
        by_name.entry(name).or_default().push((id, path.clone()));
        path_of.insert(id, path);
    }
    tx.execute("DELETE FROM links", [])?;
    let mut ins = tx.prepare("INSERT OR IGNORE INTO links (src, dst) VALUES (?1, ?2)")?;
    for row in tx
        .prepare("SELECT src, name FROM refs")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
    {
        let (src, name): (i64, String) = row?;
        let (Some(from), Some(cands)) = (path_of.get(&src), by_name.get(&name)) else {
            continue;
        };
        if let Some((dst, _)) = pick(from, cands)
            && *dst != src
        {
            ins.execute([src, *dst])?;
        }
    }
    Ok(())
}

/// Bring the index level with the vault: only changed, new and missing notes are touched, in one
/// transaction. Size and mtime first, then the content hash.
pub fn sync(conn: &mut Connection, cfg: &Config) -> Result<Stats, Fault> {
    let files = vault::walk(&cfg.vault)?;
    sync_files(conn, cfg, &files, None)
}

/// A checkout's docs into the repo-docs index (`cfg` is `Config::repos()`): `docs/**.md`,
/// `CLAUDE.md` and `README.md`, under the vault's excludes, stored as `<Name>/...`. Only that
/// repo's rows are compared and removed.
pub fn sync_repo(conn: &mut Connection, cfg: &Config, name: &str) -> Result<Stats, Fault> {
    let root = cfg.vault.join(name);
    let mut files = Vec::new();
    let docs = root.join("docs");
    if docs.is_dir() {
        for f in vault::walk(&docs)? {
            files.push(File {
                rel: format!("{name}/docs/{}", f.rel),
                ..f
            });
        }
    }
    for top in ["CLAUDE.md", "README.md"] {
        if let Ok(meta) = fs::metadata(root.join(top))
            && meta.is_file()
        {
            files.push(File {
                rel: format!("{name}/{top}"),
                size: meta.len() as i64,
                mtime: vault::mtime(&meta),
            });
        }
    }
    sync_files(conn, cfg, &files, Some(&format!("{name}/")))
}

/// Whether a checkouts-relative path is a repo doc `sync_repo` would hold.
pub fn repo_doc(rel: &str) -> bool {
    let mut parts = rel.splitn(3, '/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(_), Some("CLAUDE.md" | "README.md"), None) => true,
        (Some(_), Some("docs"), Some(rest)) => vault::indexable(rest),
        _ => false,
    }
}

fn sync_files(
    conn: &mut Connection,
    cfg: &Config,
    files: &[File],
    prefix: Option<&str>,
) -> Result<Stats, Fault> {
    let tx = conn.transaction()?;
    let mut rows: HashMap<String, (i64, i64, i64, String)> = HashMap::new();
    for row in tx
        .prepare("SELECT path, id, size, mtime, hash FROM notes")?
        .query_map([], |r| {
            Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        })?
    {
        let (path, v): (String, _) = row?;
        if prefix.is_none_or(|p| path.starts_with(p)) {
            rows.insert(path, v);
        }
    }
    let mut stats = Stats::default();
    let mut changed = Vec::new();
    let mut renamed = false;
    for f in files.iter().filter(|f| f.is_note()) {
        match rows.remove(&f.rel) {
            Some((_, size, mtime, _)) if size == f.size && mtime == f.mtime => stats.unchanged += 1,
            old => {
                let Some(bytes) = read(cfg, &f.rel) else {
                    continue;
                };
                match old {
                    Some((id, _, _, hash)) if hash == vault::hash(&bytes) => {
                        tx.execute(
                            "UPDATE notes SET size = ?1, mtime = ?2 WHERE id = ?3",
                            params![f.size, f.mtime, id],
                        )?;
                        stats.unchanged += 1;
                    }
                    Some(_) => {
                        changed.push(put_note(&tx, &f.rel, f.size, f.mtime, &bytes)?);
                        stats.updated += 1;
                    }
                    None => {
                        put_note(&tx, &f.rel, f.size, f.mtime, &bytes)?;
                        renamed = true;
                        stats.added += 1;
                    }
                }
            }
        }
    }
    for (id, ..) in rows.into_values() {
        delete_note(&tx, id)?;
        renamed = true;
        stats.removed += 1;
    }
    if prefix.is_none() {
        put_files(&tx, files)?;
    }
    if renamed {
        relink_all(&tx)?;
    } else {
        for id in changed {
            relink(&tx, id)?;
        }
    }
    tx.commit()?;
    Ok(stats)
}

/// Every file's name, lower-cased: a `[[diagram.png]]` link resolves to a file, not a note.
fn put_files(tx: &Transaction, files: &[File]) -> Result<(), Fault> {
    tx.execute("DELETE FROM files", [])?;
    let mut ins = tx.prepare("INSERT OR IGNORE INTO files (name) VALUES (?1)")?;
    for f in files {
        ins.execute([f.rel.rsplit('/').next().unwrap_or(&f.rel).to_lowercase()])?;
    }
    Ok(())
}

/// Reindex one note by its stored path; a note no longer on disk is removed. Returns whether the
/// note is in the index afterwards.
pub fn one(conn: &mut Connection, cfg: &Config, rel: &str) -> Result<bool, Fault> {
    let path = cfg.abs_of(rel);
    let meta = fs::metadata(&path).ok().filter(fs::Metadata::is_file);
    let tx = conn.transaction()?;
    let old: Option<(i64, String)> = tx
        .query_row("SELECT id, hash FROM notes WHERE path = ?1", [rel], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let present = match (meta, old) {
        (Some(meta), old) => {
            let bytes = fs::read(&path).map_err(|e| Fault::io(&path, e))?;
            let (size, mtime) = (meta.len() as i64, vault::mtime(&meta));
            match old {
                Some((id, hash)) if hash == vault::hash(&bytes) => {
                    tx.execute(
                        "UPDATE notes SET size = ?1, mtime = ?2 WHERE id = ?3",
                        params![size, mtime, id],
                    )?;
                }
                Some(_) => {
                    let id = put_note(&tx, rel, size, mtime, &bytes)?;
                    relink(&tx, id)?;
                }
                None => {
                    put_note(&tx, rel, size, mtime, &bytes)?;
                    tx.execute(
                        "INSERT OR IGNORE INTO files (name) VALUES (?1)",
                        [rel.rsplit('/').next().unwrap_or(rel).to_lowercase()],
                    )?;
                    relink_all(&tx)?;
                }
            }
            true
        }
        (None, Some((id, _))) => {
            delete_note(&tx, id)?;
            relink_all(&tx)?;
            false
        }
        (None, None) => false,
    };
    tx.commit()?;
    Ok(present)
}

pub struct Check {
    pub notes: usize,
    pub rows: usize,
    pub ghosts: Vec<String>,
    pub missing: Vec<String>,
    pub stale: Vec<String>,
}

/// What the doctor's `vault-index` check needs: notes on disk, rows, and the differences.
pub fn check(conn: &Connection, cfg: &Config) -> Result<Check, Fault> {
    let files = vault::walk(&cfg.vault)?;
    let mut rows: HashMap<String, (i64, i64)> = HashMap::new();
    for row in conn
        .prepare("SELECT path, size, mtime FROM notes")?
        .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
    {
        let (path, v) = row?;
        rows.insert(path, v);
    }
    let total = rows.len();
    let (mut missing, mut stale) = (Vec::new(), Vec::new());
    let mut notes = 0;
    for f in files.iter().filter(|f| f.is_note()) {
        notes += 1;
        match rows.remove(&f.rel) {
            None => missing.push(f.rel.clone()),
            Some(v) if v != (f.size, f.mtime) => stale.push(f.rel.clone()),
            Some(_) => {}
        }
    }
    let mut ghosts: Vec<String> = rows.into_keys().collect();
    ghosts.sort();
    Ok(Check {
        notes,
        rows: total,
        ghosts,
        missing,
        stale,
    })
}

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &["repo"])?;
    let cfg = config::load()?;
    if let Some(root) = args.get("repo") {
        let t0 = Instant::now();
        let repos = cfg.repos();
        let name = repos
            .rel_of(Path::new(root))
            .filter(|r| !r.contains('/'))
            .ok_or_else(|| {
                Fault::Usage(format!(
                    "{root}: not a checkout directly under {}",
                    repos.vault.display()
                ))
            })?;
        let s = sync_repo(&mut open_rw(&repos)?, &repos, &name)?;
        emit(&format!(
            "{name}: {} docs: {} added, {} updated, {} removed, {} unchanged, in {} ms\n",
            s.added + s.updated + s.unchanged,
            s.added,
            s.updated,
            s.removed,
            s.unchanged,
            t0.elapsed().as_millis()
        ));
        return Ok(0);
    }
    if args.has("check") {
        let c = check(&open_ro(&cfg)?, &cfg)?;
        if args.has("json") {
            emit(&format!(
                "{}\n",
                serde_json::json!({"notes": c.notes, "rows": c.rows, "ghosts": c.ghosts, "missing": c.missing, "stale": c.stale})
            ));
        } else {
            emit(&format!(
                "{} notes on disk, {} rows: {} ghosts, {} missing, {} stale\n",
                c.notes,
                c.rows,
                c.ghosts.len(),
                c.missing.len(),
                c.stale.len()
            ));
        }
        return Ok(u8::from(!c.ghosts.is_empty() || !c.missing.is_empty()));
    }
    let t0 = Instant::now();
    if args.has("rebuild") {
        remove_db(&db_path(&cfg));
    }
    let mut conn = open_rw(&cfg)?;
    if !args.words.is_empty() {
        for w in &args.words {
            let rel = cfg
                .rel_of(Path::new(w))
                .filter(|r| vault::indexable(r))
                .ok_or_else(|| {
                    Fault::Usage(format!(
                        "{w}: not a note in the vault {}",
                        cfg.vault.display()
                    ))
                })?;
            let present = one(&mut conn, &cfg, &rel)?;
            emit(&format!(
                "{} {rel}\n",
                if present { "indexed" } else { "removed" }
            ));
        }
        return Ok(0);
    }
    let s = sync(&mut conn, &cfg)?;
    emit(&format!(
        "{} notes: {} added, {} updated, {} removed, {} unchanged, in {} ms\n",
        s.added + s.updated + s.unchanged,
        s.added,
        s.updated,
        s.removed,
        s.unchanged,
        t0.elapsed().as_millis()
    ));
    Ok(0)
}
