//! `knowledge mcp`: the vault's retrieval as an MCP server over stdio, for a harness whose shell
//! sandbox cannot read `~/.config/knowledge` (Codex on Windows runs its commands as
//! `CodexSandboxUsers`, denied there; the harness spawns an MCP server as the user, outside it).
//! It is also the whole engine for a harness with no shell at all (the Claude desktop app's chat).
//! JSON-RPC 2.0, one message per line. Four tools, K39's ceiling: `find`, `graph` and `read`
//! (read-only) and `note` (an append), each re-running this binary's own verb, so the server holds
//! no index handle and no state between calls.
use crate::{Fault, emit};
use serde_json::{Value, json};
use std::io::BufRead;
use std::time::Duration;

/// A verb that outlives this has hung; the harness gets an error, not a stalled turn.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    if !raw.is_empty() {
        return Err(Fault::Usage("knowledge mcp (takes no arguments)".into()));
    }
    let exe = std::env::current_exe().map_err(|e| Fault::Config(format!("current_exe: {e}")))?;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle(&exe, &line) {
            emit(&format!("{reply}\n"));
        }
    }
    Ok(0)
}

/// One message in, at most one reply out. A notification (no `id`) and a client's response (no
/// `method`) get none.
fn handle(exe: &std::path::Path, line: &str) -> Option<Value> {
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return Some(error(Value::Null, -32700, "parse error"));
    };
    let id = msg.get("id").cloned()?;
    let method = msg["method"].as_str()?;
    let p = &msg["params"];
    Some(match method {
        "initialize" => ok(
            id,
            json!({
                // Echo the client's revision: the two tools read the same in every one so far.
                "protocolVersion": p["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "knowledge", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }),
        ),
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({ "tools": tools() })),
        // An unknown tool is a protocol error; a bad argument is the tool's, so the model reads it.
        "tools/call" => match argv(&p["name"], &p["arguments"]) {
            Ok(args) => ok(id, call(exe, &args)),
            Err(None) => error(id, -32602, "unknown tool"),
            Err(Some(m)) => ok(id, result(m, true)),
        },
        _ => error(id, -32601, "method not found"),
    })
}

/// What the server is for, sent at `initialize` for a client that puts it before the model. The
/// desktop app's chat asked "where is perkRefilled defined?" and searched its own sandbox and the
/// disk instead (S8.2): nothing told it the owner's code is reachable only through this server.
const INSTRUCTIONS: &str = "The owner's knowledge vault and the code map of their repositories. \
    The repositories are not in your sandbox or working folder: for where a function, class, \
    component or other symbol is defined, call find with code true; for what uses it, what it \
    uses or how two symbols connect, call graph with code true. For the owner's notes, \
    decisions and plans, call find, then read the note it names.";

/// Read-only, closed-world: Codex calls a tool annotated this way without asking.
fn tools() -> Value {
    let notes = json!({ "readOnlyHint": true, "destructiveHint": false,
                        "idempotentHint": true, "openWorldHint": false });
    json!([
        {
            "name": "find",
            "title": "Search the knowledge vault, or find where code is defined",
            "description": "Two searches. With code true: where a function, class, component or \
                other symbol is defined in the owner's repositories (which are not in your \
                sandbox), one line each as repo path:line kind signature; q is the name or part \
                of it, and repo narrows it to one repository. Without code: ranked notes for a \
                question, each with its best passage; scope vault (default), repo (a \
                repository's indexed docs; repo names one) or captures (kept and fetched output, \
                14 days). One call per question.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "The question, in words; with code, a symbol's name or part of it." },
                    "scope": { "type": "string", "enum": ["vault", "repo", "captures"] },
                    "repo": { "type": "string", "description": "With scope repo or code: the repository's name." },
                    "code": { "type": "boolean", "description": "Search the repositories' code symbols." }
                },
                "required": ["q"],
                "additionalProperties": false
            },
            "annotations": notes,
        },
        {
            "name": "graph",
            "title": "A vault note's links, or a code symbol's uses",
            "description": "A note's links in and out, or with other, the path between two notes. \
                With code: a code symbol's definition, what uses it and what it uses, or with \
                other, the chain of uses between two symbols (repo narrows it to one repository).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "note": { "type": "string", "description": "A note's name or vault path; with code, a symbol's name." },
                    "other": { "type": "string" },
                    "repo": { "type": "string", "description": "With code: the repository's name." },
                    "code": { "type": "boolean", "description": "Read the repositories' code map." }
                },
                "required": ["note"],
                "additionalProperties": false
            },
            "annotations": notes,
        },
        {
            "name": "read",
            "title": "Read a vault note",
            "description": "One note's full text, by the name find or graph gave.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "note": { "type": "string", "description": "A note's name or vault path." }
                },
                "required": ["note"],
                "additionalProperties": false
            },
            "annotations": notes,
        },
        {
            "name": "note",
            "title": "Write to the vault's memory",
            "description": "decision: one line in the decision ledger, for the owner to promote to \
                an ADR. log: one bullet in today's session note. One line of text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["decision", "log"] },
                    "text": { "type": "string", "description": "What was decided, or what happened." }
                },
                "required": ["kind", "text"],
                "additionalProperties": false
            },
            // An append: not read-only, so a harness asks before calling it; never destructive.
            "annotations": { "readOnlyHint": false, "destructiveHint": false,
                             "idempotentHint": false, "openWorldHint": false },
        }
    ])
}

/// The verb's argv for a call. Every value is a string that cannot start with `-`, so a model
/// cannot pass a flag (`--repair`, `--budget`) through a field: this is the trust boundary.
/// `Err(None)` names no tool of this server; `Err(Some(m))` is a bad argument.
fn argv(name: &Value, a: &Value) -> Result<Vec<String>, Option<String>> {
    let field = |k: &str, required: bool| -> Result<Option<String>, Option<String>> {
        match &a[k] {
            Value::Null if !required => Ok(None),
            Value::String(s) if !s.trim().is_empty() && !s.starts_with('-') => Ok(Some(s.clone())),
            _ => Err(Some(format!(
                "{k}: a non-empty string not starting with '-'"
            ))),
        }
    };
    if !(a.is_object() || a.is_null()) {
        return Err(Some("arguments: an object".into()));
    }
    let code = match &a["code"] {
        Value::Null => false,
        Value::Bool(b) => *b,
        _ => return Err(Some("code: true or false".into())),
    };
    // The code map's flags: `--code`, and `--repo <name>` when one is named.
    let code_flags = |v: &mut Vec<String>| -> Result<(), Option<String>> {
        v.push("--code".into());
        if let Some(r) = field("repo", false)? {
            v.extend(["--repo".into(), r]);
        }
        Ok(())
    };
    match name.as_str() {
        Some("find") if code => {
            let mut v = vec!["find".into(), field("q", true)?.unwrap_or_default()];
            if !(a["scope"].is_null() || a["scope"] == "repo") {
                return Err(Some("scope: none, or repo, with code".into()));
            }
            code_flags(&mut v)?;
            Ok(v)
        }
        Some("find") => {
            let mut v = vec!["find".into(), field("q", true)?.unwrap_or_default()];
            let scope = if a["scope"].is_null() {
                Some("vault")
            } else {
                a["scope"].as_str()
            };
            match (scope, field("repo", false)?) {
                (Some("vault"), None) => {}
                (Some("repo"), repo) => {
                    v.push("--repo".into());
                    v.extend(repo);
                }
                (Some("captures"), None) => v.push("--captures".into()),
                _ => {
                    return Err(Some(
                        "scope: vault | repo | captures; repo only with scope repo".into(),
                    ));
                }
            }
            Ok(v)
        }
        Some("graph") => {
            let mut v = vec!["graph".into(), field("note", true)?.unwrap_or_default()];
            v.extend(field("other", false)?);
            if code {
                code_flags(&mut v)?;
            } else if !a["repo"].is_null() {
                return Err(Some("repo: only with code".into()));
            }
            Ok(v)
        }
        Some("read") => Ok(vec![
            "graph".into(),
            field("note", true)?.unwrap_or_default(),
            "--text".into(),
        ]),
        Some("note") => match a["kind"].as_str() {
            Some(k @ ("decision" | "log")) => Ok(vec![
                "note".into(),
                k.into(),
                field("text", true)?.unwrap_or_default(),
            ]),
            _ => Err(Some("kind: decision | log".into())),
        },
        _ => Err(None),
    }
}

/// Run the verb; its stdout is the result, its stderr the error text when it fails. A code verb
/// runs in `knowledge-code` directly, not through this binary's hand-off (spec item 44). `argv`
/// is the only source of a `--code`: no field can carry a flag.
fn call(exe: &std::path::Path, args: &[String]) -> Value {
    let code;
    let exe = if args.iter().any(|a| a == "--code") {
        match crate::code_exe() {
            Ok(p) => {
                code = p;
                code.as_path()
            }
            Err(e) => return result(e.to_string(), true),
        }
    } else {
        exe
    };
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = crate::state::run(exe, &refs, None, None, &[], CALL_TIMEOUT);
    let failed = !o.ok();
    let text = match (failed, o.timed_out) {
        (false, _) => o.stdout,
        (true, true) => format!("knowledge {}: timed out", args[0]),
        (true, false) => format!("{}{}", o.stdout, o.stderr).trim().to_string(),
    };
    result(text, failed)
}

fn result(text: String, failed: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": failed })
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
