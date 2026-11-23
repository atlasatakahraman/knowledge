//! Claude Code's translator: the hook payload's snake_case fields in, `hookSpecificOutput` out.
use super::{Adapter, Call, Event, Payload, Tool, Translator, Turn};
use crate::Fault;
use crate::state::{J, obj};
use serde_json::Value;

pub struct Claude;

impl Translator for Claude {
    fn parse(&self, adapter: &Adapter, input: &str) -> Result<Payload, Fault> {
        let v: Value = serde_json::from_str(input)?;
        let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let name = v["hook_event_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let ti = &v["tool_input"];
        // A payload with no `tool_input` names no call: the guard has nothing to judge.
        let tool = match v["tool_name"].as_str() {
            _ if ti.is_null() => Tool::None,
            None => Tool::None,
            Some("Bash") => Tool::Shell,
            Some("PowerShell") => Tool::PowerShell,
            Some("Edit" | "Write" | "NotebookEdit" | "MultiEdit") => Tool::Edit,
            Some(t) if t.starts_with("mcp__") => Tool::Mcp(t.to_string()),
            Some(_) => Tool::Other,
        };
        let paths = [&ti["file_path"], &ti["notebook_path"]]
            .into_iter()
            .map(s)
            .find(|p| !p.is_empty())
            .into_iter()
            .collect();
        Ok(Payload {
            harness: adapter.id.clone(),
            event: adapter.event_of(&name),
            name,
            session: s(&v["session_id"]),
            transcript: v["transcript_path"].as_str().map(str::to_string),
            cwd: s(&v["cwd"]),
            prompt: v["prompt"].as_str().map(str::to_string),
            call: Call {
                tool,
                command: s(&ti["command"]),
                paths,
            },
        })
    }

    fn context(&self, adapter: &Adapter, event: Event, text: &str) -> String {
        out(obj([
            ("hookEventName", J::s(adapter.name_of(event))),
            ("additionalContext", J::s(text)),
        ]))
    }

    fn verdict(&self, adapter: &Adapter, action: &str, reason: &str) -> String {
        out(obj([
            ("hookEventName", J::s(adapter.name_of(Event::PreTool))),
            ("permissionDecision", J::s(action)),
            ("permissionDecisionReason", J::s(reason)),
        ]))
    }

    fn turns(&self, text: &str) -> Vec<Turn> {
        text.split('\n')
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|o| Self::turn(&o))
            .collect()
    }
}

impl Claude {
    /// One transcript line's turn: a `user` or `assistant` record with text.
    fn turn(o: &Value) -> Option<Turn> {
        let role = match o["type"].as_str()? {
            "user" => "user",
            "assistant" => "assistant",
            _ => return None,
        };
        let body = match &o["message"]["content"] {
            Value::Array(parts) => parts
                .iter()
                .filter(|p| p["type"] == "text")
                .map(|p| crate::memory::js_string(&p["text"]))
                .collect::<Vec<_>>()
                .join("\n"),
            Value::String(s) => s.clone(),
            _ => String::new(),
        };
        let text = body.trim();
        (!text.is_empty()).then(|| Turn {
            role,
            text: text.to_string(),
            at: o["timestamp"].as_str().map(str::to_string),
        })
    }
}

/// Keys in the order Claude Code's docs list them.
fn out(inner: J) -> String {
    obj([("hookSpecificOutput", inner)]).text()
}
