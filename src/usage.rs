//! The token, model and cost digest from Claude Code's transcripts (brain-usage.mjs, ported in
//! Stage 6). It reads `~/.claude/projects/**/*.jsonl` and sums `message.usage` by logical day and
//! model.
//!
//!   knowledge usage [--days N]            last N days (30), to stdout
//!   knowledge usage --month YYYY-MM       writes 50-Ops/Usage/usage-<month>-<os>.md
//!   ... --projects | --blocks             by project; rolling 5-hour windows
//! The capture hook runs `--month` once per logical day.
use crate::state::{self, OPS};
use crate::{Args, Fault, emit};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// USD per million tokens. Not authoritative: token counts are exact, these rates are not.
/// Cache reads bill at 0.1x input, cache writes at 1.25x.
const PRICING: [(&str, f64, f64); 4] = [
    ("opus", 15.0, 75.0),
    ("sonnet", 3.0, 15.0),
    ("haiku", 1.0, 5.0),
    ("fable", 3.0, 15.0),
];

fn cost(model: &str, t: &[u64; 5]) -> Option<f64> {
    let m = model.to_lowercase();
    let (_, i, o) = PRICING.iter().find(|(k, _, _)| m.contains(k))?;
    let [inp, out, cread, ccreate, _] = t.map(|v| v as f64);
    Some((inp * i + ccreate * i * 1.25 + cread * i * 0.1 + out * o) / 1_000_000.0)
}

/// JavaScript's `toFixed(d)`: the double's exact decimal value, a tie rounded up (Rust rounds a tie
/// to even, and 1.25 must print 1.3 here as it does in bun).
fn to_fixed(x: f64, d: usize) -> String {
    let exact = format!("{x:.60}");
    let (int, frac) = exact.split_once('.').unwrap_or((&exact, ""));
    let keep = &frac[..d.min(frac.len())];
    let rest = &frac[d.min(frac.len())..];
    let up = rest.as_bytes().first().is_some_and(|&b| b >= b'5');
    let mut digits: Vec<u8> = format!("{int}{keep}").into_bytes();
    if up {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, b'1');
                break;
            }
            i -= 1;
            if digits[i] == b'-' {
                digits.insert(i + 1, b'1');
                break;
            }
            if digits[i] == b'9' {
                digits[i] = b'0';
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let s = String::from_utf8(digits).unwrap_or_default();
    if d == 0 {
        return s;
    }
    let (a, b) = s.split_at(s.len() - d);
    format!("{a}.{b}")
}

fn human(n: u64) -> String {
    for (unit, div) in [("B", 1e9), ("M", 1e6), ("k", 1e3)] {
        if n as f64 >= div {
            return format!("{}{unit}", to_fixed(n as f64 / div, 1));
        }
    }
    n.to_string()
}

fn group(int: &str) -> String {
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `$1,234.56`, as `toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })`.
fn usd(c: f64) -> String {
    let s = to_fixed(c, 2);
    let (int, frac) = s.split_once('.').unwrap_or((&s, "00"));
    format!("${}.{frac}", group(int))
}

fn add(into: &mut [u64; 5], v: &[u64; 5]) {
    for i in 0..5 {
        into[i] += v[i];
    }
}

struct Ev {
    ms: i64,
    project: String,
    model: String,
    t: [u64; 5],
}

fn jsonl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            jsonl_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
}

fn read_events(root: &Path) -> Vec<Ev> {
    let mut files = Vec::new();
    jsonl_files(root, &mut files);
    let mut events = Vec::new();
    for path in files {
        let project = path
            .parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(bytes) = fs::read(&path) else { continue };
        let text = String::from_utf8_lossy(&bytes);
        for line in text.split('\n') {
            if !line.contains("\"usage\"") {
                continue;
            }
            let Ok(rec) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let msg = &rec["message"];
            let u = &msg["usage"];
            if !u.is_object() {
                continue;
            }
            let Some(ms) = rec["timestamp"]
                .as_str()
                .filter(|s| !s.is_empty())
                .and_then(state::parse_ms)
            else {
                continue;
            };
            let n = |v: &Value| v.as_u64().unwrap_or(0);
            events.push(Ev {
                ms,
                project: project.clone(),
                model: msg["model"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("unknown")
                    .to_string(),
                t: [
                    n(&u["input_tokens"]),
                    n(&u["output_tokens"]),
                    n(&u["cache_read_input_tokens"]),
                    n(&u["cache_creation_input_tokens"]),
                    n(&u["output_tokens_details"]["thinking_tokens"]),
                ],
            });
        }
    }
    events
}

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &["month", "days"])?;
    let cfg = crate::config::load()?;
    let month = args.get("month").map(str::to_string);
    let days: i64 = args.num("days", 30)?;
    let root = state::home().join(".claude").join("projects");
    if !root.exists() {
        emit(&format!("no transcripts at {}\n", state::native(&root)));
        return Ok(1);
    }
    let mut events = read_events(&root);
    if events.is_empty() {
        emit("no usage records found\n");
        return Ok(1);
    }
    match &month {
        Some(m) => events.retain(|e| state::ym(&state::local(e.ms)) == *m),
        None => {
            let cutoff = state::now_ms() - days * 86_400_000;
            events.retain(|e| e.ms >= cutoff);
        }
    }
    if events.is_empty() {
        emit("no usage records in range\n");
        return Ok(1);
    }
    let mut by_day: BTreeMap<String, BTreeMap<String, [u64; 5]>> = BTreeMap::new();
    let mut by_project: Vec<(String, [u64; 5])> = Vec::new();
    let mut project_at: HashMap<String, usize> = HashMap::new();
    for e in &events {
        if e.t.iter().all(|&v| v == 0) {
            continue; // <synthetic> placeholder rows carry no tokens
        }
        let day = state::ymd(&state::logical(&cfg, e.ms));
        add(
            by_day
                .entry(day)
                .or_default()
                .entry(e.model.clone())
                .or_default(),
            &e.t,
        );
        let i = *project_at.entry(e.project.clone()).or_insert_with(|| {
            by_project.push((e.project.clone(), [0; 5]));
            by_project.len() - 1
        });
        add(&mut by_project[i].1, &e.t);
    }
    let pad = |n: i64| format!("{n:02}");
    let mut out: Vec<String> = Vec::new();
    out.push(format!(
        "# Claude usage — {}",
        month.clone().unwrap_or_else(|| format!("last {days} days"))
    ));
    out.push(String::new());
    out.push(format!(
        "Rollover {}:00 local. {} assistant messages across {} days.",
        pad(cfg.rollover),
        group(&events.len().to_string()),
        by_day.len()
    ));
    out.push(String::new());
    out.push("| Date | Model | In | Out | Think | Cache rd | Cache wr | Hit % | Est. USD |".into());
    out.push("|---|---|--:|--:|--:|--:|--:|--:|--:|".into());
    let mut grand = [0u64; 5];
    let mut grand_cost = 0.0;
    let mut unpriced: Vec<String> = Vec::new();
    for (day, models) in by_day.iter().rev() {
        for (model, t) in models {
            add(&mut grand, t);
            let [i, o, cr, cc, th] = *t;
            let total_in = i + cr + cc;
            let hit = if total_in > 0 {
                format!("{}%", (cr as f64 / total_in as f64 * 100.0 + 0.5).floor())
            } else {
                "-".into()
            };
            let cstr = match cost(model, t) {
                None => {
                    if !unpriced.contains(model) {
                        unpriced.push(model.clone());
                    }
                    "?".into()
                }
                Some(c) => {
                    grand_cost += c;
                    usd(c)
                }
            };
            out.push(format!(
                "| {day} | {} | {} | {} | {} | {} | {} | {hit} | {cstr} |",
                model.replacen("claude-", "", 1),
                human(i),
                human(o),
                human(th),
                human(cr),
                human(cc)
            ));
        }
    }
    let [gi, go, gcr, gcc, gth] = grand;
    let g_in = gi + gcr + gcc;
    out.extend(["".into(), "## Total".into(), "".into()]);
    out.push(format!(
        "- input **{}** · output **{}** · thinking **{}**",
        human(gi),
        human(go),
        human(gth)
    ));
    out.push(format!(
        "- cache read **{}** · cache write **{}**",
        human(gcr),
        human(gcc)
    ));
    if g_in > 0 {
        out.push(format!(
            "- **cache hit rate {}%** — cache reads bill at 10% of input, so this is the number that moves cost",
            to_fixed(gcr as f64 / g_in as f64 * 100.0, 1)
        ));
    }
    out.push(format!(
        "- **{}** at pay-as-you-go API rates",
        usd(grand_cost)
    ));
    out.push("- On a **Pro subscription this is not what you paid** — it is what the same traffic would have cost via the API. Useful as a relative measure of which days and models are expensive, not as a bill.".into());
    out.push("- Rates in `brain-usage.mjs` are not authoritative. Token counts are exact.".into());
    if !unpriced.is_empty() {
        unpriced.sort();
        out.push(format!(
            "- unpriced models, excluded from cost: {}",
            unpriced.join(", ")
        ));
    }
    if args.has("projects") {
        out.extend([
            "".into(),
            "## By project".into(),
            "".into(),
            "| Project | In | Out | Cache rd |".into(),
            "|---|--:|--:|--:|".into(),
        ]);
        by_project.sort_by_key(|(_, t)| std::cmp::Reverse(t.iter().sum::<u64>()));
        for (p, t) in &by_project {
            out.push(format!(
                "| {p} | {} | {} | {} |",
                human(t[0]),
                human(t[1]),
                human(t[2])
            ));
        }
    }
    if args.has("blocks") {
        out.extend(["".into(), "## Rolling 5-hour blocks".into(), "".into()]);
        let mut blocks: BTreeMap<String, [u64; 5]> = BTreeMap::new();
        for e in &events {
            let z = state::local(e.ms);
            let h = i64::from(z.hour());
            let key = format!("{} {}:00", state::ymd(&z), pad(h - h % 5));
            add(blocks.entry(key).or_default(), &e.t);
        }
        out.push("| Block start | In | Out | Cache rd |".into());
        out.push("|---|--:|--:|--:|".into());
        for (k, t) in blocks.iter().rev().take(12) {
            out.push(format!(
                "| {k} | {} | {} | {} |",
                human(t[0]),
                human(t[1]),
                human(t[2])
            ));
        }
    }
    let text = out.join("\n");
    let Some(month) = month else {
        emit(&format!("{text}\n"));
        return Ok(0);
    };
    // One file per boot: each digest is built from this machine's transcripts only.
    let dest = cfg
        .vault
        .join(OPS)
        .join("Usage")
        .join(format!("usage-{month}-{}.md", cfg.os));
    let created = !dest.exists();
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir).map_err(|e| Fault::io(dir, e))?;
    }
    let today = state::ymd(&state::local(state::now_ms()));
    let header = format!(
        "---\ntype: usage\nproject: brain\ntags: [usage, tokens, cost]\ncreated: {today}\nupdated: {today}\nstatus: active\n---\n\n"
    );
    fs::write(&dest, format!("{header}{text}\n")).map_err(|e| Fault::io(&dest, e))?;
    // A new file is an orphan until the hubs link it: a script write fires no hook.
    if created {
        let _ = crate::hubs::write(&cfg);
    }
    emit(&format!("wrote {}\n", state::native(&dest)));
    Ok(0)
}
