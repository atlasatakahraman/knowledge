//! knowledge-code: the code map (Stage 8, spec item 44). A binary of its own because the
//! tree-sitter grammars double the exe, and every hook spawn of `knowledge` would pay for them.
//! `knowledge` hands its code verbs here with the same arguments.
#![deny(unsafe_code)]

use knowledge::{Fault, code, report};
use std::process::ExitCode;

const USAGE: &str = "usage:
  knowledge-code --version
  knowledge-code code index [<repo>|--all] [--rebuild]
  knowledge-code find <query> --code [--repo <name>] [--limit N] [--budget N]
  knowledge-code graph <symbol> [<other symbol>] --code [--repo <name>] [--budget N]";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (verb, rest) = argv
        .split_first()
        .map_or(("", &[][..]), |(v, r)| (v.as_str(), r));
    report(match verb {
        "--version" | "-V" | "version" => {
            knowledge::emit(&format!("knowledge-code {}\n", env!("CARGO_PKG_VERSION")));
            Ok(0)
        }
        "code" => code::run(rest),
        "find" => code::run_find(rest),
        "graph" => code::run_graph(rest),
        _ => Err(Fault::Usage(USAGE.to_string())),
    })
}
