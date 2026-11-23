//! The code map (Stage 8): one SQLite file per checkout, KNOWLEDGE_HOME/code/<remote-name>.db,
//! rebuildable, never in git. Symbols and the refs between them, from each grammar's own tags
//! query. A ref resolves by name inside the repo, as graphify's AST pass did: no type resolution.
use crate::config::{self, Config};
use crate::index;
use crate::vault;
use crate::{Args, Fault, emit};
use rusqlite::{Connection, Transaction, params};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tree_sitter::{
    Language, Node, ParseOptions, ParseState, Parser, Query, QueryCursor, StreamingIterator, Tree,
};

const SCHEMA_VERSION: i32 = 1;

// `uses` holds each symbol's raw reference names; `refs` is them resolved, rebuilt in one
// statement after any change (index.rs keeps notes' `refs` and `links` the same way).
const SCHEMA: &str = "
CREATE TABLE files   (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL, lang TEXT NOT NULL,
                      size INTEGER NOT NULL, mtime INTEGER NOT NULL, hash TEXT NOT NULL);
CREATE TABLE symbols (id INTEGER PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL,
                      file INTEGER NOT NULL REFERENCES files(id), line INTEGER NOT NULL,
                      signature TEXT NOT NULL, shape TEXT);
CREATE INDEX symbols_name ON symbols (name);
CREATE INDEX symbols_file ON symbols (file);
CREATE TABLE uses    (src INTEGER NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL,
                      PRIMARY KEY (src, name, kind)) WITHOUT ROWID;
CREATE TABLE refs    (src INTEGER NOT NULL, dst INTEGER NOT NULL, kind TEXT NOT NULL,
                      PRIMARY KEY (src, dst, kind)) WITHOUT ROWID;
CREATE INDEX refs_dst ON refs (dst);
PRAGMA user_version = 1;";

/// Generated or minified code: not worth a parse, and the likeliest to be slow.
const MAX_BYTES: u64 = 1 << 20;
/// A hostile or broken file cannot hang the binary: the parse is cancelled past this. The slowest
/// tracked file measured 22.6 ms (S8.0).
const BUDGET: Duration = Duration::from_millis(200);
/// Bodies shorter than this get no shape: a two-line getter matches a hundred others and says
/// nothing about copying. A guess to tune against `duplicate`'s fixture (S8.3).
const MIN_SHAPE_TOKENS: usize = 32;
const MAX_SIGNATURE: usize = 160;

/// tree-sitter-rust 0.24.2's tags.scm marks `f()`, `x.f()` and `m!()` but not `path::f()` or
/// `f::<T>()`, which is how one module calls another (S8.1: `index::open_db` had no caller).
const RUST_CALLS: &str = "
(call_expression function: (scoped_identifier name: (identifier) @name)) @reference.call
(call_expression function: (generic_function function: (identifier) @name)) @reference.call
(call_expression function: (generic_function
  function: (scoped_identifier name: (identifier) @name))) @reference.call
(call_expression function: (generic_function
  function: (field_expression field: (field_identifier) @name))) @reference.call";

/// tree-sitter-javascript 0.25.0's tags.scm does not mark JSX: `<Section />` is how a React
/// component is used (S8.1: Queue's `ProtectionSection` had no caller). Capitalised names only, as
/// JSX itself tells a component from an HTML tag. Not for plain TypeScript, which has no JSX nodes.
const JSX_RENDERS: &str = r#"
(jsx_opening_element name: (identifier) @name (#match? @name "^[A-Z]")) @reference.render
(jsx_self_closing_element name: (identifier) @name (#match? @name "^[A-Z]")) @reference.render
(jsx_opening_element name: (member_expression property: (property_identifier) @name)) @reference.render
(jsx_self_closing_element name: (member_expression property: (property_identifier) @name)) @reference.render"#;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    JavaScript,
    TypeScript,
    Tsx,
    Rust,
    Python,
}

impl Lang {
    const ALL: [Lang; 5] = [
        Lang::JavaScript,
        Lang::TypeScript,
        Lang::Tsx,
        Lang::Rust,
        Lang::Python,
    ];

    fn of(path: &str) -> Option<Lang> {
        let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match ext.as_str() {
            "js" | "mjs" | "cjs" | "jsx" => Lang::JavaScript,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "rs" => Lang::Rust,
            "py" | "pyi" => Lang::Python,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Lang::JavaScript => "javascript",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::Rust => "rust",
            Lang::Python => "python",
        }
    }

    fn language(self) -> Language {
        match self {
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }

    /// TypeScript's tags.scm holds only the TS additions; the tree-sitter CLI runs JavaScript's
    /// under it, and so does this (both compile on both grammars, S8.0).
    fn tags(self) -> String {
        match self {
            Lang::JavaScript => format!("{}\n{JSX_RENDERS}", tree_sitter_javascript::TAGS_QUERY),
            Lang::TypeScript => format!(
                "{}\n{}",
                tree_sitter_javascript::TAGS_QUERY,
                tree_sitter_typescript::TAGS_QUERY
            ),
            Lang::Tsx => format!(
                "{}\n{}\n{JSX_RENDERS}",
                tree_sitter_javascript::TAGS_QUERY,
                tree_sitter_typescript::TAGS_QUERY
            ),
            Lang::Rust => format!("{}\n{RUST_CALLS}", tree_sitter_rust::TAGS_QUERY),
            Lang::Python => tree_sitter_python::TAGS_QUERY.to_string(),
        }
    }
}

/// One parser, and each language's query compiled on first use: a refresh with nothing changed
/// compiles none.
struct Grammars {
    parser: Parser,
    queries: [Option<Query>; 5],
}

impl Grammars {
    fn new() -> Grammars {
        Grammars {
            parser: Parser::new(),
            queries: Default::default(),
        }
    }

    fn parse(&mut self, lang: Lang, src: &[u8]) -> Result<Option<Tree>, Fault> {
        self.parser
            .set_language(&lang.language())
            .map_err(|e| Fault::Config(format!("{} grammar: {e}", lang.name())))?;
        let start = Instant::now();
        let mut over = |_: &ParseState| {
            if start.elapsed() > BUDGET {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let len = src.len();
        let tree = self.parser.parse_with_options(
            &mut |off, _| if off < len { &src[off..] } else { &[][..] },
            None,
            Some(ParseOptions::new().progress_callback(&mut over)),
        );
        if tree.is_none() {
            self.parser.reset();
        }
        Ok(tree)
    }

    fn query(&mut self, lang: Lang) -> Result<&Query, Fault> {
        let slot = &mut self.queries[Lang::ALL.iter().position(|l| *l == lang).unwrap_or(0)];
        if slot.is_none() {
            let q = Query::new(&lang.language(), &lang.tags())
                .map_err(|e| Fault::Config(format!("{} tags query: {e}", lang.name())))?;
            *slot = Some(q);
        }
        Ok(slot.as_ref().expect("set above"))
    }
}

struct Def {
    name: String,
    kind: String,
    line: i64,
    signature: String,
    shape: Option<String>,
    start: usize,
    end: usize,
}

struct Use {
    at: usize,
    name: String,
    kind: String,
}

/// The definitions and references the tags query marks in one file.
fn extract(q: &Query, tree: &Tree, src: &[u8]) -> (Vec<Def>, Vec<Use>) {
    let names = q.capture_names();
    let (mut defs, mut uses) = (Vec::new(), Vec::new());
    let mut seen = HashSet::new();
    let mut cursor = QueryCursor::new();
    let mut it = cursor.matches(q, tree.root_node(), src);
    while let Some(m) = it.next() {
        let (mut name, mut tag) = (None, None);
        for c in m.captures() {
            let n = names[c.index as usize];
            if n == "name" {
                name = Some(c.node);
            } else if n.starts_with("definition.") || n.starts_with("reference.") {
                tag = Some((n, c.node));
            }
        }
        let (Some(name), Some((tag, node))) = (name, tag) else {
            continue;
        };
        let Ok(text) = name.utf8_text(src) else {
            continue;
        };
        if text.is_empty() || text.len() > 200 {
            continue;
        }
        if let Some(kind) = tag.strip_prefix("definition.") {
            // JavaScript's and TypeScript's queries can both mark one method.
            if seen.insert((node.start_byte(), text.to_string())) {
                defs.push(Def {
                    name: text.to_string(),
                    kind: kind.to_string(),
                    line: node.start_position().row as i64 + 1,
                    signature: signature(node, src),
                    shape: shape(node, name, src),
                    start: node.start_byte(),
                    end: node.end_byte(),
                });
            }
        } else if let Some(kind) = tag.strip_prefix("reference.") {
            uses.push(Use {
                at: name.start_byte(),
                name: text.to_string(),
                kind: kind.to_string(),
            });
        }
    }
    (defs, uses)
}

/// The definition's first line, trimmed, at most MAX_SIGNATURE characters.
fn signature(def: Node, src: &[u8]) -> String {
    let text = &src[def.start_byte()..def.end_byte()];
    let first = text.split(|b| *b == b'\n').next().unwrap_or_default();
    let line = String::from_utf8_lossy(first);
    let line = line.trim().trim_end_matches('{').trim_end();
    match line.char_indices().nth(MAX_SIGNATURE) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_string(),
    }
}

/// A hash of the body's tokens, comments and whitespace dropped, and the definition's own name
/// left out, so a copy under a new name hashes the same. None for a short body.
fn shape(def: Node, name: Node, src: &[u8]) -> Option<String> {
    let mut buf = Vec::new();
    let mut tokens = 0;
    let mut c = def.walk();
    'walk: loop {
        let n = c.node();
        let skip = n.kind().contains("comment") || n.byte_range() == name.byte_range();
        if !skip && n.child_count() == 0 {
            buf.extend_from_slice(&src[n.byte_range()]);
            buf.push(0x1f);
            tokens += 1;
        }
        if !skip && c.goto_first_child() {
            continue;
        }
        loop {
            if c.node() == def {
                break 'walk;
            }
            if c.goto_next_sibling() {
                continue 'walk;
            }
            if !c.goto_parent() {
                break 'walk;
            }
        }
    }
    (tokens >= MIN_SHAPE_TOKENS).then(|| vault::hash(&buf))
}

pub fn db_path(cfg: &Config, repo: &str) -> PathBuf {
    cfg.home.join("code").join(format!("{repo}.db"))
}

pub fn open_rw(cfg: &Config, repo: &str) -> Result<Connection, Fault> {
    index::open_db(&db_path(cfg, repo), SCHEMA, SCHEMA_VERSION)
}

#[derive(Default)]
pub struct Stats {
    pub files: usize,
    pub parsed: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub skipped: usize,
    pub symbols: i64,
    pub refs: i64,
}

/// The tracked files a grammar covers, under the size cap: `git ls-files`, so `node_modules/`,
/// `target/` and anything gitignored is never read.
fn tracked(root: &Path) -> Result<Vec<(String, Lang, i64, i64)>, Fault> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .map_err(|e| Fault::io(root, e))?;
    if !out.status.success() {
        return Err(Fault::Config(format!(
            "{}: not a git checkout",
            root.display()
        )));
    }
    let mut files = Vec::new();
    for rel in out.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let rel = String::from_utf8_lossy(rel).into_owned();
        let Some(lang) = Lang::of(&rel) else {
            continue;
        };
        let Ok(meta) = fs::metadata(root.join(&rel)) else {
            continue;
        };
        if meta.is_file() && meta.len() <= MAX_BYTES {
            files.push((rel, lang, meta.len() as i64, vault::mtime(&meta)));
        }
    }
    Ok(files)
}

fn drop_file(tx: &Transaction, id: i64) -> Result<(), Fault> {
    tx.execute(
        "DELETE FROM uses WHERE src IN (SELECT id FROM symbols WHERE file = ?1)",
        [id],
    )?;
    tx.execute("DELETE FROM symbols WHERE file = ?1", [id])?;
    Ok(())
}

/// One file's symbols and raw uses. A file the parse budget cancelled keeps its row and hash, so
/// it is not tried again until it changes, and holds no symbols.
fn put_file(
    tx: &Transaction,
    g: &mut Grammars,
    rel: &str,
    lang: Lang,
    meta: (i64, i64),
    src: &[u8],
) -> Result<bool, Fault> {
    let id: i64 = tx.query_row(
        "INSERT INTO files (path, lang, size, mtime, hash) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (path) DO UPDATE SET lang = excluded.lang, size = excluded.size,
           mtime = excluded.mtime, hash = excluded.hash
         RETURNING id",
        params![rel, lang.name(), meta.0, meta.1, vault::hash(src)],
        |r| r.get(0),
    )?;
    drop_file(tx, id)?;
    let Some(tree) = g.parse(lang, src)? else {
        return Ok(false);
    };
    let (defs, uses) = extract(g.query(lang)?, &tree, src);
    let mut ins = tx.prepare(
        "INSERT INTO symbols (name, kind, file, line, signature, shape)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
    )?;
    // The file itself is a symbol, so a call at the top level has a source.
    let file_sym: i64 = ins.query_row(params![rel, "file", id, 1, rel, None::<String>], |r| {
        r.get(0)
    })?;
    let mut ids = Vec::with_capacity(defs.len());
    for d in &defs {
        ids.push(ins.query_row(
            params![d.name, d.kind, id, d.line, d.signature, d.shape],
            |r| r.get::<_, i64>(0),
        )?);
    }
    let mut put_use =
        tx.prepare("INSERT OR IGNORE INTO uses (src, name, kind) VALUES (?1, ?2, ?3)")?;
    for u in &uses {
        // The innermost definition around the use is its source. O(defs × uses) per file: a few
        // hundred by a few thousand at most in the checkouts (S8.0); an interval tree if not.
        let src_id = defs
            .iter()
            .zip(&ids)
            .filter(|(d, _)| d.start <= u.at && u.at < d.end)
            .min_by_key(|(d, _)| d.end - d.start)
            .map_or(file_sym, |(_, id)| *id);
        put_use.execute(params![src_id, u.name, u.kind])?;
    }
    Ok(true)
}

/// Bring one checkout's map level with its tracked files: size and mtime first, then the content
/// hash, then a parse. Refs are resolved again only when something changed.
pub fn sync(conn: &mut Connection, root: &Path) -> Result<Stats, Fault> {
    let files = tracked(root)?;
    let tx = conn.transaction()?;
    let mut rows: HashMap<String, (i64, i64, i64, String)> = HashMap::new();
    for row in tx
        .prepare("SELECT path, id, size, mtime, hash FROM files")?
        .query_map([], |r| {
            Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        })?
    {
        let (path, v): (String, _) = row?;
        rows.insert(path, v);
    }
    let mut g = Grammars::new();
    let mut s = Stats {
        files: files.len(),
        ..Stats::default()
    };
    for (rel, lang, size, mtime) in &files {
        let old = rows.remove(rel);
        if let Some((_, osize, omtime, _)) = &old
            && osize == size
            && omtime == mtime
        {
            s.unchanged += 1;
            continue;
        }
        let path = root.join(rel);
        let Ok(src) = fs::read(&path) else {
            continue;
        };
        if let Some((id, _, _, hash)) = &old
            && *hash == vault::hash(&src)
        {
            tx.execute(
                "UPDATE files SET size = ?1, mtime = ?2 WHERE id = ?3",
                params![size, mtime, id],
            )?;
            s.unchanged += 1;
            continue;
        }
        if put_file(&tx, &mut g, rel, *lang, (*size, *mtime), &src)? {
            s.parsed += 1;
        } else {
            s.skipped += 1;
        }
    }
    for (id, ..) in rows.into_values() {
        drop_file(&tx, id)?;
        tx.execute("DELETE FROM files WHERE id = ?1", [id])?;
        s.removed += 1;
    }
    if s.parsed + s.skipped + s.removed > 0 {
        // Name-only resolution in one statement: an ambiguous name links to every candidate.
        tx.execute_batch(
            "DELETE FROM refs;
             INSERT OR IGNORE INTO refs (src, dst, kind)
               SELECT u.src, s.id, u.kind FROM uses u
               JOIN symbols s ON s.name = u.name AND s.kind != 'file'
               WHERE s.id != u.src;",
        )?;
    }
    s.symbols = tx.query_row(
        "SELECT count(*) FROM symbols WHERE kind != 'file'",
        [],
        |r| r.get(0),
    )?;
    s.refs = tx.query_row("SELECT count(*) FROM refs", [], |r| r.get(0))?;
    tx.commit()?;
    Ok(s)
}

/// The checkouts to work on: the named one, every one (`--all`), or the one the working
/// directory is in.
fn checkouts(cfg: &Config, args: &Args) -> Result<Vec<String>, Fault> {
    if args.has("all") {
        let mut names: Vec<String> = fs::read_dir(&cfg.dev)
            .map_err(|e| Fault::io(&cfg.dev, e))?
            .filter_map(Result::ok)
            .filter(|e| e.path().join(".git").exists())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        return Ok(names);
    }
    let name = match args.words.first() {
        Some(w) => w.clone(),
        None => std::env::current_dir()
            .ok()
            .and_then(|cwd| cfg.repos().rel_of(&cwd))
            .and_then(|rel| rel.split('/').next().map(str::to_string))
            .ok_or_else(|| {
                Fault::Usage(format!(
                    "not inside a checkout under {}: name one, or --all",
                    cfg.dev.display()
                ))
            })?,
    };
    if !cfg.dev.join(&name).join(".git").exists() {
        return Err(Fault::Usage(format!(
            "{name}: no checkout at {}",
            cfg.dev.join(&name).display()
        )));
    }
    Ok(vec![name])
}

/// `knowledge code index [<repo>|--all] [--rebuild]`.
pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let (sub, rest) = raw
        .split_first()
        .map_or(("", &[][..]), |(v, r)| (v.as_str(), r));
    if sub != "index" {
        return Err(Fault::Usage(
            "usage: knowledge code index [<repo>|--all] [--rebuild]".to_string(),
        ));
    }
    let args = Args::parse(rest, &[])?;
    let cfg = config::load()?;
    for name in checkouts(&cfg, &args)? {
        let t0 = Instant::now();
        if args.has("rebuild") {
            index::remove_db(&db_path(&cfg, &name));
        }
        let s = sync(&mut open_rw(&cfg, &name)?, &cfg.dev.join(&name))?;
        emit(&format!(
            "{name}: {} files: {} parsed, {} unchanged, {} removed, {} skipped; {} symbols, {} refs, in {} ms\n",
            s.files,
            s.parsed,
            s.unchanged,
            s.removed,
            s.skipped,
            s.symbols,
            s.refs,
            t0.elapsed().as_millis()
        ));
    }
    Ok(0)
}
