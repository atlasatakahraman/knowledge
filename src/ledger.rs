//! `knowledge ledger list | promote <src> <path|not-a-decision>` (Stage 9): the decision ledger's
//! reader and its promote, ported from brain-ledger.mjs so the `/brain` skill runs no bun. `add` is
//! `knowledge note decision`. `list` prints what the bun tool printed, byte for byte.
use crate::config;
use crate::state::{self, J};
use crate::{Args, Fault, emit};
use std::fs;

const USAGE: &str =
    "knowledge ledger list | promote <src> <vault-relative ADR path | not-a-decision>";

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &[])?;
    let cfg = config::load()?;
    match args.words.as_slice() {
        [cmd] if cmd == "list" => {
            let open: Vec<J> = state::ledger_read(&cfg)
                .into_iter()
                .filter(|e| e.get("promoted") == Some(&J::Null))
                .collect();
            let mut out = String::new();
            for e in &open {
                out.push_str(&format!(
                    "{}\n  Q: {}\n  A: {}",
                    js(e.get("src")),
                    js(e.get("question")),
                    js(e.get("answer"))
                ));
                if truthy(e.get("note")) {
                    out.push_str(&format!("\n  note: {}", js(e.get("note"))));
                }
                if let Some(J::Arr(o)) = e.get("options")
                    && !o.is_empty()
                {
                    let items: Vec<String> = o.iter().map(join_item).collect();
                    out.push_str(&format!("\n  offered: {}", items.join(" · ")));
                }
                out.push_str("\n\n");
            }
            out.push_str(&format!("{} unpromoted\n", open.len()));
            emit(&out);
            Ok(0)
        }
        [cmd, src, to] if cmd == "promote" => {
            // A typo here would mark the decision promoted to a note that does not exist.
            if to != "not-a-decision" && !cfg.vault.join(to).is_file() {
                return Err(Fault::Usage(format!(
                    "{to}: no such note in {}",
                    cfg.vault.display()
                )));
            }
            let file = state::ledger_file(&cfg);
            let done = state::ledger_locked(|| promote(&file, src, to)).ok_or_else(|| {
                Fault::Config(format!("{}: another writer holds it", file.display()))
            })??;
            emit(&if done {
                "updated\n".to_string()
            } else {
                format!("no entry with src {src}\n")
            });
            Ok(if done { 0 } else { 1 })
        }
        _ => Err(Fault::Usage(USAGE.into())),
    }
}

/// Set `promoted` on the first entry with this `src`, in its place, and write the file back
/// through a temporary file and a rename. Every other line is kept byte for byte.
fn promote(file: &std::path::Path, src: &str, to: &str) -> Result<bool, Fault> {
    let text = fs::read_to_string(file).map_err(|e| Fault::io(file, e))?;
    let mut done = false;
    let lines: Vec<String> = text
        .split_inclusive('\n')
        .map(|line| {
            if done {
                return line.to_string();
            }
            let body = line.trim_end_matches(['\n', '\r']);
            let Ok(mut e) = serde_json::from_str::<J>(body) else {
                return line.to_string();
            };
            if e.get("src").and_then(J::str) != Some(src) {
                return line.to_string();
            }
            done = true;
            e.set("promoted", J::s(to));
            format!("{}{}", e.text(), &line[body.len()..])
        })
        .collect();
    if done {
        let tmp = file.with_extension("jsonl.promoting");
        fs::write(&tmp, lines.concat()).map_err(|e| Fault::io(&tmp, e))?;
        fs::rename(&tmp, file).map_err(|e| Fault::io(file, e))?;
    }
    Ok(done)
}

/// JavaScript's String(v) inside a template literal: what the bun tool printed for each field.
fn js(v: Option<&J>) -> String {
    match v {
        None => "undefined".into(),
        Some(J::Null) => "null".into(),
        Some(J::Bool(b)) => b.to_string(),
        Some(J::Num(n)) => n.to_string(),
        Some(J::Str(s)) => s.clone(),
        Some(J::Arr(a)) => a.iter().map(join_item).collect::<Vec<_>>().join(","),
        Some(J::Obj(_)) => "[object Object]".into(),
    }
}

/// Array.join's view of one element: null becomes "".
fn join_item(v: &J) -> String {
    match v {
        J::Null => String::new(),
        v => js(Some(v)),
    }
}

fn truthy(v: Option<&J>) -> bool {
    match v {
        None | Some(J::Null) | Some(J::Bool(false)) => false,
        Some(J::Str(s)) => !s.is_empty(),
        Some(J::Num(n)) => n.as_f64() != Some(0.0),
        _ => true,
    }
}
