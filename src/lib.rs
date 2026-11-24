//! knowledge: the engine, as a library its two binaries share (spec D9, K30, item 44). `knowledge`
//! runs every hook, the doctor and `setup`; `knowledge-code` holds the code map, whose grammars
//! double the exe and would slow every hook spawn. Stable Rust, no async runtime, arguments parsed
//! by hand. `unsafe` is denied everywhere except the one Windows call in `win.rs` (owner,
//! 2026-11-22: a detached child must not hold the hook's pipe).
#![deny(unsafe_code)]

pub mod budget;
pub mod capture;
pub mod code;
pub mod compile;
pub mod config;
pub mod doctor;
pub mod events;
pub mod fetch;
pub mod graph;
pub mod guard;
pub mod harness;
pub mod health;
pub mod hook;
pub mod hubs;
pub mod index;
pub mod instructions;
pub mod ledger;
pub mod linkcheck;
pub mod mcp;
pub mod memory;
pub mod note;
pub mod recorder;
pub mod records;
pub mod rules;
pub mod search;
pub mod session;
pub mod setup;
pub mod state;
pub mod usage;
pub mod vault;
#[cfg(windows)]
mod win;

use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::Path;

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

/// `knowledge-code` beside this binary: where the code verbs live (spec item 44).
pub fn code_exe() -> Result<std::path::PathBuf, Fault> {
    let exe = std::env::current_exe().map_err(|e| Fault::Config(format!("current_exe: {e}")))?;
    Ok(exe.with_file_name(format!("knowledge-code{}", std::env::consts::EXE_SUFFIX)))
}

/// The process's exit code for a fault: a usage or setup fault is 2, as `main` returns it.
pub fn report(result: Result<u8, Fault>) -> std::process::ExitCode {
    match result {
        Ok(code) => std::process::ExitCode::from(code),
        Err(f) => {
            eprintln!("knowledge: {f}");
            std::process::ExitCode::from(2)
        }
    }
}
