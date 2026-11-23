//! The adapter layer (spec D13, K41): what each harness is, as data compiled into the binary, and
//! for a harness with hooks one translator, its payload in and its answer shape out. The hook verbs
//! see only [`Payload`] and [`Event`], never a harness name (K26).
//!
//! An adapter is `adapters/<id>.json`, embedded with `include_str!`: a changed adapter is a new
//! binary, never a file an agent could edit. Its `events` table names the harness's own events for
//! each abstract one. The spec's six events, with `end` split in two because the engine treats them
//! differently: a turn's end (`stop`, a throttled flush) and the session's (`session-end`, a flush
//! then the records commit). `post-tool` is the spec's `post-edit`; it also carries the shell call
//! that `post-bash` records.
use crate::Fault;
use crate::instructions::Form;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

mod claude;
mod codex;

const ADAPTERS: [&str; 4] = [
    include_str!("../../adapters/claude.json"),
    include_str!("../../adapters/codex.json"),
    include_str!("../../adapters/opencode.json"),
    include_str!("../../adapters/generic.json"),
];

#[derive(Deserialize)]
pub struct Adapter {
    pub id: String,
    /// The harness as a person names it: "Claude Code", "Codex".
    #[serde(default)]
    pub name: String,
    /// The translator's name; none for a harness without hooks.
    translator: Option<String>,
    #[serde(default)]
    events: BTreeMap<String, Vec<String>>,
    /// The global instructions file the block goes into, `~/` for the home folder; none for
    /// `generic`, whose file `setup --instructions` names.
    instructions: Option<Instructions>,
    /// The headless run the flush summarizes through; none for a harness without one.
    pub runner: Option<Runner>,
}

/// A one-shot model call: the prompt goes in on stdin, and `reply` says how to read stdout.
#[derive(Deserialize)]
pub struct Runner {
    pub argv: Vec<String>,
    pub reply: Reply,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Reply {
    /// The reply is stdout as printed.
    Text,
    /// `codex exec --json`: the last `item.completed` whose item is an `agent_message`.
    CodexJsonl,
}

#[derive(Deserialize)]
struct Instructions {
    global: String,
    form: Form,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    SessionStart,
    Prompt,
    PreTool,
    PostTool,
    PreCompact,
    Stop,
    SessionEnd,
}

const EVENTS: [(Event, &str); 7] = [
    (Event::SessionStart, "session-start"),
    (Event::Prompt, "prompt"),
    (Event::PreTool, "pre-tool"),
    (Event::PostTool, "post-tool"),
    (Event::PreCompact, "pre-compact"),
    (Event::Stop, "stop"),
    (Event::SessionEnd, "session-end"),
];

/// One hook call, harness-neutral. Absent fields are empty, as the bun hooks read them.
#[derive(Default)]
pub struct Payload {
    /// The adapter's id: the flush reads the transcript and runs the model through it.
    pub harness: String,
    /// The harness's own name for the event, for the event log; "unknown" when it sent none.
    pub name: String,
    pub event: Option<Event>,
    /// As sent: `state::clean_sid` cleans it where it becomes a file name.
    pub session: String,
    pub transcript: Option<String>,
    pub cwd: String,
    pub prompt: Option<String>,
    pub call: Call,
}

/// A tool call: the guard judges `tool`, `post-edit` reads `paths`, `post-bash` reads `command`.
/// One edit can name several files (Codex's `apply_patch`).
#[derive(Default)]
pub struct Call {
    pub tool: Tool,
    pub command: String,
    pub paths: Vec<String>,
}

/// One conversation turn from a transcript: `user` or `assistant`, its text, and its time.
pub struct Turn {
    pub role: &'static str,
    pub text: String,
    pub at: Option<String>,
}

#[derive(Default, PartialEq, Eq, Debug)]
pub enum Tool {
    /// No tool in the payload.
    #[default]
    None,
    Shell,
    PowerShell,
    Edit,
    /// An MCP tool, by its full name.
    Mcp(String),
    Other,
}

/// A harness's side of a hook: its payload read, and its context and verdict shapes written.
pub trait Translator {
    fn parse(&self, adapter: &Adapter, input: &str) -> Result<Payload, Fault>;
    /// Text the harness adds to the model's context, answering `event`.
    fn context(&self, adapter: &Adapter, event: Event, text: &str) -> String;
    /// The guard's answer to a pre-tool call: `deny` or `ask`, and why.
    fn verdict(&self, adapter: &Adapter, action: &str, reason: &str) -> String;
    /// The real conversation turns in a transcript file's text, in order.
    fn turns(&self, text: &str) -> Vec<Turn>;
}

impl Reply {
    /// The model's reply in a runner's stdout.
    pub fn of(self, stdout: &str) -> String {
        match self {
            Reply::Text => stdout.to_string(),
            Reply::CodexJsonl => stdout
                .lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter(|v| v["type"] == "item.completed" && v["item"]["type"] == "agent_message")
                .filter_map(|v| v["item"]["text"].as_str().map(str::to_string))
                .next_back()
                .unwrap_or_default(),
        }
    }
}

/// Every adapter the binary carries.
pub fn all() -> Vec<Adapter> {
    ADAPTERS
        .iter()
        .filter_map(|a| serde_json::from_str::<Adapter>(a).ok())
        .collect()
}

/// The adapter named `id`, if the binary carries one.
pub fn adapter(id: &str) -> Option<Adapter> {
    all().into_iter().find(|a| a.id == id)
}

impl Adapter {
    /// The global instructions file, `~/` expanded, and the block's form.
    pub fn instructions(&self) -> Option<(PathBuf, Form)> {
        let i = self.instructions.as_ref()?;
        let rel = i.global.strip_prefix("~/").unwrap_or(&i.global);
        Some((crate::state::home().join(rel), i.form))
    }

    pub fn translator(&self) -> Option<&'static dyn Translator> {
        match self.translator.as_deref()? {
            "claude" => Some(&claude::Claude),
            "codex" => Some(&codex::Codex),
            _ => None,
        }
    }

    /// The global instructions file as the adapter writes it (`~/...`), and its form.
    pub fn instructions_label(&self) -> Option<(&str, Form)> {
        self.instructions
            .as_ref()
            .map(|i| (i.global.as_str(), i.form))
    }

    /// The abstract event a harness event name stands for.
    pub fn event_of(&self, name: &str) -> Option<Event> {
        EVENTS.iter().find_map(|(e, key)| {
            self.events
                .get(*key)
                .is_some_and(|names| names.iter().any(|n| n == name))
                .then_some(*e)
        })
    }

    /// The harness's first name for an abstract event: what its answers are labelled with.
    pub fn name_of(&self, event: Event) -> &str {
        EVENTS
            .iter()
            .find(|(e, _)| *e == event)
            .and_then(|(_, key)| self.events.get(*key))
            .and_then(|names| names.first())
            .map_or("", String::as_str)
    }
}
