//! `knowledge hook <verb> --harness <id>`: what a harness runs on each of its events. The adapter
//! named by `--harness` turns the payload into a [`Payload`] and the answer into its own shape; the
//! verbs below never see which harness called (K26).
//! Fails silent, always ([[0002-hooks-fail-silent-always-query]]): an unknown harness, a harness
//! without hooks, bad JSON, no match or no index all end in exit 0 and no output.
use crate::config::{self, Config};
use crate::harness::{self, Event, Payload};
use crate::{Args, Fault, emit, index, search, vault};
use std::collections::HashSet;
use std::io::Read;

pub fn run(raw: &[String]) {
    let _ = handle(raw);
}

fn handle(raw: &[String]) -> Result<(), Fault> {
    let args = Args::parse(raw, &["harness", "record", "pin"])?;
    let Some(adapter) = args.get("harness").and_then(harness::adapter) else {
        return Ok(());
    };
    let Some(t) = adapter.translator() else {
        return Ok(());
    };
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| Fault::Io("stdin".into(), e))?;
    if let Some(dir) = args.get("record") {
        record(dir, &input);
    }
    let verb = args.words.first().map_or("", String::as_str);
    if verb == "record" {
        return Ok(());
    }
    // The Codex and opencode plugins name the version they were written for (`--pin`). A binary of
    // another version stands down: the one session start says why, so the owner knows to update.
    if let Some(pin) = args.get("pin").filter(|p| *p != env!("CARGO_PKG_VERSION")) {
        if verb == "session" {
            let line = format!(
                "knowledge-skeleton: this binary is {}, but the {} plugin's hooks were written for {pin}, so they stand down. Update the plugin: knowledge setup --harness {} --plan, then --apply.",
                env!("CARGO_PKG_VERSION"),
                adapter.name,
                adapter.id
            );
            emit(&t.context(&adapter, Event::SessionStart, &line));
        }
        return Ok(());
    }
    let p = match t.parse(&adapter, &input) {
        Ok(p) => p,
        Err(f) if verb == "pre-tool" => {
            eprintln!("knowledge guard: {f}; allowing");
            return Ok(());
        }
        Err(_) if matches!(verb, "prompt" | "post-edit") => return Ok(()),
        // The rest read what they can of an empty payload, as the bun hooks did.
        Err(_) => Payload {
            name: "unknown".into(),
            ..Payload::default()
        },
    };
    // Each verb is one of the six events (`capture` is pre-compact or end, by the payload's name).
    let (event, context) = match verb {
        "session" => (
            Event::SessionStart,
            crate::session::hook(args.has("dry"), adapter.instructions_label()),
        ),
        "pre-tool" => {
            if let Some((action, reason)) = crate::guard::run(&p) {
                emit(&t.verdict(&adapter, &action, &reason));
            }
            return Ok(());
        }
        // A `claude -p` this engine started fires the hooks too: these stand down inside it.
        "post-bash" if !crate::state::invoked() => {
            crate::recorder::hook(&p.call.command);
            return Ok(());
        }
        "capture" => {
            crate::memory::hook(&p, args.has("dry-run"));
            return Ok(());
        }
        "prompt" => (Event::Prompt, prompt(&config::load()?, &p)?),
        "post-edit" => (Event::PostTool, post_edit(&config::load()?, &p)?),
        _ => return Ok(()),
    };
    if let Some(context) = context {
        emit(&t.context(&adapter, event, &context));
    }
    Ok(())
}

/// `--record <dir>`: append the raw payload, one line, to `<dir>/<hook_event_name>.jsonl`, then
/// carry on as usual. The parity gate replays these (spec item 2); the folder holds real prompts and
/// paths, so it is the caller's to keep out of git.
fn record(dir: &str, input: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(input) else {
        return;
    };
    let event = v["hook_event_name"].as_str().unwrap_or("unknown");
    let event: String = event.chars().filter(char::is_ascii_alphanumeric).collect();
    let _ = std::fs::create_dir_all(dir);
    let path = std::path::Path::new(dir).join(format!("{event}.jsonl"));
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{v}");
    }
}

fn prompt(cfg: &Config, p: &Payload) -> Result<Option<String>, Fault> {
    let Some(text) = &p.prompt else {
        return Ok(None);
    };
    search::inject(
        &index::open_ro(cfg)?,
        cfg,
        text,
        search::FORMAT,
        search::PASSAGE,
        &search::TUNED,
    )
}

/// Each file an edit touched (one `apply_patch` can name several) is judged on its own.
fn post_edit(cfg: &Config, p: &Payload) -> Result<Option<String>, Fault> {
    let mut msgs = Vec::new();
    for path in &p.call.paths {
        if let Some(m) = post_edit_one(cfg, std::path::Path::new(path))? {
            msgs.push(m);
        }
    }
    Ok((!msgs.is_empty()).then(|| msgs.join("\n")))
}

/// An Edit or Write on a vault note reindexes that note, then checks it the way brain-reindex's
/// validateNote() did. Anything outside the vault is not ours.
fn post_edit_one(cfg: &Config, path: &std::path::Path) -> Result<Option<String>, Fault> {
    let Some(rel) = cfg.rel_of(path) else {
        // A repo's docs are kept current the same way, without the vault's write-time rules.
        let repos = cfg.repos();
        if let Some(rel) = repos.rel_of(path).filter(|r| index::repo_doc(r)) {
            index::one(&mut index::open_rw(&repos)?, &repos, &rel)?;
        }
        return Ok(None);
    };
    if !vault::indexable(&rel) {
        return Ok(None);
    }
    let mut conn = index::open_rw(cfg)?;
    if !index::one(&mut conn, cfg, &rel)? {
        return Ok(None);
    }
    validate(&conn, cfg, &rel)
}

/// Frontmatter `type` and `created`, and every [[link]] resolving to a note, a file or a listed
/// forward link; a [[Note#Heading]] needs that heading. The message is brain-reindex's, word for word.
pub fn validate(
    conn: &rusqlite::Connection,
    cfg: &Config,
    rel: &str,
) -> Result<Option<String>, Fault> {
    let file_name = rel.rsplit('/').next().unwrap_or(rel);
    if vault::exempt(file_name) {
        return Ok(None);
    }
    let path = cfg.abs_of(rel);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| Fault::io(&path, e))?
        .replace("\r\n", "\n");
    let mut problems = Vec::new();
    if !vault::split_front(&text).0.is_some_and(vault::front_ok) {
        problems.push("frontmatter needs `type` and `created`".to_string());
    }
    let forward = vault::forward_links(&cfg.vault);
    let mut is_file = conn.prepare("SELECT 1 FROM files WHERE name = ?1")?;
    let links = vault::links_of(&text);
    let mut broken = Vec::new();
    for l in &links {
        let k = l.name.to_lowercase();
        if index::notes_named(conn, &k)?.is_empty()
            && !is_file.exists([&k])?
            && !forward.contains(&k)
        {
            broken.push(l.name.clone());
        }
    }
    for l in &links {
        let Some(h) = &l.heading else { continue };
        let Some((_, target)) = index::pick(rel, &index::notes_named(conn, &l.name)?).cloned()
        else {
            continue;
        };
        let Ok(t) = std::fs::read_to_string(cfg.abs_of(&target)) else {
            continue;
        };
        if !vault::headings_of(&t).contains(&h.to_lowercase()) {
            broken.push(format!("{}#{h}", l.name));
        }
    }
    let mut seen = HashSet::new();
    broken.retain(|b| seen.insert(b.clone()));
    if !broken.is_empty() {
        let list: Vec<String> = broken.iter().map(|b| format!("[[{b}]]")).collect();
        problems.push(format!(
            "links to nothing: {} — fix, or list as intentional in 00-Meta/Forward-Links.md",
            list.join(", ")
        ));
    }
    Ok((!problems.is_empty()).then(|| {
        format!(
            "brain-validate {file_name}: {}. Conventions: 00-Meta/Conventions.md.",
            problems.join("; ")
        )
    }))
}
