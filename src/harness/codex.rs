//! Codex's translator. Its hook payloads share Claude Code's snake_case fields and its answers take
//! the same `hookSpecificOutput` shape (the Codex spec's F4, confirmed live in S7.0); three things
//! differ:
//! - an edit is one `apply_patch` call whose `tool_input.command` is the patch text, naming any
//!   number of files (F6);
//! - `permissionDecision: "ask"` parses but is not supported: the hook run fails and the tool runs
//!   anyway (S7.0, on 0.171.0). So every `ask` goes out as a `deny`;
//! - the transcript is the rollout file, `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<sid>.jsonl`.
use super::{Adapter, Call, Event, Payload, Tool, Translator, Turn};
use crate::Fault;
use crate::state::{J, obj};
use serde_json::Value;
use std::path::Path;

pub struct Codex;

impl Translator for Codex {
    fn parse(&self, adapter: &Adapter, input: &str) -> Result<Payload, Fault> {
        let v: Value = serde_json::from_str(input)?;
        let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let name = v["hook_event_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let cwd = s(&v["cwd"]);
        let ti = &v["tool_input"];
        let command = s(&ti["command"]);
        let tool = match v["tool_name"].as_str() {
            _ if ti.is_null() => Tool::None,
            None => Tool::None,
            Some("Bash" | "shell" | "exec_command") => Tool::Shell,
            Some("apply_patch") => Tool::Edit,
            Some(t) if t.starts_with("mcp__") => Tool::Mcp(t.to_string()),
            Some(_) => Tool::Other,
        };
        let paths = if tool == Tool::Edit {
            patch_paths(&command, &cwd)
        } else {
            Vec::new()
        };
        Ok(Payload {
            harness: adapter.id.clone(),
            event: adapter.event_of(&name),
            name,
            session: s(&v["session_id"]),
            transcript: v["transcript_path"].as_str().map(str::to_string),
            cwd,
            prompt: v["prompt"].as_str().map(str::to_string),
            call: Call {
                tool,
                command,
                paths,
            },
        })
    }

    fn context(&self, adapter: &Adapter, event: Event, text: &str) -> String {
        obj([(
            "hookSpecificOutput",
            obj([
                ("hookEventName", J::s(adapter.name_of(event))),
                ("additionalContext", J::s(text)),
            ]),
        )])
        .text()
    }

    fn verdict(&self, adapter: &Adapter, action: &str, reason: &str) -> String {
        // Codex cannot ask: an `ask` would fail the hook and let the tool run.
        let reason = if action == "ask" {
            format!("{reason} (an ask rule; Codex cannot ask, so it is a deny here)")
        } else {
            reason.to_string()
        };
        obj([(
            "hookSpecificOutput",
            obj([
                ("hookEventName", J::s(adapter.name_of(Event::PreTool))),
                ("permissionDecision", J::s("deny")),
                ("permissionDecisionReason", J::s(reason)),
            ]),
        )])
        .text()
    }

    fn turns(&self, text: &str) -> Vec<Turn> {
        text.split('\n')
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|o| turn(&o))
            .collect()
    }
}

/// A rollout's `response_item` message from the user or the model, with its text. The user-role
/// messages Codex writes itself (the `AGENTS.md` instructions, `<environment_context>`) are not
/// turns; the developer role (hook context, the system prompt) never is.
fn turn(o: &Value) -> Option<Turn> {
    if o["type"] != "response_item" || o["payload"]["type"] != "message" {
        return None;
    }
    let p = &o["payload"];
    let role = match p["role"].as_str()? {
        "user" => "user",
        "assistant" => "assistant",
        _ => return None,
    };
    let text = p["content"]
        .as_array()?
        .iter()
        .filter(|c| matches!(c["type"].as_str(), Some("input_text" | "output_text")))
        .filter_map(|c| c["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    if text.is_empty()
        || (role == "user"
            && (text.starts_with("# AGENTS.md instructions") || text.starts_with('<')))
    {
        return None;
    }
    Some(Turn {
        role,
        text: text.to_string(),
        at: o["timestamp"].as_str().map(str::to_string),
    })
}

/// The files an `apply_patch` text names, made absolute against the call's `cwd`.
fn patch_paths(patch: &str, cwd: &str) -> Vec<String> {
    const HEADS: [&str; 4] = [
        "*** Add File: ",
        "*** Update File: ",
        "*** Delete File: ",
        "*** Move to: ",
    ];
    let mut out: Vec<String> = Vec::new();
    for line in patch.lines() {
        let Some(rel) = HEADS.iter().find_map(|h| line.strip_prefix(h)) else {
            continue;
        };
        let rel = rel.trim();
        if rel.is_empty() {
            continue;
        }
        let p = Path::new(rel);
        let abs = if p.is_absolute() || cwd.is_empty() {
            p.to_path_buf()
        } else {
            Path::new(cwd).join(p)
        };
        let abs = abs.to_string_lossy().into_owned();
        if !out.contains(&abs) {
            out.push(abs);
        }
    }
    out
}
