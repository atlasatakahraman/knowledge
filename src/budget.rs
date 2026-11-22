//! `knowledge doctor budget [<transcript>] [--json]`: what fills the context before the first prompt,
//! by owner and source, read from one transcript (the vault audit's `session-start-cost.mjs`, ported:
//! D10). Prints names, counts and sizes, never content.
//!
//! Tokens are chars / 3.7, an estimate. Chars are UTF-16 units, as the script's `.length` counts them,
//! so both agree on the same transcript. It WARNs for a plugin or connector whose listing costs over
//! 1,000 tokens and that had no call in the last 30 days, in any transcript.
use crate::config;
use crate::{Args, Fault, emit};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const IDLE_WARN: usize = 1_000;
const IDLE_DAYS: u64 = 30;

fn tok(ch: usize) -> usize {
    (ch as f64 / 3.7).round() as usize
}
fn ulen(s: &str) -> usize {
    s.encode_utf16().count()
}
/// The script's `len`: a string's length, anything else's JSON; null and absent count as `""`.
fn len(v: &Value) -> usize {
    match v {
        Value::String(s) => ulen(s),
        Value::Null => 2,
        _ => ulen(&v.to_string()),
    }
}
/// JS `a || b`: the first value that is not null, false, 0 or "".
fn or<'a>(vs: &[&'a Value]) -> Option<&'a Value> {
    vs.iter().copied().find(|v| {
        !matches!(v, Value::Null | Value::Bool(false))
            && v.as_str() != Some("")
            && v.as_f64() != Some(0.0)
    })
}
fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// Which plugin or connector a name belongs to: `plugin_vercel_vercel` and `plugin:vercel:vercel`
/// are both `vercel`; any other server is itself.
fn owner_of(server: &str) -> String {
    if let Some(rest) = server.strip_prefix("plugin_") {
        return rest.split('_').next().unwrap_or(rest).to_string();
    }
    if let Some(rest) = server.strip_prefix("plugin:") {
        return rest.split(':').next().unwrap_or(rest).to_string();
    }
    server.to_string()
}
/// `mcp__<server>__tool` → `<server>`.
fn server_of(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("mcp__")?;
    rest.find("__").map(|i| &rest[..i])
}
/// A skill listing line `- name: …` or `- plugin:name: …` → its name.
fn skill_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("- ")?;
    let word = |s: &str| {
        s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(s.len())
    };
    let mut end = word(rest);
    if end == 0 {
        return None;
    }
    if rest[end..].starts_with(':') {
        let more = word(&rest[end + 1..]);
        if more > 0
            && (rest[end + 1 + more..].is_empty() || rest[end + 1 + more..].starts_with(": "))
        {
            end += 1 + more;
        }
    }
    (rest[end..].is_empty() || rest[end..].starts_with(": ")).then(|| &rest[..end])
}

struct Slice {
    owner: &'static str,
    name: String,
    ch: usize,
}

fn newest(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .max_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        })
        .map(|e| e.path())
}

/// Every installed plugin's hook files (any `.json` under a `hooks/` folder, and `plugin.json`,
/// which may declare hooks inline), as (plugin, text). The transcript stores a plugin hook's
/// command unexpanded (`${CLAUDE_PLUGIN_ROOT}/…`) or as its status message, so the plugin is the one
/// whose file holds that exact text.
fn hook_files(plugins: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut dirs = vec![(plugins.join("cache"), 0, String::new())];
    while let Some((d, depth, plugin)) = dirs.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() && depth < 5 && name != "node_modules" {
                // cache/<marketplace>/<plugin>/<version>/…
                dirs.push((p, depth + 1, if depth == 1 { name } else { plugin.clone() }));
            } else if (name == "plugin.json" || (name.ends_with(".json") && d.ends_with("hooks")))
                && let Ok(t) = fs::read_to_string(&p)
            {
                out.push((plugin.clone(), t));
            }
        }
    }
    out
}

/// Calls per plugin or connector across every transcript written in the last 30 days.
fn calls(projects: &Path) -> BTreeMap<String, usize> {
    let cut = SystemTime::now() - Duration::from_secs(IDLE_DAYS * 86_400);
    let mut n = BTreeMap::new();
    let mut dirs = vec![projects.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
                continue;
            }
            if p.extension().is_none_or(|x| x != "jsonl")
                || e.metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t < cut)
            {
                continue;
            }
            let Ok(text) = fs::read_to_string(&p) else {
                continue;
            };
            for line in text.lines().filter(|l| l.contains("\"tool_use\"")) {
                let Ok(o) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                for b in arr(&o["message"]["content"])
                    .iter()
                    .filter(|b| b["type"] == "tool_use")
                {
                    let name = b["name"].as_str().unwrap_or("");
                    let key = match server_of(name) {
                        Some(s) => owner_of(s),
                        None if name == "Skill" => {
                            match b["input"]["skill"].as_str().and_then(|s| s.split_once(':')) {
                                Some((plugin, _)) => plugin.to_string(),
                                None => continue,
                            }
                        }
                        None => continue,
                    };
                    *n.entry(key).or_insert(0) += 1;
                }
            }
        }
    }
    n
}

/// `knowledge doctor profile <note>`: the project settings that leave every connector off but the
/// note's `connectors:` (K33). A note with `checkout:` is that checkout's project; one without is the
/// vault's own. It prints only the two settings the vault audit's S5.0 probe proved to work per project:
/// `enabledPlugins: {<plugin>: false}` and `disableClaudeAiConnectors: true`. It writes nothing.
pub fn profile(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &[])?;
    let Some(want) = args.words.get(1) else {
        return Err(Fault::Usage("knowledge doctor profile <note>".into()));
    };
    let cfg = config::load()?;
    let note = crate::vault::walk(&cfg.vault)?
        .into_iter()
        .find(|f| f.is_note() && f.rel.rsplit('/').next() == Some(&format!("{want}.md")))
        .ok_or_else(|| Fault::Usage(format!("no note named {want}")))?;
    let text = fs::read_to_string(cfg.abs_of(&note.rel))
        .map_err(|e| Fault::Io(note.rel.clone(), e))?
        .replace("\r\n", "\n");
    let fields = crate::vault::split_front(&text)
        .0
        .map(crate::vault::fields)
        .unwrap_or_default();
    let field = |k: &str| {
        fields
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let keep: Vec<String> = field("connectors")
        .iter()
        .map(|c| c.to_lowercase())
        .collect();
    let project = match field("checkout").first() {
        Some(c) => cfg.dev.join(c),
        None => cfg.vault.clone(),
    };

    // A plugin is a connector when it ships an MCP server: `.mcp.json`, or `mcpServers` in its manifest.
    let claude = std::env::home_dir().unwrap_or_default().join(".claude");
    let read_json = |p: PathBuf| {
        fs::read_to_string(p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .unwrap_or(Value::Null)
    };
    let installed = read_json(claude.join("plugins").join("installed_plugins.json"));
    let settings = read_json(claude.join("settings.json"));
    let mut off = serde_json::Map::new();
    let mut plugins_kept = Vec::new();
    for (id, on) in settings["enabledPlugins"].as_object().into_iter().flatten() {
        let Some(path) = installed["plugins"][id][0]["installPath"]
            .as_str()
            .map(PathBuf::from)
        else {
            continue;
        };
        let manifest =
            fs::read_to_string(path.join(".claude-plugin").join("plugin.json")).unwrap_or_default();
        if on != true || !(path.join(".mcp.json").exists() || manifest.contains("\"mcpServers\"")) {
            continue;
        }
        let name = id.split('@').next().unwrap_or(id).to_lowercase();
        if keep.contains(&name) {
            plugins_kept.push(name);
        } else {
            off.insert(id.clone(), Value::Bool(false));
        }
    }
    // A claude.ai connector is listed as `claude.ai/<name>`: "supabase" is also a plugin, and the two are
    // different servers. They have one proven switch, all or none, so one listed keeps them all on.
    let web: Vec<&String> = keep
        .iter()
        .filter(|k| k.starts_with("claude.ai/"))
        .collect();
    let unknown: Vec<&String> = keep
        .iter()
        .filter(|k| !k.starts_with("claude.ai/") && !plugins_kept.contains(k))
        .collect();
    let mut out = serde_json::Map::new();
    if !off.is_empty() {
        out.insert("enabledPlugins".into(), Value::Object(off));
    }
    if web.is_empty() {
        out.insert("disableClaudeAiConnectors".into(), Value::Bool(true));
    }
    let mut s = format!(
        "{}\n  connectors: {}\n  merge into {}\n{}\n",
        note.rel,
        if keep.is_empty() {
            "(none)".into()
        } else {
            keep.join(", ")
        },
        project.join(".claude").join("settings.json").display(),
        serde_json::to_string_pretty(&Value::Object(out)).unwrap_or_default()
    );
    if !web.is_empty() {
        s.push_str(&format!(
            "  claude.ai connectors stay on, all of them: {} needs them, and they switch off only together\n",
            web.iter().map(|w| w.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    for u in &unknown {
        s.push_str(&format!("  !! {u}: not an enabled plugin with an MCP server; a claude.ai connector is written claude.ai/<name>\n"));
    }
    emit(&s);
    Ok(u8::from(!unknown.is_empty()))
}

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &[])?;
    let claude = std::env::home_dir().unwrap_or_default().join(".claude");
    let projects = claude.join("projects");
    let hook_json = hook_files(&claude.join("plugins"));
    let file = match args.words.get(1) {
        Some(f) => PathBuf::from(f),
        None => {
            // Claude Code names a project's folder after its path, every non-alphanumeric char a `-`.
            let cfg = config::load()?;
            let slug: String = cfg
                .vault
                .to_string_lossy()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            newest(&projects.join(slug))
                .ok_or_else(|| Fault::Usage(format!("no transcript in {}", projects.display())))?
        }
    };
    let text = fs::read_to_string(&file).map_err(|e| Fault::Io(file.display().to_string(), e))?;

    let mut shape = Value::Null;
    let mut first: BTreeMap<String, Value> = BTreeMap::new();
    let mut hooks: Vec<(String, String, usize)> = Vec::new(); // (shown name, plugin, chars)
    let (mut inject_n, mut inject_ch) = (0usize, 0usize);
    for line in text.lines() {
        let Ok(mut o) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if o["type"] == "api-request-shape" {
            if shape.is_null() {
                shape = o["shape"].take();
            }
            continue;
        }
        let a = o["attachment"].take();
        if o["type"] != "attachment" || !a.is_object() {
            continue;
        }
        if a["type"] == "hook_success"
            && a["hookEvent"] == "SessionStart"
            && a["hookName"] == "SessionStart:startup"
        {
            let cmd = a["command"].as_str().unwrap_or("").replace('\\', "/");
            let parts: Vec<&str> = cmd.split('/').collect();
            let tail = parts[parts.len().saturating_sub(2)..].join("/");
            let shown: String = tail
                .chars()
                .rev()
                .take(60)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let raw_cmd = a["command"].as_str().unwrap_or("");
            let quoted = serde_json::to_string(raw_cmd).unwrap_or_default();
            let plugin = match cmd
                .split_once("plugins/cache/")
                .and_then(|(_, r)| r.split('/').nth(1))
            {
                Some(p) => p.to_string(),
                None if !raw_cmd.is_empty() => hook_json
                    .iter()
                    .find(|(_, t)| t.contains(&quoted[1..quoted.len() - 1]))
                    .map(|(p, _)| p.clone())
                    .unwrap_or_default(),
                None => String::new(),
            };
            hooks.push((
                shown,
                plugin,
                or(&[&a["content"], &a["stdout"]]).map_or(0, len),
            ));
        } else if a["type"] == "hook_additional_context" && a["hookEvent"] == "UserPromptSubmit" {
            inject_n += 1;
            inject_ch += len(&a["content"]);
        } else if let Some(t) = a["type"].as_str() {
            first.entry(t.to_string()).or_insert(a);
        }
    }
    let at = |k: &str| first.get(k).unwrap_or(&Value::Null);

    let tools = arr(&shape["tools"]);
    let files = arr(&at("instructions")["files"]);
    let dt = at("deferred_tools_delta");
    let dt_lines = arr(or(&[&dt["addedLines"], &dt["addedNames"]]).unwrap_or(&Value::Null));
    let sk = at("skill_listing");
    let sk_text = sk["content"].as_str().unwrap_or("");
    let mi = at("mcp_instructions_delta");
    let ag = at("agent_listing_delta");
    let lines_ch = |v: &[Value]| v.iter().map(|l| len(l) + 1).sum::<usize>();
    let skill_n = sk["skillCount"]
        .as_u64()
        .map(|n| n as usize)
        .unwrap_or(arr(&sk["names"]).len());
    let mut rows = vec![
        Slice {
            owner: "harness",
            name: "system prompt".into(),
            ch: or(&[&shape["system"]]).map_or(0, len),
        },
        Slice {
            owner: "harness",
            name: format!("tool schemas loaded up front ({})", tools.len()),
            ch: len(&Value::Array(tools.to_vec())),
        },
        Slice {
            owner: "ours",
            name: format!("priming files ({})", files.len()),
            ch: files
                .iter()
                .map(|f| {
                    len(if f["content"].is_null() {
                        f
                    } else {
                        &f["content"]
                    })
                })
                .sum(),
        },
        Slice {
            owner: "plugins",
            name: format!("deferred tool names ({})", dt_lines.len()),
            ch: lines_ch(dt_lines),
        },
        Slice {
            owner: "plugins",
            name: format!("skill listing ({skill_n})"),
            ch: ulen(sk_text),
        },
        Slice {
            owner: "plugins",
            name: format!("MCP server instructions ({})", arr(&mi["addedNames"]).len()),
            ch: arr(&mi["addedBlocks"]).iter().map(len).sum(),
        },
        Slice {
            owner: "plugins",
            name: format!("SessionStart hook output ({} hooks)", hooks.len()),
            ch: hooks.iter().map(|h| h.2).sum(),
        },
        Slice {
            owner: "harness",
            name: format!("agent types ({})", arr(&ag["addedTypes"]).len()),
            ch: lines_ch(arr(&ag["addedLines"])),
        },
        Slice {
            owner: "ours",
            name: "output style".into(),
            ch: or(&[&at("output_style_instructions")["style"]]).map_or(0, len),
        },
    ];
    rows.sort_by_key(|r| std::cmp::Reverse(r.ch));
    let total: usize = rows.iter().map(|r| r.ch).sum();
    let by = |who: &str| tok(rows.iter().filter(|r| r.owner == who).map(|r| r.ch).sum());

    // Each plugin's or connector's listing: its deferred names, MCP instructions, skills and hook output.
    let mut listing: BTreeMap<String, usize> = BTreeMap::new();
    for l in dt_lines {
        let s = l.as_str().unwrap_or("");
        let name = s
            .trim()
            .split(|c: char| c.is_whitespace() || c == ':' || c == '(')
            .next()
            .unwrap_or("");
        if let Some(server) = server_of(name) {
            *listing.entry(owner_of(server)).or_insert(0) += len(l) + 1;
        }
    }
    for (n, b) in arr(&mi["addedNames"]).iter().zip(arr(&mi["addedBlocks"])) {
        *listing
            .entry(owner_of(n.as_str().unwrap_or("")))
            .or_insert(0) += len(b);
    }
    let mut cur: Option<String> = None;
    for l in sk_text.split('\n') {
        if let Some(name) = skill_name(l) {
            cur = name.split_once(':').map(|(p, _)| p.to_string());
        }
        if let Some(p) = &cur {
            *listing.entry(p.clone()).or_insert(0) += ulen(l) + 1;
        }
    }
    for (_, plugin, ch) in hooks.iter().filter(|h| !h.1.is_empty()) {
        *listing.entry(plugin.clone()).or_insert(0) += ch;
    }
    let used = calls(&projects);
    let idle: Vec<(String, usize)> = listing
        .iter()
        .filter(|(k, ch)| tok(**ch) > IDLE_WARN && !used.contains_key(*k))
        .map(|(k, ch)| (k.clone(), tok(*ch)))
        .collect();

    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if args.has("json") {
        let slices: Vec<_> = rows
            .iter()
            .map(|r| json!({"owner": r.owner, "slice": r.name, "chars": r.ch, "tokens": tok(r.ch)}))
            .collect();
        let idle: Vec<_> = idle
            .iter()
            .map(|(k, t)| json!({"name": k, "tokens": t}))
            .collect();
        emit(&format!(
            "{}\n",
            json!({"check": "budget", "ok": idle.is_empty(), "transcript": name, "tokens": {"total": tok(total), "harness": by("harness"), "ours": by("ours"), "plugins": by("plugins"), "controllable": tok(rows.iter().filter(|r| r.owner != "harness").map(|r| r.ch).sum())}, "slices": slices, "idle": idle})
        ));
        return Ok(0);
    }
    let mut out = format!(
        "{name}\n\nowner    slice                                         chars    ~tokens\n"
    );
    for r in &rows {
        out.push_str(&format!(
            "{:<8} {:<42} {:>8} {:>9}\n",
            r.owner,
            r.name,
            r.ch,
            tok(r.ch)
        ));
    }
    out.push_str(&format!(
        "{:<8} {:<42} {:>8} {:>9}\n",
        "",
        "TOTAL before the first prompt",
        total,
        tok(total)
    ));
    let controllable = tok(rows
        .iter()
        .filter(|r| r.owner != "harness")
        .map(|r| r.ch)
        .sum());
    out.push_str(&format!(
        "  harness {}  ours {}  plugins {}  controllable (ours + plugins) {controllable}\n",
        by("harness"),
        by("ours"),
        by("plugins")
    ));
    out.push_str("\nlisting by plugin or connector (tokens, calls in 30 days)\n");
    let mut ls: Vec<_> = listing.iter().collect();
    ls.sort_by(|a, b| b.1.cmp(a.1));
    for (k, ch) in ls {
        out.push_str(&format!(
            "  {:<44} {:>7} {:>6}\n",
            k,
            tok(*ch),
            used.get(k).copied().unwrap_or(0)
        ));
    }
    out.push_str("\nSessionStart hooks\n");
    for (n, _, ch) in &hooks {
        out.push_str(&format!("  {n:<60} {:>7}\n", tok(*ch)));
    }
    out.push_str(&format!(
        "\nper prompt: brain-inject {inject_n} prompts, {} tokens each\n",
        inject_ch.checked_div(inject_n).map_or(0, tok)
    ));
    for (k, t) in &idle {
        out.push_str(&format!(
            "WARN {k}: {t} tokens of listing every session, no call in {IDLE_DAYS} days\n"
        ));
    }
    emit(&out);
    Ok(0)
}
