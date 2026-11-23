//! knowledge: the engine's main binary (spec D9, K30). Every hook, the doctor and `setup` (Stage 6).
//! The code map is `knowledge-code`, beside it (spec item 44); its verbs are handed off.
#![deny(unsafe_code)]

use knowledge::{
    Fault, capture, compile, config, doctor, emit, events, fetch, graph, hook, hubs, index, mcp,
    memory, note, recorder, records, report, rules, search, setup, usage,
};
use std::process::{Command, ExitCode};

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
  knowledge graph <note> [<other note>] [--json] | <note> --text
  knowledge code index [<repo>|--all] [--rebuild]   (run by knowledge-code)
  knowledge note decision <text> [--question <q>] [--promoted <path>] | note log <text>
  knowledge config [<key>]
  knowledge rules list [--json] | kernel | check [--staged] [--restore] | lock [--seed <file>]
  knowledge rules enable|disable <pack>
  knowledge hook session|prompt|pre-tool|post-edit|post-bash|capture --harness <id> [--pin <version>]
  knowledge doctor [--pretty] [--repair] [--record] [--only <id,...>]
  knowledge doctor --only portable [--json]
  knowledge doctor budget [<transcript.jsonl>] [--json]
  knowledge doctor profile <note>
  knowledge setup --plan [--vault <path>] [--dev <path>] [--machine <text>] [--from-template | --adopt]
                         [--signing] [--skeleton <checkout>]
  knowledge setup --apply <id> [the same options] | --remove --plan | --remove --apply <id>
  knowledge setup --harness claude|codex|opencode|claude-desktop|generic ...   (one harness alone)
  knowledge flush <session-id> [<cwd>] [--transcript <file>] [--date <YYYY-MM-DD>] [--dry-run [--from-start]]
  knowledge compile [--force] [--dry-run]
  knowledge post-bash [--backfill <since>] [--dry-run]
  knowledge records | hubs
  knowledge usage [--month <YYYY-MM>] [--days N] [--projects] [--blocks]
  knowledge mcp   (an MCP server on stdio: find, graph, read and note)";

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

/// Run a code verb in `knowledge-code`, with this process's arguments and stdio. One extra spawn
/// for a code query, so that no hook spawn pays for the grammars (spec item 44).
fn hand_off(argv: &[String]) -> Result<u8, Fault> {
    let exe = knowledge::code_exe()?;
    let status = Command::new(&exe).args(argv).status().map_err(|e| {
        Fault::Config(format!(
            "{}: {e}: `cargo install --path <checkout> --locked` installs it beside knowledge",
            exe.display()
        ))
    })?;
    Ok(status
        .code()
        .and_then(|c| u8::try_from(c).ok())
        .unwrap_or(2))
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (verb, rest) = argv
        .split_first()
        .map_or(("", &[][..]), |(v, r)| (v.as_str(), r));
    report(match verb {
        "--version" | "-V" | "version" => version(),
        "index" => index::run(rest),
        "find" => search::run_find(rest),
        "keep" => capture::run_keep(rest),
        "fetch" => fetch::run(rest),
        "events" => events::run(rest),
        "inject" => search::run_inject(rest),
        "graph" => graph::run(rest),
        "code" => hand_off(&argv),
        "note" => note::run(rest),
        "config" => config::run(rest),
        "rules" => rules::run(rest),
        // A hook never fails the harness: whatever happens inside, it exits 0.
        "hook" => {
            hook::run(rest);
            return ExitCode::SUCCESS;
        }
        "doctor" => doctor::run(rest),
        "post-bash" => recorder::run(rest),
        "compile" => compile::run(rest),
        "flush" => memory::run_flush(rest),
        "records" => records::run(rest),
        "usage" => usage::run(rest),
        "hubs" => hubs::run(rest),
        "setup" => setup::run(rest),
        "mcp" => mcp::run(rest),
        _ => Err(Fault::Usage(USAGE.to_string())),
    })
}
