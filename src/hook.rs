//! `knowledge hook <event> --harness claude`: what Claude Code runs on a prompt and after an edit.
//! Fails silent, always ([[0002-hooks-fail-silent-always-query]]): an unknown harness, bad JSON,
//! no match or no index all end in exit 0 and no output.
use crate::config::{self, Config};
use crate::{Args, Fault, emit, index, search, vault};
use serde::Deserialize;
use std::collections::HashSet;
use std::io::Read;

#[derive(Deserialize)]
struct Prompt {
    prompt: Option<String>,
}

#[derive(Deserialize)]
struct Edit {
    tool_input: Option<ToolInput>,
}

#[derive(Deserialize)]
struct ToolInput {
    file_path: Option<String>,
}

pub fn run(raw: &[String]) {
    let _ = handle(raw);
}

fn handle(raw: &[String]) -> Result<(), Fault> {
    let args = Args::parse(raw, &["harness"])?;
    if args.get("harness") != Some("claude") {
        return Ok(());
    }
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| Fault::Io("stdin".into(), e))?;
    let cfg = config::load()?;
    let (event, context) = match args.words.first().map(String::as_str) {
        Some("prompt") => ("UserPromptSubmit", prompt(&cfg, &input)?),
        Some("post-edit") => ("PostToolUse", post_edit(&cfg, &input)?),
        _ => return Ok(()),
    };
    if let Some(context) = context {
        emit(&serde_json::json!({"hookSpecificOutput": {"hookEventName": event, "additionalContext": context}}).to_string());
    }
    Ok(())
}

fn prompt(cfg: &Config, input: &str) -> Result<Option<String>, Fault> {
    let Some(text) = serde_json::from_str::<Prompt>(input)?.prompt else {
        return Ok(None);
    };
    search::inject(
        &index::open_ro(cfg)?,
        cfg,
        &text,
        search::FORMAT,
        search::PASSAGE,
        &search::TUNED,
    )
}

/// An Edit or Write on a vault note reindexes that note, then checks it the way brain-reindex's
/// validateNote() did. Anything outside the vault is not ours.
fn post_edit(cfg: &Config, input: &str) -> Result<Option<String>, Fault> {
    let Some(file) = serde_json::from_str::<Edit>(input)?
        .tool_input
        .and_then(|t| t.file_path)
    else {
        return Ok(None);
    };
    let Some(rel) = cfg.rel_of(std::path::Path::new(&file)) else {
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
