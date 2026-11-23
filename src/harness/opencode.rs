//! opencode's translator. opencode's hooks are JavaScript run inside it, so its plugin
//! (`opencode/knowledge.ts`) does the one thing JavaScript must: it sends each hook here as a
//! canonical payload, the same snake_case fields Claude Code and Codex send, with `tool_input` as
//! opencode passed it. The answers are plain JSON the plugin reads: `{"context"}` and
//! `{"decision", "reason"}`.
//!
//! As read from opencode 2.2.1's binary (Context7's docs, at 2.1.2, name the patch tool
//! `apply_patch`; the binary wins): `shell {command}`, `edit {path, oldString, newString}`,
//! `write {path, content}`, and `patch {patchText}` in `apply_patch`'s format. A path may be
//! relative to the session's directory. A hook cannot ask, so an `ask` goes out as a deny.
//!
//! There is no transcript file: sessions are rows in opencode's database, and the adapter's
//! `export` command writes one out as JSON (`{info, messages}`) for the flush.
use super::{Adapter, Call, Event, Payload, Tool, Translator, Turn};
use crate::Fault;
use crate::state::{J, obj};
use serde_json::Value;
use std::path::Path;

pub struct Opencode;

impl Translator for Opencode {
    fn parse(&self, adapter: &Adapter, input: &str) -> Result<Payload, Fault> {
        let v: Value = serde_json::from_str(input)?;
        let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let name = v["hook_event_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let cwd = s(&v["cwd"]);
        let ti = &v["tool_input"];
        let tool = match v["tool_name"].as_str() {
            _ if ti.is_null() => Tool::None,
            None => Tool::None,
            Some("shell") => Tool::Shell,
            Some("edit" | "write" | "patch") => Tool::Edit,
            Some(_) => Tool::Other,
        };
        let paths = match v["tool_name"].as_str() {
            Some("patch") => super::codex::patch_paths(&s(&ti["patchText"]), &cwd),
            Some("edit" | "write") => {
                let p = s(&ti["path"]);
                if p.is_empty() {
                    Vec::new()
                } else if Path::new(&p).is_absolute() || cwd.is_empty() {
                    vec![p]
                } else {
                    vec![Path::new(&cwd).join(p).to_string_lossy().into_owned()]
                }
            }
            _ => Vec::new(),
        };
        Ok(Payload {
            harness: adapter.id.clone(),
            event: adapter.event_of(&name),
            name,
            session: s(&v["session_id"]),
            transcript: None,
            cwd,
            prompt: v["prompt"].as_str().map(str::to_string),
            call: Call {
                tool,
                command: s(&ti["command"]),
                paths,
            },
        })
    }

    fn context(&self, _: &Adapter, _: Event, text: &str) -> String {
        obj([("context", J::s(text))]).text()
    }

    fn verdict(&self, _: &Adapter, action: &str, reason: &str) -> String {
        let reason = if action == "ask" {
            format!("{reason} (an ask rule; an opencode hook cannot ask, so it is a deny here)")
        } else {
            reason.to_string()
        };
        obj([("decision", J::s("deny")), ("reason", J::s(reason))]).text()
    }

    /// `opencode session export`: `user` messages carry `text`; an `assistant` message's
    /// `content` holds reasoning, tool and text parts, and only the text parts are the reply.
    fn turns(&self, text: &str) -> Vec<Turn> {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };
        let Some(messages) = v["messages"].as_array() else {
            return Vec::new();
        };
        messages
            .iter()
            .filter_map(|m| {
                let (role, body) = match m["type"].as_str()? {
                    "user" => ("user", m["text"].as_str()?.to_string()),
                    "assistant" => (
                        "assistant",
                        m["content"]
                            .as_array()?
                            .iter()
                            .filter(|p| p["type"] == "text")
                            .filter_map(|p| p["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                    _ => return None,
                };
                let body = body.trim();
                (!body.is_empty()).then(|| Turn {
                    role,
                    text: body.to_string(),
                    at: m["time"]["created"].as_i64().map(crate::state::iso),
                })
            })
            .collect()
    }
}
