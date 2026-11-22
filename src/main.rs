//! knowledge: the engine's one binary (spec D9, K30). Verbs: index, find, inject, graph, hook, doctor.
//! Stable Rust, no async runtime, arguments parsed by hand, and no `unsafe` anywhere in the crate.
#![forbid(unsafe_code)]

mod budget;
mod capture;
mod config;
mod doctor;
mod events;
mod fetch;
mod graph;
mod guard;
mod hook;
mod index;
mod rules;
mod search;
mod vault;

use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage:
  knowledge --version
  knowledge index [--rebuild] [<note.md>...]
  knowledge index --check [--json]
  knowledge index --repo <checkout>
  knowledge find <query> [--limit N] [--kind K|all] [--json]
  knowledge find <query> --captures [--budget N] | --repo [<name>]
  knowledge keep <label> [--q <query>]... [--budget N] < input
  knowledge fetch <url> [--q <query>]... [--budget N] [--allow-private]
  knowledge events <transcript.jsonl> [--json]
  knowledge inject <text> [--format lines|tagged] [--no-passage | --passage-max N --passage-score S]
  knowledge graph <note> [<other note>] [--json]
  knowledge rules list [--json] | kernel | check [--staged] [--restore] | lock [--seed <file>]
  knowledge rules enable|disable <pack>
  knowledge hook prompt|post-edit|pre-tool --harness claude
  knowledge doctor --only portable [--json]
  knowledge doctor budget [<transcript.jsonl>] [--json]
  knowledge doctor profile <note>";

/// Every fallible function returns this ([[Hard-Rules]] § Working preferences: `Fault`, never
/// `Result<_, String>`). Named `Fault` so it never collides with an `Error` already in scope.
#[derive(Debug)]
pub enum Fault {
    Io(String, std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    Config(String),
    Usage(String),
    Net(String),
}

impl Fault {
    pub fn io(path: &Path, e: std::io::Error) -> Fault {
        Fault::Io(path.display().to_string(), e)
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Io(path, e) => write!(f, "{path}: {e}"),
            Fault::Sql(e) => write!(f, "index: {e}"),
            Fault::Json(e) => write!(f, "json: {e}"),
            Fault::Config(m) | Fault::Usage(m) | Fault::Net(m) => f.write_str(m),
        }
    }
}

impl From<rusqlite::Error> for Fault {
    fn from(e: rusqlite::Error) -> Self {
        Fault::Sql(e)
    }
}

impl From<serde_json::Error> for Fault {
    fn from(e: serde_json::Error) -> Self {
        Fault::Json(e)
    }
}

/// Positional words, `--flag` switches and `--flag value` options. `valued` names the options
/// that take a value; any other `--x` is a switch.
pub struct Args {
    pub words: Vec<String>,
    flags: HashMap<String, String>,
}

impl Args {
    pub fn parse(raw: &[String], valued: &[&str]) -> Result<Args, Fault> {
        let mut words = Vec::new();
        let mut flags = HashMap::new();
        let mut it = raw.iter();
        while let Some(a) = it.next() {
            match a.strip_prefix("--") {
                Some(name) if valued.contains(&name) => {
                    let v = it
                        .next()
                        .ok_or_else(|| Fault::Usage(format!("--{name} needs a value")))?;
                    flags.insert(name.to_string(), v.clone());
                }
                Some(name) => {
                    flags.insert(name.to_string(), String::new());
                }
                None => words.push(a.clone()),
            }
        }
        Ok(Args { words, flags })
    }

    pub fn has(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.flags.get(name).map(String::as_str)
    }

    /// A numeric option, or `default` when absent.
    pub fn num<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, Fault> {
        match self.get(name) {
            None => Ok(default),
            Some(v) => v
                .parse()
                .map_err(|_| Fault::Usage(format!("--{name}: not a number: {v}"))),
        }
    }
}

/// Write to stdout, ignoring a closed pipe: `knowledge find x | head` must not abort.
pub fn emit(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

/// The version, and the SQLite build's FTS5 flag: an index without FTS5 cannot search, and the
/// `bundled` feature is what promises it. The stage check fails when this line says "off".
fn version() -> Result<u8, Fault> {
    let db = rusqlite::Connection::open_in_memory()?;
    let sqlite: String = db.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
    let mut stmt = db.prepare("PRAGMA compile_options")?;
    let fts5 = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .filter_map(Result::ok)
        .any(|o| o == "ENABLE_FTS5");
    emit(&format!(
        "knowledge {}\nSQLite {sqlite}, FTS5 {}\n",
        env!("CARGO_PKG_VERSION"),
        if fts5 { "on" } else { "off" }
    ));
    Ok(if fts5 { 0 } else { 1 })
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (verb, rest) = argv
        .split_first()
        .map_or(("", &[][..]), |(v, r)| (v.as_str(), r));
    let result = match verb {
        "--version" | "-V" | "version" => version(),
        "index" => index::run(rest),
        "find" => search::run_find(rest),
        "keep" => capture::run_keep(rest),
        "fetch" => fetch::run(rest),
        "events" => events::run(rest),
        "inject" => search::run_inject(rest),
        "graph" => graph::run(rest),
        "rules" => rules::run(rest),
        // A hook never fails the harness: whatever happens inside, it exits 0.
        "hook" => {
            hook::run(rest);
            return ExitCode::SUCCESS;
        }
        "doctor" => doctor::run(rest),
        _ => Err(Fault::Usage(USAGE.to_string())),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(f) => {
            eprintln!("knowledge: {f}");
            ExitCode::from(2)
        }
    }
}
