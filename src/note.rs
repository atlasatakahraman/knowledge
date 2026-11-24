//! `knowledge note decision|log <text>` (spec T1): the memory write for a harness that has a shell
//! and no hooks to capture with, and the verb behind the MCP server's `note` tool.
//!   decision  one line in 50-Ops/decisions.jsonl, in the shape brain-ledger's `add` writes (tier 1,
//!             `src: manual:<ms>`), so the session start counts it and `/brain new decision
//!             --ledger` promotes it like a captured one.
//!   log       one bullet in today's session note (the rollover's day), under its own heading as a
//!             flush writes one, so it never reads as part of the section above it.
use crate::config;
use crate::state::{self, J, obj};
use crate::{Args, Fault, emit};

const USAGE: &str =
    "knowledge note decision <text> [--question <q>] [--promoted <vault path> | not-a-decision]
  knowledge note log <text>";

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &["question", "promoted"])?;
    let [kind, text] = args.words.as_slice() else {
        return Err(Fault::Usage(USAGE.into()));
    };
    // One line: a newline in a log bullet would start a heading or a list of its own.
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(Fault::Usage(USAGE.into()));
    }
    let cfg = config::load()?;
    let now = state::now_ms();
    match kind.as_str() {
        "decision" => {
            let mut e = obj([
                ("tier", J::n(1)),
                ("sid", J::Null),
                ("src", J::s(format!("manual:{now}"))),
                ("question", J::s(args.get("question").unwrap_or_default())),
                ("answer", J::s(&text)),
            ]);
            if let Some(p) = args.get("promoted") {
                // A typo here would mark the decision promoted to a note that does not exist.
                if p != "not-a-decision" && !cfg.vault.join(p).is_file() {
                    return Err(Fault::Usage(format!(
                        "--promoted {p}: no such note in {}",
                        cfg.vault.display()
                    )));
                }
                e.set("promoted", J::s(p));
            }
            if state::ledger_add(&cfg, vec![e]) == 0 {
                return Err(Fault::Config(format!(
                    "{}: not written",
                    state::ledger_file(&cfg).display()
                )));
            }
            emit(&format!(
                "added to {}\n",
                state::native(&state::ledger_file(&cfg))
            ));
        }
        "log" if args.get("question").is_none() && args.get("promoted").is_none() => {
            let file = state::note_path(&cfg, now);
            let block = format!(
                "\n### {} · note · {}\n\n- {text}\n",
                state::hm(&state::local(now)),
                cfg.os
            );
            state::append_note(&cfg, &file, now, &block).map_err(|e| Fault::io(&file, e))?;
            emit(&format!("added to {}\n", state::native(&file)));
        }
        _ => return Err(Fault::Usage(USAGE.into())),
    }
    Ok(0)
}
