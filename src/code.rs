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

// 2: refs prefer a definition in the using file and stay in one language (S8.2).
// 3: uses of a type are refs too (S8.4).
// 4: a ref crosses files only to a definition the other file can name; a Rust `x.f()` reaches
//    methods only; a function handed on as a value is a use (S8.4). A cache of another version
//    is rebuilt.
const SCHEMA_VERSION: i32 = 4;

// `uses` holds each symbol's raw reference names; `refs` is them resolved, rebuilt in one
// statement after any change (index.rs keeps notes' `refs` and `links` the same way).
// `reach`: another file can name the symbol (see `reach`). `member`: a Rust method-call use.
const SCHEMA: &str = "
CREATE TABLE files   (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL, lang TEXT NOT NULL,
                      size INTEGER NOT NULL, mtime INTEGER NOT NULL, hash TEXT NOT NULL);
CREATE TABLE symbols (id INTEGER PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL,
                      file INTEGER NOT NULL REFERENCES files(id), line INTEGER NOT NULL,
                      signature TEXT NOT NULL, shape TEXT, reach INTEGER NOT NULL);
CREATE INDEX symbols_name ON symbols (name);
CREATE INDEX symbols_file ON symbols (file);
CREATE TABLE uses    (src INTEGER NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL,
                      member INTEGER NOT NULL,
                      PRIMARY KEY (src, name, kind, member)) WITHOUT ROWID;
CREATE TABLE refs    (src INTEGER NOT NULL, dst INTEGER NOT NULL, kind TEXT NOT NULL,
                      PRIMARY KEY (src, dst, kind)) WITHOUT ROWID;
CREATE INDEX refs_dst ON refs (dst);
PRAGMA user_version = 4;";

/// Generated or minified code: not worth a parse, and the likeliest to be slow.
const MAX_BYTES: u64 = 1 << 20;
/// A hostile or broken file cannot hang the binary: the parse is cancelled past this. The slowest
/// tracked file measured 22.6 ms (S8.0).
const BUDGET: Duration = Duration::from_millis(200);
/// Bodies shorter than this get no shape: a two-line getter matches a hundred others and says
/// nothing about copying. Measured at S8.3 (2026-11-24): shapes shared by two or more symbols, 7
/// among TheAtlas's 961, 2 among the skeleton's 671 (one a real copy, `is_log_name`), 0 among
/// TheAtlas-Queue's 646.
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

/// No grammar's tags.scm marks a type used in a field, a signature, an `impl` or a cast, so a struct
/// or an enum had no users at all (S8.4: Vantage's `TableState` "used by (0)" against 26 lines of
/// `git grep`; graphify's AST pass listed 8). A type's own name sits inside its own definition and
/// resolves to itself, which `sync` drops. Rust's `TableState::Open` is a path, not a type node:
/// a capitalised path head is a type, a lower-case one a module.
const RUST_TYPES: &str = r#"
(type_identifier) @name @reference.type
(scoped_identifier path: (identifier) @name (#match? @name "^[A-Z]")) @reference.type"#;
/// A function handed on as a value is used without a call: `players.map(score)`, a callback,
/// `actions={{ announceSaves }}` (S8.4: Queue's `score` had no user in either tool). Such a use
/// resolves to a function or a class only (`sync`): a variable is no symbol, and one named like
/// another file's method or constant linked to it. Not for Rust, where a bare argument is nearly
/// always a variable: of 75 such refs in the four checkouts, about half were `path`, `args`, `len`
/// and the like naming a function elsewhere (S8.4).
const JS_VALUES: &str = "
(arguments (identifier) @name) @reference.value
(shorthand_property_identifier) @name @reference.value
(pair value: (identifier) @name) @reference.value";
/// `onClick={submit}`: where JSX_RENDERS goes, as plain TypeScript has no JSX nodes.
const JSX_VALUES: &str = "
(jsx_expression (identifier) @name) @reference.value";
const PYTHON_VALUES: &str = "
(argument_list (identifier) @name) @reference.value";

/// TypeScript's annotations, `extends` and generics; JavaScript has no type nodes. Its tags.scm
/// defines interfaces but not a `type` alias or an `enum`, so `parseCommand(…, commands: Commands)`
/// found no `Commands` beside it and linked a component of that name in another file (S8.4).
const TS_TYPES: &str = "
(type_identifier) @name @reference.type
(type_alias_declaration name: (type_identifier) @name) @definition.type
(enum_declaration name: (identifier) @name) @definition.enum";
/// Python's annotations: `x: Foo`, and the head of `list[Foo]` (whose `Foo` is a `type` again).
const PYTHON_TYPES: &str = "
(type (identifier) @name) @reference.type
(generic_type (identifier) @name) @reference.type";

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
            Lang::JavaScript => format!(
                "{}\n{JSX_RENDERS}\n{JS_VALUES}\n{JSX_VALUES}",
                tree_sitter_javascript::TAGS_QUERY
            ),
            Lang::TypeScript => format!(
                "{}\n{}\n{TS_TYPES}\n{JS_VALUES}",
                tree_sitter_javascript::TAGS_QUERY,
                tree_sitter_typescript::TAGS_QUERY
            ),
            Lang::Tsx => format!(
                "{}\n{}\n{JSX_RENDERS}\n{TS_TYPES}\n{JS_VALUES}\n{JSX_VALUES}",
                tree_sitter_javascript::TAGS_QUERY,
                tree_sitter_typescript::TAGS_QUERY
            ),
            Lang::Rust => format!(
                "{}\n{RUST_CALLS}\n{RUST_TYPES}",
                tree_sitter_rust::TAGS_QUERY
            ),
            Lang::Python => format!(
                "{}\n{PYTHON_TYPES}\n{PYTHON_VALUES}",
                tree_sitter_python::TAGS_QUERY
            ),
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
    reach: bool,
    start: usize,
    end: usize,
}

struct Use {
    at: usize,
    name: String,
    kind: String,
    member: bool,
}

/// The definitions and references the tags query marks in one file.
fn extract(lang: Lang, q: &Query, tree: &Tree, src: &[u8]) -> (Vec<Def>, Vec<Use>) {
    let names = q.capture_names();
    let (mut defs, mut uses) = (Vec::new(), Vec::new());
    let mut seen = HashSet::new();
    let exports = Exports::of(lang, tree.root_node(), src);
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
                    reach: reach(lang, node, text, &exports),
                    start: node.start_byte(),
                    end: node.end_byte(),
                });
            }
        } else if let Some(kind) = tag.strip_prefix("reference.") {
            uses.push(Use {
                at: name.start_byte(),
                name: text.to_string(),
                kind: kind.to_string(),
                // Only Rust's grammar has `field_expression`: `x.f()` there is a method, never a
                // free function (S8.4: `s.join(…)` linked to `fetch.rs`'s `fn join`).
                member: name
                    .parent()
                    .is_some_and(|p| p.kind() == "field_expression"),
            });
        }
    }
    (defs, uses)
}

/// A JS or TS file's module facts: whether it imports or exports at all (a file that does neither
/// is a script, whose top level is global), and the names `export { a, b }` and `export default a`
/// export.
struct Exports {
    module: bool,
    names: HashSet<String>,
}

impl Exports {
    fn of(lang: Lang, root: Node, src: &[u8]) -> Exports {
        let mut e = Exports {
            module: false,
            names: HashSet::new(),
        };
        if !matches!(lang, Lang::JavaScript | Lang::TypeScript | Lang::Tsx) {
            return e;
        }
        let text = |n: Node| n.utf8_text(src).ok().map(str::to_string);
        let mut c = root.walk();
        for n in root.children(&mut c) {
            match n.kind() {
                "import_statement" => e.module = true,
                "export_statement" => {
                    e.module = true;
                    e.names
                        .extend(n.child_by_field_name("value").and_then(text));
                    let mut cc = n.walk();
                    for clause in n.children(&mut cc).filter(|c| c.kind() == "export_clause") {
                        let mut sc = clause.walk();
                        for spec in clause.children(&mut sc) {
                            e.names
                                .extend(spec.child_by_field_name("name").and_then(text));
                        }
                    }
                }
                _ => {}
            }
        }
        e
    }
}

/// Whether a file other than the definition's own can name it. Name-only resolution linked
/// Queue's `content.replace()` to a hook's local closure and TheAtlas's `tauri::State` to a
/// private struct in another crate (S8.4). A use in the definition's own file needs none of this.
/// Rust: `pub` of any scope, a trait's or a trait impl's method, a macro. JS and TS: a method or
/// an object's property, which travel with their object; a top-level declaration exported where
/// it stands or by name, or any top-level one in a script. Python: anything not inside a function.
/// Not followed: a Rust child module's `super::` reach into a private item, and imports, which
/// would tell two exported names apart.
fn reach(lang: Lang, def: Node, name: &str, exports: &Exports) -> bool {
    match lang {
        Lang::Rust => {
            let mut c = def.walk();
            def.children(&mut c)
                .any(|n| n.kind() == "visibility_modifier")
                || def.kind() == "macro_definition"
                || def.parent().and_then(|l| l.parent()).is_some_and(|p| {
                    p.kind() == "trait_item"
                        || (p.kind() == "impl_item" && p.child_by_field_name("trait").is_some())
                })
        }
        Lang::JavaScript | Lang::TypeScript | Lang::Tsx => {
            if matches!(
                def.kind(),
                "method_definition"
                    | "method_signature"
                    | "abstract_method_signature"
                    | "pair"
                    | "assignment_expression"
            ) {
                return true;
            }
            let mut n = def;
            while let Some(p) = n.parent() {
                match p.kind() {
                    "export_statement" => return true,
                    "program" => return !exports.module || exports.names.contains(name),
                    "lexical_declaration" | "variable_declaration" | "ambient_declaration" => n = p,
                    _ => return false,
                }
            }
            false
        }
        Lang::Python => {
            let mut p = def.parent();
            while let Some(n) = p {
                if n.kind() == "function_definition" {
                    return false;
                }
                p = n.parent();
            }
            true
        }
    }
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
    let (defs, uses) = extract(lang, g.query(lang)?, &tree, src);
    let mut ins = tx.prepare(
        "INSERT INTO symbols (name, kind, file, line, signature, shape, reach)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )?;
    // The file itself is a symbol, so a call at the top level has a source.
    let file_sym: i64 = ins.query_row(
        params![rel, "file", id, 1, rel, None::<String>, false],
        |r| r.get(0),
    )?;
    let mut ids = Vec::with_capacity(defs.len());
    for d in &defs {
        ids.push(ins.query_row(
            params![d.name, d.kind, id, d.line, d.signature, d.shape, d.reach],
            |r| r.get::<_, i64>(0),
        )?);
    }
    let mut put_use =
        tx.prepare("INSERT OR IGNORE INTO uses (src, name, kind, member) VALUES (?1, ?2, ?3, ?4)")?;
    for u in &uses {
        // The innermost definition around the use is its source. O(defs × uses) per file: a few
        // hundred by a few thousand at most in the checkouts (S8.0); an interval tree if not.
        let src_id = defs
            .iter()
            .zip(&ids)
            .filter(|(d, _)| d.start <= u.at && u.at < d.end)
            .min_by_key(|(d, _)| d.end - d.start)
            .map_or(file_sym, |(_, id)| *id);
        put_use.execute(params![src_id, u.name, u.kind, u.member])?;
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
        // Name-only resolution in one statement. A name the using file defines itself means that
        // definition, as an unqualified call does in Rust, Python and a JS module (S8.2: three
        // `open_rw`s, and `code.rs`'s own call linked to all three). Any other name links to every
        // candidate, and the verbs mark it ambiguous. A name never crosses languages: JS and TS
        // import each other, Rust and Python reach no other (S8.2: Rust's `conn.prepare` linked to
        // a method in a `.mjs`). Another file's definition is a candidate only when that file can
        // name it, and a Rust `x.f()` only reaches a method (`reach`, `extract`; S8.4).
        tx.execute_batch(
            "DELETE FROM refs;
             INSERT OR IGNORE INTO refs (src, dst, kind)
               SELECT u.src, s.id, u.kind FROM uses u
               JOIN symbols us ON us.id = u.src
               JOIN files uf ON uf.id = us.file
               JOIN symbols s ON s.name = u.name AND s.kind != 'file'
                 AND (u.member = 0 OR s.kind = 'method')
                 AND (u.kind != 'value' OR s.kind IN ('function', 'class'))
               JOIN files sf ON sf.id = s.file
               WHERE s.id != u.src
                 AND (CASE WHEN uf.lang IN ('rust', 'python') THEN uf.lang ELSE 'js' END)
                   = (CASE WHEN sf.lang IN ('rust', 'python') THEN sf.lang ELSE 'js' END)
                 AND (s.file = us.file OR (s.reach = 1 AND NOT EXISTS (
                   SELECT 1 FROM symbols l WHERE l.name = u.name AND l.file = us.file
                     AND l.kind != 'file' AND l.id != u.src
                     AND (u.member = 0 OR l.kind = 'method')
                     AND (u.kind != 'value' OR l.kind IN ('function', 'class')))));",
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

/// Every checkout under the checkouts folder.
fn every(cfg: &Config) -> Result<Vec<String>, Fault> {
    let mut names: Vec<String> = fs::read_dir(&cfg.dev)
        .map_err(|e| Fault::io(&cfg.dev, e))?
        .filter_map(Result::ok)
        .filter(|e| e.path().join(".git").exists())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names)
}

/// The checkout the working directory is in.
fn here(cfg: &Config) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let rel = cfg.repos().rel_of(&cwd)?;
    rel.split('/').next().map(str::to_string)
}

/// A named checkout. The name is a folder name and never a path: it reaches here from a model
/// through `knowledge mcp`, and it names a folder under the checkouts and a file under the home.
fn named(cfg: &Config, name: &str) -> Result<String, Fault> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
        return Err(Fault::Usage(format!("{name}: a checkout's folder name")));
    }
    if !cfg.dev.join(name).join(".git").exists() {
        return Err(Fault::Usage(format!(
            "{name}: no checkout at {}",
            cfg.dev.join(name).display()
        )));
    }
    Ok(name.to_string())
}

/// The checkouts to index: the named one, every one (`--all`), or the one the working directory
/// is in.
fn checkouts(cfg: &Config, args: &Args) -> Result<Vec<String>, Fault> {
    if args.has("all") {
        return every(cfg);
    }
    let name = match args.words.first() {
        Some(w) => w.clone(),
        None => here(cfg).ok_or_else(|| {
            Fault::Usage(format!(
                "not inside a checkout under {}: name one, or --all",
                cfg.dev.display()
            ))
        })?,
    };
    Ok(vec![named(cfg, &name)?])
}

/// The maps a query reads, each refreshed first so an answer never lags the files: `--repo`'s,
/// else the working directory's checkout, else every checkout.
fn scoped(cfg: &Config, repo: Option<&str>) -> Result<Vec<(String, Connection)>, Fault> {
    let names = match (repo, here(cfg)) {
        (Some(r), _) => vec![named(cfg, r)?],
        (None, Some(h)) => vec![h],
        (None, None) => every(cfg)?,
    };
    let mut maps = Vec::with_capacity(names.len());
    for name in names {
        let mut conn = open_rw(cfg, &name)?;
        sync(&mut conn, &cfg.dev.join(&name))?;
        maps.push((name, conn));
    }
    Ok(maps)
}

/// Output held to a character budget, as every verb's is: a line that does not fit is counted,
/// never cut in half.
struct Out {
    text: String,
    budget: usize,
    dropped: usize,
}

impl Out {
    fn new(args: &Args) -> Result<Out, Fault> {
        Ok(Out {
            text: String::new(),
            budget: args.num("budget", crate::capture::BUDGET)?.max(200),
            dropped: 0,
        })
    }

    fn line(&mut self, l: &str) {
        if self.dropped == 0 && self.text.len() + l.len() < self.budget {
            self.text.push_str(l);
            self.text.push('\n');
        } else {
            self.dropped += 1;
        }
    }

    fn finish(mut self) -> String {
        if self.dropped > 0 {
            self.text.push_str(&format!(
                "… {} more lines past the {}-char budget (--budget N)\n",
                self.dropped, self.budget
            ));
        }
        self.text
    }
}

/// How well a name matches the query: exact, exact but for case, a prefix, then inside the name.
fn match_rank(name: &str, query: &str) -> u8 {
    if name == query {
        0
    } else if name.eq_ignore_ascii_case(query) {
        1
    } else if name.to_lowercase().starts_with(&query.to_lowercase()) {
        2
    } else {
        3
    }
}

/// Types before functions before the rest: "where is X" most often means where X is declared.
fn kind_rank(kind: &str) -> u8 {
    match kind {
        "class" | "interface" | "struct" | "enum" | "type" | "trait" => 0,
        "function" | "method" | "macro" => 1,
        "module" => 2,
        _ => 3,
    }
}

/// `find --code <q> [--repo <name>] [--limit N] [--budget N]`: symbols whose name holds every
/// word of the query, ranked by name match, then kind. One line each: `repo path:line kind
/// signature`.
pub fn run_find(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &["repo", "limit", "budget"])?;
    let query = args.words.join(" ");
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let Some(first) = terms.first() else {
        return Err(Fault::Usage(
            "knowledge find <query> --code [--repo <name>] [--limit N] [--budget N]".into(),
        ));
    };
    let limit = args.num("limit", 10usize)?.clamp(1, 50);
    let mut out = Out::new(&args)?;
    let cfg = config::load()?;
    let mut hits = Vec::new();
    for (repo, conn) in scoped(&cfg, args.get("repo"))? {
        // The first word narrows in SQL (a scan of one table: ~10,000 rows in the largest
        // checkout), the rest in Rust.
        let mut stmt = conn.prepare(
            "SELECT s.name, s.kind, f.path, s.line, s.signature FROM symbols s
             JOIN files f ON f.id = s.file
             WHERE s.kind != 'file' AND instr(lower(s.name), ?1) > 0",
        )?;
        let rows = stmt.query_map([first], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (name, kind, path, line, sig) = row?;
            let lower = name.to_lowercase();
            if terms.iter().all(|t| lower.contains(t.as_str())) {
                let rank = (match_rank(&name, &query), kind_rank(&kind), name.len());
                hits.push((rank, format!("{repo} {path}:{line} {kind} {sig}")));
            }
        }
    }
    if hits.is_empty() {
        out.line(&format!("no symbol named like {query}"));
    }
    hits.sort();
    for (_, l) in hits.iter().take(limit) {
        out.line(l);
    }
    if hits.len() > limit {
        out.line(&format!(
            "… and {} more matches (--limit N)",
            hits.len() - limit
        ));
    }
    emit(&out.finish());
    Ok(0)
}

/// A symbol's definitions by exact name, or failing that by name without case. File symbols are
/// never asked for by name.
fn defs(conn: &Connection, name: &str) -> Result<Vec<i64>, Fault> {
    for sql in [
        "SELECT id FROM symbols WHERE name = ?1 AND kind != 'file' ORDER BY id",
        "SELECT id FROM symbols WHERE lower(name) = lower(?1) AND kind != 'file' ORDER BY id",
    ] {
        let mut stmt = conn.prepare(sql)?;
        let ids: Vec<i64> = stmt
            .query_map([name], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if !ids.is_empty() {
            return Ok(ids);
        }
    }
    Ok(Vec::new())
}

/// `path:line name kind`, or the path alone for a file's top level.
fn label(conn: &Connection, id: i64) -> Result<String, Fault> {
    Ok(conn.query_row(
        "SELECT s.name, s.kind, f.path, s.line FROM symbols s JOIN files f ON f.id = s.file
         WHERE s.id = ?1",
        [id],
        |r| {
            let (name, kind, path, line): (String, String, String, i64) =
                (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
            Ok(if kind == "file" {
                format!("{path} (top level)")
            } else {
                format!("{path}:{line} {name} {kind}")
            })
        },
    )?)
}

/// The ref from `src` to `dst` as a line: the other end's label, the ref's kind when it is not a
/// call, and how many symbols the name could mean when name-only resolution had to guess.
fn edge(conn: &Connection, src: i64, dst: i64, other: i64, kind: &str) -> Result<String, Fault> {
    let n: i64 = conn.query_row(
        "SELECT count(DISTINCT r.dst) FROM refs r JOIN symbols s ON s.id = r.dst
         WHERE r.src = ?1 AND s.name = (SELECT name FROM symbols WHERE id = ?2)",
        [src, dst],
        |r| r.get(0),
    )?;
    let mut l = label(conn, other)?;
    if kind != "call" {
        l.push_str(&format!(" [{kind}]"));
    }
    if n > 1 {
        l.push_str(&format!(" [ambiguous: {n} by this name]"));
    }
    Ok(l)
}

/// What uses one symbol (`callers`), or what it uses.
fn ends(conn: &Connection, id: i64, callers: bool) -> Result<Vec<String>, Fault> {
    let sql = if callers {
        "SELECT src, min(kind) FROM refs WHERE dst = ?1 GROUP BY src ORDER BY src"
    } else {
        "SELECT dst, min(kind) FROM refs WHERE src = ?1 GROUP BY dst ORDER BY dst"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(|(other, kind)| {
            let (src, dst) = if callers { (other, id) } else { (id, other) };
            edge(conn, src, dst, other, &kind)
        })
        .collect()
}

/// A symbol's definitions, each with what uses it and what it uses.
fn neighbours(maps: &[(String, Connection)], sym: &str, out: &mut Out) -> Result<(), Fault> {
    let mut found = false;
    for (repo, conn) in maps {
        for id in defs(conn, sym)? {
            found = true;
            let (sig, place): (String, String) = conn.query_row(
                "SELECT s.signature, f.path || ':' || s.line || ' ' || s.kind FROM symbols s
                 JOIN files f ON f.id = s.file WHERE s.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let (callers, callees) = (ends(conn, id, true)?, ends(conn, id, false)?);
            out.line(&format!("{sym} — {repo} {place}"));
            out.line(&format!("  {sig}"));
            for (head, list) in [("used by", &callers), ("uses", &callees)] {
                out.line(&format!("  {head} ({}):", list.len()));
                for l in list {
                    out.line(&format!("    {l}"));
                }
            }
        }
    }
    if !found {
        out.line(&format!(
            "no symbol named {sym}; `knowledge find {sym} --code` lists near names"
        ));
    }
    Ok(())
}

/// The shortest chain of refs from any definition of `a` to any of `b`, at most DEPTH hops, walked
/// back from `b` one hop closer each step (graph.rs does the same over notes' links).
fn chain(conn: &Connection, a: &[i64], b: &[i64]) -> Result<Option<Vec<i64>>, Fault> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE reach(id, d) AS (
           SELECT value, 0 FROM json_each(?1)
           UNION SELECT r.dst, reach.d + 1 FROM reach JOIN refs r ON r.src = reach.id
             WHERE reach.d < ?2
         )
         SELECT id, min(d) FROM reach GROUP BY id",
    )?;
    let seeds = serde_json::to_string(a)?;
    let dist: HashMap<i64, i64> = stmt
        .query_map(params![seeds, DEPTH], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let Some((&end, &d)) = b
        .iter()
        .filter_map(|id| dist.get_key_value(id))
        .min_by_key(|(_, d)| **d)
    else {
        return Ok(None);
    };
    let mut back = conn.prepare("SELECT src FROM refs WHERE dst = ?1 ORDER BY src")?;
    let (mut path, mut at) = (vec![end], end);
    for step in (0..d).rev() {
        let prev: Vec<i64> = back
            .query_map([at], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        at = *prev
            .iter()
            .find(|n| dist.get(n) == Some(&step))
            .ok_or_else(|| Fault::Config("the code map changed mid-query".into()))?;
        path.push(at);
    }
    path.reverse();
    Ok(Some(path))
}

/// How far `graph --code <a> <b>` looks, as `graph` does for notes: past four hops a chain runs
/// through a name a hundred symbols share and says nothing.
const DEPTH: i64 = 4;

/// The chain from `a` to `b`, or failing that from `b` to `a`, in the first checkout that defines
/// both and joins them.
fn between(maps: &[(String, Connection)], a: &str, b: &str, out: &mut Out) -> Result<(), Fault> {
    let mut both = false;
    for (repo, conn) in maps {
        let (da, db) = (defs(conn, a)?, defs(conn, b)?);
        if da.is_empty() || db.is_empty() {
            continue;
        }
        both = true;
        if let Some(path) = chain(conn, &da, &db)?.or(chain(conn, &db, &da)?) {
            out.line(&format!("{repo}:"));
            out.line(&format!("  {}", label(conn, path[0])?));
            for hop in path.windows(2) {
                let kind: String = conn.query_row(
                    "SELECT min(kind) FROM refs WHERE src = ?1 AND dst = ?2",
                    [hop[0], hop[1]],
                    |r| r.get(0),
                )?;
                out.line(&format!(
                    "  → {}",
                    edge(conn, hop[0], hop[1], hop[1], &kind)?
                ));
            }
            return Ok(());
        }
    }
    out.line(&if both {
        format!("no chain of uses between {a} and {b} within {DEPTH} hops")
    } else {
        format!("no checkout defines both {a} and {b}")
    });
    Ok(())
}

/// `graph --code <symbol> [<other>] [--repo <name>] [--budget N]`: a symbol's definition, what
/// uses it and what it uses; or the chain of uses between two symbols.
pub fn run_graph(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &["repo", "budget"])?;
    let mut out = Out::new(&args)?;
    let cfg = config::load()?;
    match args.words.as_slice() {
        [sym] => neighbours(&scoped(&cfg, args.get("repo"))?, sym, &mut out)?,
        [a, b] => between(&scoped(&cfg, args.get("repo"))?, a, b, &mut out)?,
        _ => {
            return Err(Fault::Usage(
                "knowledge graph <symbol> [<other symbol>] --code [--repo <name>] [--budget N]"
                    .into(),
            ));
        }
    }
    emit(&out.finish());
    Ok(0)
}

/// Next.js's App Router file conventions: every app defines its own `page.tsx` and `layout.tsx`,
/// so one name in two apps' convention files is the framework, not a copy. Measured on TheAtlas
/// (2026-11-24): 7 of the 9 names defined in more than one app were these; the other two,
/// `AppNav` and `NotFoundView`, are the copies the turborepo rule means.
const NEXT_FILES: [&str; 10] = [
    "page",
    "layout",
    "template",
    "loading",
    "error",
    "global-error",
    "not-found",
    "default",
    "route",
    "middleware",
];

/// A copy listed against more symbols than this says "and N more": six crates' `check!` macros
/// would otherwise fill the budget with one finding.
const MAX_MATCHES: usize = 3;

/// The language family a name stays in, as `sync`'s refs do: JS and TS together, Rust and Python
/// each alone (TheAtlas's Rust `run` and a TSX `run` are not one thing).
fn family(lang: &str) -> &str {
    match lang {
        "rust" | "python" => lang,
        _ => "js",
    }
}

/// The app an `apps/<name>/…` file belongs to, unless it is one of Next.js's convention files.
fn app_of(path: &str) -> Option<&str> {
    let app = path.strip_prefix("apps/")?.split('/').next()?;
    let stem = path.rsplit('/').next()?.split('.').next()?;
    (!NEXT_FILES.contains(&stem)).then_some(app)
}

fn git_bytes(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// The definitions in one file's text; none when the parse budget cancels it.
fn defs_of(g: &mut Grammars, lang: Lang, src: &[u8]) -> Result<Vec<Def>, Fault> {
    let Some(tree) = g.parse(lang, src)? else {
        return Ok(Vec::new());
    };
    Ok(extract(lang, g.query(lang)?, &tree, src).0)
}

/// Blobs by revision spec (`HEAD:path`, `:path`), all through one `git cat-file --batch`: a git
/// spawn costs ~100 ms on the owner's Windows desktop (S8.3), so one per blob would make a commit
/// of five files wait a second. None for a spec git does not have (a new file at HEAD).
fn blobs(root: &Path, specs: &[String]) -> Result<Vec<Option<Vec<u8>>>, Fault> {
    use std::io::Write;
    use std::process::Stdio;
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Fault::io(root, e))?;
    let input = specs.join("\n") + "\n";
    let mut stdin = child.stdin.take().expect("piped above");
    // Written from a thread: git answers each line as it reads it, and a full stdout pipe would
    // stop it reading the rest while this thread still writes.
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output().map_err(|e| Fault::io(root, e))?;
    let _ = writer.join();
    let (mut rest, mut found) = (&out.stdout[..], Vec::with_capacity(specs.len()));
    for _ in specs {
        let Some(nl) = rest.iter().position(|b| *b == b'\n') else {
            break;
        };
        let header = String::from_utf8_lossy(&rest[..nl]).into_owned();
        rest = &rest[nl + 1..];
        // `<sha> blob <size>`, then the bytes and a newline; or `<spec> missing`.
        match header
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse::<usize>().ok())
        {
            Some(size) if header.contains(" blob ") && rest.len() > size => {
                found.push(Some(rest[..size].to_vec()));
                rest = &rest[size + 1..];
            }
            _ => found.push(None),
        }
    }
    found.resize(specs.len(), None);
    Ok(found)
}

/// The changed files a grammar covers, each with its new text and HEAD's: the index against HEAD
/// with `staged`, else the working tree against HEAD. A new file's old text is empty.
type Change = (String, Lang, Vec<u8>, Vec<u8>);

fn changed(root: &Path, staged: bool) -> Result<Vec<Change>, Fault> {
    let against = if staged { "--cached" } else { "HEAD" };
    let names = git_bytes(
        root,
        &["diff", "--name-only", "-z", "--diff-filter=ACMR", against],
    )
    .ok_or_else(|| Fault::Config(format!("{}: git diff failed", root.display())))?;
    let files: Vec<(String, Lang)> = names
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .filter_map(|rel| Lang::of(&rel).map(|l| (rel, l)))
        .collect();
    let mut specs: Vec<String> = files.iter().map(|(r, _)| format!("HEAD:{r}")).collect();
    if staged {
        specs.extend(files.iter().map(|(r, _)| format!(":{r}")));
    }
    let mut got = blobs(root, &specs)?.into_iter();
    let olds: Vec<Option<Vec<u8>>> = got.by_ref().take(files.len()).collect();
    let mut out = Vec::new();
    for ((rel, lang), old) in files.into_iter().zip(olds) {
        let new = if staged {
            got.next().flatten()
        } else {
            fs::read(root.join(&rel)).ok()
        };
        if let Some(new) = new.filter(|t| t.len() as u64 <= MAX_BYTES) {
            out.push((rel, lang, new, old.unwrap_or_default()));
        }
    }
    Ok(out)
}

/// `code duplicate [--staged] [<repo>] [--budget N]`: each symbol a change adds or alters whose
/// body has the same shape as another symbol's, or whose name and kind another app under `apps/`
/// defines (the turborepo rule). It warns and exits 0: a same-shaped `new()` is often right, and
/// the rules it backs are agent rules the agent reads at commit time (S8.3, spec item 42).
fn run_duplicate(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &["budget"])?;
    let staged = args.has("staged");
    let cfg = config::load()?;
    let name = match args.words.first() {
        Some(w) => named(&cfg, w)?,
        None => match here(&cfg) {
            Some(h) => h,
            // The pre-commit hook runs in every repo it guards, and the vault is not a checkout.
            None if staged => return Ok(0),
            None => {
                return Err(Fault::Usage(format!(
                    "not inside a checkout under {}: name one",
                    cfg.dev.display()
                )));
            }
        },
    };
    let root = cfg.dev.join(&name);
    let mut conn = open_rw(&cfg, &name)?;
    sync(&mut conn, &root)?;
    let mut same = conn.prepare(
        "SELECT s.name, f.path, s.line, f.lang FROM symbols s JOIN files f ON f.id = s.file
         WHERE s.shape = ?1 AND NOT (f.path = ?2 AND s.name = ?3) ORDER BY f.path, s.line",
    )?;
    let mut alike = conn.prepare(
        "SELECT f.path, s.line, f.lang FROM symbols s JOIN files f ON f.id = s.file
         WHERE s.name = ?1 AND s.kind = ?2 AND f.path LIKE 'apps/%' ORDER BY f.path, s.line",
    )?;
    let mut g = Grammars::new();
    let mut found = Vec::new();
    for (rel, lang, new, old) in changed(&root, staged)? {
        let before: HashSet<(String, String, Option<String>)> = defs_of(&mut g, lang, &old)?
            .into_iter()
            .map(|d| (d.name, d.kind, d.shape))
            .collect();
        let fam = family(lang.name());
        let app = app_of(&rel);
        for d in defs_of(&mut g, lang, &new)? {
            if before.contains(&(d.name.clone(), d.kind.clone(), d.shape.clone())) {
                continue;
            }
            let at = format!("{rel}:{} {} {}", d.line, d.kind, d.name);
            let mut copies = Vec::new();
            if let Some(shape) = &d.shape {
                for row in same.query_map(params![shape, rel, d.name], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })? {
                    let (n, p, l, lg) = row?;
                    if family(&lg) == fam {
                        copies.push(format!("the same body as {p}:{l} {n}"));
                    }
                }
            }
            if let Some(a) = app {
                for row in alike.query_map(params![d.name, d.kind], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })? {
                    let (p, l, lg) = row?;
                    if family(&lg) == fam
                        && let Some(b) = app_of(&p).filter(|b| *b != a)
                    {
                        copies.push(format!("also in apps/{b}: {p}:{l}"));
                    }
                }
            }
            let more = copies.len().saturating_sub(MAX_MATCHES);
            for c in copies.into_iter().take(MAX_MATCHES) {
                found.push(format!("duplicate: {at}: {c}"));
            }
            if more > 0 {
                found.push(format!("duplicate: {at}: and {more} more"));
            }
        }
    }
    if found.is_empty() {
        return Ok(0);
    }
    let mut out = Out::new(&args)?;
    out.line(
        "duplicate: a warning, not a block. Each line is a symbol this change adds that the repo \
         already has: reuse the one that exists, or move both to one shared place.",
    );
    for f in &found {
        out.line(f);
    }
    emit(&out.finish());
    Ok(0)
}

/// `knowledge code index [<repo>|--all] [--rebuild]` and `code duplicate`.
pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let (sub, rest) = raw
        .split_first()
        .map_or(("", &[][..]), |(v, r)| (v.as_str(), r));
    match sub {
        "index" => {}
        "duplicate" => return run_duplicate(rest),
        _ => {
            return Err(Fault::Usage(
                "usage: knowledge code index [<repo>|--all] [--rebuild] | code duplicate [--staged] [<repo>] [--budget N]"
                    .to_string(),
            ));
        }
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
