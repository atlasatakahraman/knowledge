//! Retrieval: `find` (tier 2) and `inject` (tier 0, every prompt). Spec § Track 4, Retrieval:
//! the prompt's rarest tokens, the top chunks, the best chunk per note.
use crate::config::{self, Config};
use crate::index;
use crate::{Args, Fault, emit};
use rusqlite::Connection;
use std::collections::HashSet;

/// English function words: they say nothing about a note, yet in a technical vault a word like
/// `these` is rare enough to take a rare-token slot from a word that names the topic (S7.0a:
/// `these`, `there` and `most` pushed `player` out of a Queue prompt). The usual English list, of
/// three letters or more, plus brain-lib's `get`, `got`, `use`, `using` and `would`.
#[rustfmt::skip] // one word per line would be 105 lines
const STOP: [&str; 105] = [
    "about", "above", "after", "again", "against", "all", "and", "any", "are", "because", "been",
    "before", "being", "below", "between", "both", "but", "can", "did", "does", "doing", "down",
    "during", "each", "few", "for", "from", "further", "get", "got", "had", "has", "have",
    "having", "her", "here", "hers", "herself", "him", "himself", "his", "how", "into", "its",
    "itself", "just", "more", "most", "myself", "nor", "not", "now", "off", "once", "only",
    "other", "our", "ours", "ourselves", "out", "over", "own", "same", "she", "should", "some",
    "such", "than", "that", "the", "their", "theirs", "them", "themselves", "then", "there",
    "these", "they", "this", "those", "through", "too", "under", "until", "use", "using", "very",
    "was", "were", "what", "when", "where", "which", "while", "who", "whom", "why", "will",
    "with", "would", "you", "your", "yours", "yourself", "yourselves",
];

/// Kinds that are records, not answers: hundreds of short commit notes would crowd rules and
/// decisions out of six slots. `--kind all` or `--kind commit` reaches them.
const RECORD_KINDS: &str = "'commit', 'commits'";

/// Tuned on the retrieval cases (stage 3 Outcome has every run).
pub struct Tuning {
    /// bm25 weights for title (name, aliases, tags), heading, body.
    pub weights: [f64; 3],
    /// bm25 is negative; a chunk scoring above this is noise. Set for a vault of FLOOR_NOTES notes
    /// and scaled to the index's size (see `floor_for`).
    pub floor: f64,
    /// How many prompt tokens are kept, rarest first.
    pub tokens: usize,
    /// How many chunks are ranked before the best per note is kept.
    pub pool: usize,
}

/// 2026-11-22, on the 35 candidate cases and the 680 recorded prompts (stage 3 Outcome): 24
/// tokens (owner's pick over the spec's 8: long cases 10/10 against 6/10), title weighted 10 to the
/// heading's 2, and a floor of -9, under the weakest case hit (-9.8), which keeps 62 of 68
/// one-to-three-word prompts ("ok", "continue") from injecting anything.
pub const TUNED: Tuning = Tuning {
    weights: [10.0, 2.0, 1.0],
    floor: -9.0,
    tokens: 24,
    pool: 24,
};

/// The note count the floor was tuned at.
const FLOOR_NOTES: f64 = 2004.0;

/// bm25 grows with the log of the collection's size, so a fixed floor would let nothing through on
/// a new 20-note vault. ponytail: scaled by ln(notes) / ln(FLOOR_NOTES); re-tune when the cases
/// say a vault of another size ranks differently.
fn floor_for(conn: &Connection, floor: f64) -> Result<f64, Fault> {
    let notes: f64 =
        conn.query_row("SELECT count(*) FROM notes", [], |r| r.get::<_, i64>(0))? as f64;
    Ok(floor * notes.max(2.0).ln() / FLOOR_NOTES.ln())
}

pub struct Hit {
    pub name: String,
    pub path: String,
    pub heading: String,
    pub snippet: String,
    pub body: String,
    pub score: f64,
}

/// Lower-cased words of 3+ letters or digits, not stop words, first occurrence kept. Unicode
/// letters count, so a Turkish word is one token, not three fragments.
pub fn tokens(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(w))
        .filter(|w| seen.insert(w.to_string()))
        .take(64) // ponytail: a pasted log can hold thousands; 64 candidates is plenty to pick 8 rare ones from
        .map(str::to_string)
        .collect()
}

/// The `n` tokens fewest chunks hold, counted through the same tokenizer as the index (so
/// `guards` is counted as `guard`). A token no chunk holds is dropped: it cannot rank anything.
fn rarest(conn: &Connection, toks: Vec<String>, n: usize) -> Result<Vec<String>, Fault> {
    // Counted up to 2,000 chunks: past that a word is common, and how common no longer changes the pick.
    let mut count = conn
        .prepare("SELECT count(*) FROM (SELECT 1 FROM chunks WHERE chunks MATCH ?1 LIMIT 2000)")?;
    let mut scored = Vec::new();
    for t in toks {
        let c: i64 = count.query_row([format!("\"{t}\"")], |r| r.get(0))?;
        if c > 0 {
            scored.push((c, t));
        }
    }
    scored.sort_by_key(|(c, _)| *c); // stable: ties keep the prompt's order
    Ok(scored.into_iter().take(n).map(|(_, t)| t).collect())
}

/// The best chunk of each note, best notes first, at most `limit`. `kind`: "" leaves records out,
/// "all" keeps everything, anything else is that kind only.
pub fn search(
    conn: &Connection,
    text: &str,
    limit: usize,
    kind: &str,
    scope: Option<&str>,
    t: &Tuning,
) -> Result<Vec<Hit>, Fault> {
    let rare = rarest(conn, tokens(text), t.tokens)?;
    if rare.is_empty() {
        return Ok(Vec::new());
    }
    let matcher = rare
        .iter()
        .map(|w| format!("\"{w}\""))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut filter = match kind {
        "" => format!("AND notes.kind NOT IN ({RECORD_KINDS})"),
        "all" => String::new(),
        _ => "AND notes.kind = ?6".to_string(),
    };
    // A repo name, checked to be a plain folder name before it is written into the SQL.
    if let Some(repo) = scope {
        if repo.is_empty()
            || !repo
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(Fault::Usage(format!("--repo {repo}: not a checkout name")));
        }
        filter.push_str(&format!(" AND notes.path LIKE '{repo}/%'"));
    }
    let sql = format!(
        "SELECT notes.name, notes.path, chunks.heading, snippet(chunks, 2, '', '', '…', 26), chunks.body,
                bm25(chunks, ?2, ?3, ?4) AS score
         FROM chunks JOIN notes ON notes.id = chunks.note
         WHERE chunks MATCH ?1 {filter}
         ORDER BY score LIMIT ?5"
    );
    let mut stmt = conn.prepare(&sql)?;
    let map = |r: &rusqlite::Row| {
        let path: String = r.get(1)?;
        Ok(Hit {
            name: crate::vault::name_of(&path).to_string(),
            path,
            heading: r.get(2)?,
            snippet: r.get(3)?,
            body: r.get(4)?,
            score: r.get(5)?,
        })
    };
    let [wt, wh, wb] = t.weights;
    let rows = if kind.is_empty() || kind == "all" {
        stmt.query_map(rusqlite::params![matcher, wt, wh, wb, t.pool as i64], map)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(
            rusqlite::params![matcher, wt, wh, wb, t.pool as i64, kind.to_lowercase()],
            map,
        )?
        .collect::<Result<Vec<_>, _>>()?
    };
    let floor = floor_for(conn, t.floor)?;
    let mut seen = HashSet::new();
    Ok(rows
        .into_iter()
        .filter(|h| h.score < floor && seen.insert(h.path.clone()))
        .take(limit)
        .collect())
}

/// `--weights a,b,c --floor x --tokens n --pool n` override the tuned values: the scorer's knobs.
fn tuning(args: &Args) -> Result<Tuning, Fault> {
    let mut t = Tuning {
        weights: TUNED.weights,
        floor: args.num("floor", TUNED.floor)?,
        tokens: args.num("tokens", TUNED.tokens)?,
        pool: args.num("pool", TUNED.pool)?,
    };
    if let Some(w) = args.get("weights") {
        let parts: Vec<f64> = w.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        t.weights = parts.try_into().map_err(|_| {
            Fault::Usage("--weights takes three numbers: title,heading,body".into())
        })?;
    }
    Ok(t)
}

const KNOBS: [&str; 4] = ["weights", "floor", "tokens", "pool"];

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `--repo` takes an optional name: the next word when it is not a flag and a query is already
/// given, so `find "q" --repo` and `find "q" --repo Name` both read as meant.
fn repo_arg(raw: &[String]) -> (Vec<String>, Option<String>) {
    let mut rest = Vec::new();
    let mut repo = None;
    let mut it = raw.iter().peekable();
    while let Some(a) = it.next() {
        if a == "--repo" {
            let has_query = rest.iter().any(|w: &String| !w.starts_with("--"));
            repo = Some(match it.peek() {
                Some(n) if has_query && !n.starts_with("--") => {
                    it.next().cloned().unwrap_or_default()
                }
                _ => String::new(),
            });
        } else {
            rest.push(a.clone());
        }
    }
    (rest, repo)
}

pub fn run_find(raw: &[String]) -> Result<u8, Fault> {
    let (raw, repo) = repo_arg(raw);
    let args = Args::parse(&raw, &[&["limit", "kind", "budget"][..], &KNOBS].concat())?;
    let query = args.words.join(" ");
    if query.trim().is_empty() {
        return Err(Fault::Usage(
            "knowledge find <query> [--captures | --repo [<name>]]".into(),
        ));
    }
    let limit = args.num("limit", 6usize)?.clamp(1, 24);
    if args.has("captures") {
        let budget = args.num("budget", crate::capture::BUDGET)?.max(200);
        emit(&crate::capture::find(&query, limit, budget)?);
        return Ok(0);
    }
    let vault_cfg = config::load()?;
    let cfg = if repo.is_some() {
        vault_cfg.repos()
    } else {
        vault_cfg
    };
    let mut t = tuning(&args)?;
    // The floor keeps "ok" prompts from injecting vault notes; an asked-for search has no such risk.
    // Kept, it dropped every hit for "what each hook writes and when it runs" on a 28-note vault
    // (floor -3.9 there) and printed nothing (bench/tokens.ts, 2026-11-24). Ranking still orders them.
    if args.get("floor").is_none() {
        t.floor = f64::INFINITY;
    }
    let hits = search(
        &index::open_ro(&cfg)?,
        &query,
        limit,
        args.get("kind")
            .unwrap_or(if repo.is_some() { "all" } else { "" }),
        repo.as_deref().filter(|r| !r.is_empty()),
        &t,
    )?;
    if args.has("json") {
        let items: Vec<_> = hits
            .iter()
            .map(|h| {
                serde_json::json!({"name": h.name, "path": cfg.abs_of(&h.path), "rel": h.path, "heading": h.heading,
                                   "snippet": one_line(&h.snippet), "score": h.score})
            })
            .collect();
        emit(&format!("{}\n", serde_json::Value::Array(items)));
    } else if hits.is_empty() {
        emit(&format!("no note holds a word of \"{query}\"\n"));
    } else {
        let mut out = String::new();
        for h in &hits {
            out.push_str(&format!(
                "{} — {}\n  {}\n",
                h.name,
                cfg.abs_of(&h.path).display(),
                one_line(&h.snippet)
            ));
        }
        emit(&out);
    }
    Ok(0)
}

/// How injected notes are written (K34): today's Markdown lines, or one tagged block.
#[derive(Clone, Copy, PartialEq)]
pub enum Format {
    Lines,
    Tagged,
}

/// The passage rule (G16): the top note's best chunk is printed whole when it is at most `max`
/// chars and scores at least `score`.
#[derive(Clone, Copy)]
pub struct Passage {
    pub max: usize,
    pub score: f64,
}

pub const FORMAT: Format = Format::Lines;
/// Measured 2026-11-22 on the 35 cases (stage 3 Outcome): Markdown lines, median 327 tokens and 11
/// answers held with no passage; with this passage, median 370 and 14. The tagged block was larger
/// (424 median) and held no more, so K34's tagged block lost on the numbers.
pub const PASSAGE: Option<Passage> = Some(Passage {
    max: 1500,
    score: -30.0,
});
/// The block's hard cap, in chars.
const CAP: usize = 1800;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The text a prompt gets, or None when nothing clears the floor.
pub fn inject(
    conn: &Connection,
    cfg: &Config,
    text: &str,
    format: Format,
    passage: Option<Passage>,
    t: &Tuning,
) -> Result<Option<String>, Fault> {
    let hits = search(conn, text, 6, "", None, t)?;
    if hits.is_empty() {
        return Ok(None);
    }
    let vault = cfg.vault.display();
    let mut items = String::new();
    for (i, h) in hits.iter().enumerate() {
        let whole = passage.is_some_and(|p| {
            i == 0 && h.body.len() <= p.max && h.score <= p.score && !h.body.trim().is_empty()
        });
        let text = if whole {
            h.body.trim().to_string()
        } else {
            one_line(&h.snippet)
        };
        let item = match format {
            Format::Lines if whole => format!("- **{}** § {}:\n{}\n", h.name, h.heading, text),
            Format::Lines => format!("- **{}** — {}\n", h.name, text),
            Format::Tagged => format!(
                "<note name=\"{}\" path=\"{}\">{}</note>\n",
                esc(&h.name),
                esc(&h.path),
                esc(&text)
            ),
        };
        if items.len() + item.len() > CAP {
            break;
        }
        items.push_str(&item);
    }
    if items.is_empty() {
        return Ok(None);
    }
    Ok(Some(match format {
        Format::Lines => format!(
            "Relevant notes from the knowledge vault ({vault}):\n{items}\nThese are vault notes only. A snippet is not the note — follow the name into the file."
        ),
        Format::Tagged => format!(
            "<vault-notes vault=\"{}\">\n{items}</vault-notes>\nA snippet is not the note: open the file before relying on it.",
            esc(&vault.to_string())
        ),
    }))
}

pub fn run_inject(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(
        raw,
        &[&["format", "passage-max", "passage-score"][..], &KNOBS].concat(),
    )?;
    let cfg = config::load()?;
    let format = match args.get("format") {
        None => FORMAT,
        Some("lines") => Format::Lines,
        Some("tagged") => Format::Tagged,
        Some(f) => return Err(Fault::Usage(format!("--format lines|tagged, not {f}"))),
    };
    let base = PASSAGE.unwrap_or(Passage {
        max: 1500,
        score: -12.0,
    });
    let passage = if args.has("no-passage") {
        None
    } else if args.has("passage") || args.has("passage-max") || args.has("passage-score") {
        Some(Passage {
            max: args.num("passage-max", base.max)?,
            score: args.num("passage-score", base.score)?,
        })
    } else {
        PASSAGE
    };
    if let Some(block) = inject(
        &index::open_ro(&cfg)?,
        &cfg,
        &args.words.join(" "),
        format,
        passage,
        &tuning(&args)?,
    )? {
        emit(&format!("{block}\n"));
    }
    Ok(0)
}
