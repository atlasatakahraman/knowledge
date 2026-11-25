//! `knowledge events <transcript> [--json]` (Stage 4, K20): the session events the flush hands
//! its summariser, read from the transcript alone. Written from what context-mode's rows held
//! against the same transcripts (evidence/events-recon.mjs), never from its code (ELv2):
//! - decision: every AskUserQuestion answer, as "Q: … A: …" (context-mode kept the first only);
//! - plan: ExitPlanMode's plan, and a Write or Edit under a plans folder;
//! - rejected-approach: a tool call the user refused, and a prompt sentence turning something down;
//! - constraint and decision: a prompt sentence holding one of the words in the tables below;
//! - intent: one label per prompt, from its first word.
//!
//! The tables are the rules: a word list per kind, matched lower-cased inside one sentence of what
//! the user typed. A sentence yields at most one event, the first table in RULES that matches.
use crate::{Args, Fault, emit};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

const REJECTED: &[&str] = &[
    "instead of",
    "rather than",
    "don't use",
    "do not use",
    "not that",
    "revert",
    "undo ",
    "that's wrong",
    "is wrong",
    "drop the",
    "drop it",
    "stop ",
];
const CONSTRAINT: &[&str] = &[
    "never ",
    "always ",
    "must ",
    "don't ",
    "do not ",
    "only ",
    "without asking",
    "not allowed",
    "required",
    "no more than",
    "at most",
    "at least",
];
const DECISION: &[&str] = &[
    "let's ",
    "lets ",
    "go with",
    "we'll ",
    "decided",
    "decide ",
    "approve",
    "from now on",
    "switch to",
    "keep the",
    "use the",
];
/// In order: a sentence matching two tables counts once, for the first.
const RULES: [(&str, &[&str]); 3] = [
    ("rejected-approach", REJECTED),
    ("constraint", CONSTRAINT),
    ("decision", DECISION),
];
/// A prompt opening with one of these is work to do; anything else is a question to look into.
const IMPLEMENT: &[&str] = &[
    "implement",
    "add",
    "build",
    "create",
    "fix",
    "write",
    "make",
    "update",
    "change",
    "remove",
    "delete",
    "refactor",
    "rename",
    "commit",
    "push",
    "run",
    "execute",
    "do",
    "continue",
    "ship",
    "install",
    "set",
    "move",
    "apply",
    "go",
];
/// Folders whose writes are plans.
const PLAN_DIRS: &[&str] = &["/40-plans/", "/plans/"];
/// The flush cuts each event to 400 chars; so does this.
const MAX: usize = 400;

pub struct Event {
    pub category: &'static str,
    pub data: String,
}

fn clip(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match s.char_indices().nth(MAX) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s,
    }
}

/// What the user typed: fenced code, pasted blocks and quoted lines removed.
fn typed(text: &str) -> String {
    let mut out = String::new();
    let mut fence = false;
    let mut pasted = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            fence = !fence;
            continue;
        }
        if t.starts_with("<pasted_content") {
            pasted = true;
        }
        if !fence && !pasted && !t.starts_with('>') {
            out.push_str(line);
            out.push('\n');
        }
        if t.contains("</pasted_content>") {
            pasted = false;
        }
    }
    out
}

fn sentences(text: &str) -> impl Iterator<Item = &str> {
    text.split(['.', '!', '?', '\n', ';'])
        .map(str::trim)
        .filter(|s| (12..=400).contains(&s.len()))
}

/// A user record that a person typed: not a tool result, not a command echo, a hook's
/// injection, a compaction summary or an interruption marker.
fn prompt_text(o: &Value) -> Option<String> {
    if o["type"] != "user" || o["isMeta"] == true || o["isCompactSummary"] == true {
        return None;
    }
    let c = &o["message"]["content"];
    let text = match c {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => {
            if blocks.iter().any(|b| b["type"] == "tool_result") {
                return None;
            }
            blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => return None,
    };
    let t = text.trim_start();
    (!t.is_empty()
        && !t.starts_with('<')
        && !t.starts_with("Caveat:")
        && !t.starts_with("[Request interrupted"))
    .then_some(text)
}

fn intent(text: &str) -> &'static str {
    let first = text
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .find(|w| {
            !matches!(
                w.as_str(),
                "please"
                    | "ok"
                    | "okay"
                    | "now"
                    | "then"
                    | "so"
                    | "and"
                    | "can"
                    | "could"
                    | "you"
                    | ""
            )
        });
    if first.is_some_and(|w| IMPLEMENT.contains(&w.as_str())) {
        "implement"
    } else {
        "investigate"
    }
}

fn is_plan_path(p: &str) -> bool {
    let p = p.replace('\\', "/").to_lowercase();
    PLAN_DIRS.iter().any(|d| p.contains(d)) && p.ends_with(".md")
}

fn first_heading(text: &str) -> &str {
    text.lines()
        .find_map(|l| l.strip_prefix("# "))
        .unwrap_or("")
        .trim()
}

/// Every event in a transcript, in the order its records appear. Pure: the same transcript always
/// gives the same list, so the flush's watermark is a count.
pub fn parse(jsonl: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let mut calls: HashMap<String, (String, Value)> = HashMap::new();
    // A plan file is named once, at its first write: one plan edited 40 times in a session is one fact.
    let mut plans: HashSet<String> = HashSet::new();
    for line in jsonl.lines() {
        let Ok(o) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(text) = prompt_text(&o) {
            let typed = typed(&text);
            out.push(Event {
                category: "intent",
                data: intent(&typed).into(),
            });
            for s in sentences(&typed) {
                let lower = s.to_lowercase();
                if let Some((kind, _)) = RULES
                    .iter()
                    .find(|(_, words)| words.iter().any(|w| lower.contains(w)))
                {
                    out.push(Event {
                        category: kind,
                        data: clip(s),
                    });
                }
            }
        }
        let r = &o["toolUseResult"];
        if let (Some(qs), Some(answers)) = (r["questions"].as_array(), r["answers"].as_object()) {
            for q in qs {
                let Some(question) = q["question"].as_str() else {
                    continue;
                };
                if let Some(a) = answers.get(question) {
                    let a = a.as_str().map_or_else(|| a.to_string(), str::to_string);
                    out.push(Event {
                        category: "decision",
                        data: clip(&format!("Q: {question} A: {a}")),
                    });
                }
            }
        }
        let Some(blocks) = o["message"]["content"].as_array() else {
            continue;
        };
        for b in blocks {
            match b["type"].as_str() {
                Some("tool_use") => {
                    let name = b["name"].as_str().unwrap_or("").to_string();
                    let input = &b["input"];
                    if name == "ExitPlanMode"
                        && let Some(plan) = input["plan"].as_str()
                    {
                        out.push(Event {
                            category: "plan",
                            data: clip(plan),
                        });
                    }
                    if matches!(name.as_str(), "Write" | "Edit")
                        && let Some(p) = input["file_path"].as_str()
                        && is_plan_path(p)
                        && plans.insert(p.replace('\\', "/").to_lowercase())
                    {
                        let head = first_heading(input["content"].as_str().unwrap_or(""));
                        let verb = if name == "Write" { "wrote" } else { "edited" };
                        out.push(Event {
                            category: "plan",
                            data: clip(&format!(
                                "{verb} {p}{}{head}",
                                if head.is_empty() { "" } else { ": " }
                            )),
                        });
                    }
                    if let Some(id) = b["id"].as_str() {
                        calls.insert(id.to_string(), (name, input.clone()));
                    }
                }
                Some("tool_result") => {
                    let text = match &b["content"] {
                        Value::String(s) => s.clone(),
                        Value::Array(a) => a
                            .iter()
                            .filter_map(|x| x["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        _ => String::new(),
                    };
                    let refused = text.contains("doesn't want to proceed")
                        || text.contains("tool use was rejected");
                    let Some((name, input)) =
                        b["tool_use_id"].as_str().and_then(|id| calls.get(id))
                    else {
                        continue;
                    };
                    if !refused || name == "AskUserQuestion" {
                        continue;
                    }
                    // The reason the user gave, when the refusal carries one.
                    let why = text
                        .split_once("the user said:")
                        .map(|(_, w)| w.trim())
                        .unwrap_or("");
                    let what = ["file_path", "command", "url", "plan"]
                        .iter()
                        .find_map(|k| input[*k].as_str())
                        .unwrap_or("");
                    let what: String = what.chars().take(120).collect();
                    out.push(Event {
                        category: "rejected-approach",
                        data: clip(&format!(
                            "{name} {what}{}{why}",
                            if why.is_empty() { "" } else { " — " }
                        )),
                    });
                }
                _ => {}
            }
        }
    }
    out
}

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &[])?;
    let Some(path) = args.words.first() else {
        return Err(Fault::Usage(
            "knowledge events <transcript.jsonl> [--json]".into(),
        ));
    };
    let path = std::path::Path::new(path);
    let bytes = std::fs::read(path).map_err(|e| Fault::io(path, e))?;
    let events = parse(&String::from_utf8_lossy(&bytes));
    if args.has("json") {
        let items: Vec<Value> = events
            .iter()
            .map(|e| serde_json::json!({"category": e.category, "data": e.data}))
            .collect();
        emit(&format!("{}\n", Value::Array(items)));
    } else {
        let mut out = String::new();
        for e in &events {
            out.push_str(&format!("[{}] {}\n", e.category, e.data));
        }
        emit(&out);
    }
    Ok(0)
}
