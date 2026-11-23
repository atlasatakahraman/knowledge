//! Claude Code's translator: the hook payload's snake_case fields in, `hookSpecificOutput` out.
use super::{Adapter, Call, Event, Payload, Tool, Translator};
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
        let path = [&ti["file_path"], &ti["notebook_path"]]
            .into_iter()
            .map(s)
            .find(|p| !p.is_empty())
            .unwrap_or_default();
        Ok(Payload {
            event: adapter.event_of(&name),
            name,
            session: s(&v["session_id"]),
            transcript: v["transcript_path"].as_str().map(str::to_string),
            cwd: s(&v["cwd"]),
            prompt: v["prompt"].as_str().map(str::to_string),
            call: Call {
                tool,
                command: s(&ti["command"]),
                path,
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
}

/// Keys in the order Claude Code's docs list them.
fn out(inner: J) -> String {
    obj([("hookSpecificOutput", inner)]).text()
}
