//! This machine's config (spec D16). The lookup is the same as `hooks/skeleton-config.mjs`, so the
//! binary and the bun hooks cannot disagree about which vault they serve:
//!   1. $KNOWLEDGE_CONFIG, a file
//!   2. $KNOWLEDGE_HOME/config.json
//!   3. $XDG_CONFIG_HOME/knowledge/config.json
//!   4. ~/.config/knowledge/config.json, on every OS (never %APPDATA%: Claude desktop's MSIX package
//!      redirects writes there)
//!
//! KNOWLEDGE_HOME, or the folder the config was found in, also holds the index (K7).
use crate::Fault;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Config {
    /// The root the index's paths are relative to: the vault, as written in config.json, or the
    /// checkouts folder for the repo-docs index (`repos()`).
    pub vault: PathBuf,
    /// Where the indexes live: KNOWLEDGE_HOME, else the config's folder.
    pub home: PathBuf,
    /// The checkouts folder (`devRoot`).
    pub dev: PathBuf,
    /// The index file this config reads and writes.
    pub db: PathBuf,
    /// The label stamped on records (`windows`, `arch`): config's `os`, else detected.
    pub os: String,
    /// Free text the session start's Machine block opens with.
    pub machine: String,
    /// When a daily note starts, local time: a session at 02:00 belongs to the day before with 5.
    pub rollover: i64,
    /// `pullTrust: "signed"`: pulls take only a tip signed by a key in `trust_dir/allowed_signers`.
    pub pull_trust: bool,
    /// The folder config.json is in: allowed_signers and this boot's signing key.
    pub trust_dir: PathBuf,
}

/// The keys the bun hooks read, validated the same way.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    vault: Option<String>,
    dev_root: Option<String>,
    os: Option<String>,
    machine: Option<String>,
    rollover_hour: Option<serde_json::Number>,
    pull_trust: Option<String>,
    folders: Option<std::collections::BTreeMap<String, String>>,
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

pub fn config_path() -> PathBuf {
    if let Some(p) = env_path("KNOWLEDGE_CONFIG") {
        return p;
    }
    if let Some(h) = env_path("KNOWLEDGE_HOME") {
        return h.join("config.json");
    }
    let base = env_path("XDG_CONFIG_HOME")
        .unwrap_or_else(|| std::env::home_dir().unwrap_or_default().join(".config"));
    base.join("knowledge").join("config.json")
}

pub fn load() -> Result<Config, Fault> {
    let path = config_path();
    let src = path.display();
    let text = fs::read_to_string(&path)
        .map_err(|e| Fault::Config(format!("knowledge config {src}: {e}")))?;
    let raw: Raw = serde_json::from_str(&text)
        .map_err(|e| Fault::Config(format!("knowledge config {src} is not valid JSON: {e}")))?;
    let need = |v: Option<String>, k: &str| {
        v.filter(|s| !s.is_empty()).ok_or_else(|| {
            Fault::Config(format!(
                "knowledge config {src}: \"{k}\" must be a non-empty path"
            ))
        })
    };
    let vault = need(raw.vault, "vault")?;
    let dev = need(raw.dev_root, "devRoot")?;
    if let Some(h) = &raw.rollover_hour
        && h.as_u64().is_none_or(|h| h > 23)
    {
        return Err(Fault::Config(format!(
            "knowledge config {src}: \"rolloverHour\" must be an integer 0-23"
        )));
    }
    // A misspelt trust setting must not quietly mean "no check".
    if raw.pull_trust.as_deref().is_some_and(|t| t != "signed") {
        return Err(Fault::Config(format!(
            "knowledge config {src}: \"pullTrust\" must be \"signed\" or absent"
        )));
    }
    // `layout()` falls back to the defaults on a bad `folders`; here it is an error, so the doctor
    // and setup say why instead of a renamed folder silently reading as missing.
    folders_of(raw.folders).map_err(|e| Fault::Config(format!("knowledge config {src}: {e}")))?;
    let os = raw.os.filter(|s| !s.is_empty()).unwrap_or_else(|| {
        match std::env::consts::OS {
            "windows" => "windows",
            "linux" => "arch",
            other => other,
        }
        .to_string()
    });
    let trust_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let home = env_path("KNOWLEDGE_HOME")
        .unwrap_or_else(|| path.parent().map(Path::to_path_buf).unwrap_or_default());
    Ok(Config {
        vault: PathBuf::from(vault),
        db: home.join("vault-index.db"),
        dev: PathBuf::from(dev),
        home,
        os,
        machine: raw.machine.unwrap_or_default(),
        rollover: raw
            .rollover_hour
            .as_ref()
            .and_then(serde_json::Number::as_i64)
            .unwrap_or(5),
        pull_trust: raw.pull_trust.is_some(),
        trust_dir,
    })
}

impl Config {
    /// The repo-docs index (Stage 4): the same schema and code as the vault's, in its own file,
    /// with paths relative to the checkouts folder (`<Name>/docs/x.md`). Kept apart so the vault's
    /// links, graph, checks and prompt injection never see a repo doc.
    pub fn repos(&self) -> Config {
        Config {
            vault: self.dev.clone(),
            db: self.home.join("repos-index.db"),
            ..self.clone()
        }
    }

    /// A path's stored form: vault-relative, forward slashes, in the case it has on disk. None
    /// when it is outside the vault. Windows compares drive and folders case-insensitively.
    pub fn rel_of(&self, p: &Path) -> Option<String> {
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            std::env::current_dir().ok()?.join(p)
        };
        let abs = slashes(&canonical(&abs));
        let root = slashes(&canonical(&self.vault));
        let root = root.trim_end_matches('/');
        let head = abs.get(..root.len())?;
        let same = if cfg!(windows) {
            head.eq_ignore_ascii_case(root)
        } else {
            head == root
        };
        let rest = abs[root.len()..].strip_prefix('/')?;
        (same && !rest.is_empty()).then(|| rest.to_string())
    }

    /// The shown form: absolute and native.
    pub fn abs_of(&self, rel: impl std::fmt::Display) -> PathBuf {
        let rel = rel.to_string();
        let sep = std::path::MAIN_SEPARATOR_STR;
        let root = self.vault.to_string_lossy().replace(['/', '\\'], sep);
        PathBuf::from(format!(
            "{}{sep}{}",
            root.trim_end_matches(sep),
            rel.replace('/', sep)
        ))
    }
}

/// The vault's folders by `config.json` key, with the owner's layout as each default
/// (`skeleton-config.mjs`'s FOLDERS). The only place a folder name is written (D12).
const FOLDERS: [(&str, &str); 9] = [
    ("meta", "00-Meta"),
    ("repos", "10-Repos"),
    ("knowledge", "20-Knowledge"),
    ("sessions", "30-Sessions"),
    ("plans", "40-Plans"),
    ("ops", "50-Ops"),
    ("decisions", "60-Decisions"),
    ("assets", "80-Assets"),
    ("inbox", "90-Inbox"),
];

/// A vault folder, by its place in FOLDERS. It prints and joins like the `&str` constant it
/// replaced (`vault.join(OPS)`, `format!("{OPS}/x")`) and reads the name in effect, so a folder
/// renamed in `config.json` reaches every caller without each one carrying the config.
#[derive(Clone, Copy)]
pub struct Folder(usize);

pub const META: Folder = Folder(0);
pub const REPOS: Folder = Folder(1);
pub const KNOWLEDGE: Folder = Folder(2);
pub const SESSIONS: Folder = Folder(3);
pub const PLANS: Folder = Folder(4);
pub const OPS: Folder = Folder(5);
pub const DECISIONS: Folder = Folder(6);

impl Folder {
    pub fn name(self) -> &'static str {
        layout()[self.0]
    }
}

impl std::fmt::Display for Folder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl AsRef<Path> for Folder {
    fn as_ref(&self) -> &Path {
        Path::new(self.name())
    }
}

/// The folder names in effect, in FOLDERS' order: the config's `folders` over the defaults. Read
/// once per process on first use, without `load`, so a path built before the config is loaded
/// agrees with one built after; an unreadable config or a bad `folders` gives the defaults here,
/// and `load` reports the reason.
pub fn layout() -> &'static [&'static str; 9] {
    static LAYOUT: std::sync::OnceLock<[&'static str; 9]> = std::sync::OnceLock::new();
    LAYOUT.get_or_init(|| {
        fs::read_to_string(config_path())
            .ok()
            .and_then(|t| serde_json::from_str::<Raw>(&t).ok())
            .and_then(|r| folders_of(r.folders).ok())
            .unwrap_or(FOLDERS.map(|(_, d)| d))
    })
}

/// `folders` checked: a known key, a relative folder written with `/` (one note's links resolve
/// on both OSes), and no two keys naming one folder, which would file, say, the session notes
/// with the records. The names are leaked once per process: nine short strings.
fn folders_of(
    m: Option<std::collections::BTreeMap<String, String>>,
) -> Result<[&'static str; 9], String> {
    let mut out = FOLDERS.map(|(_, d)| d);
    for (k, v) in m.unwrap_or_default() {
        let Some(i) = FOLDERS.iter().position(|(key, _)| *key == k) else {
            let keys: Vec<&str> = FOLDERS.iter().map(|(k, _)| *k).collect();
            return Err(format!(
                "\"folders\" has no key \"{k}\" ({})",
                keys.join(", ")
            ));
        };
        let relative = !v.contains(['\\', ':'])
            && v.split('/').all(|s| !s.is_empty() && s != "." && s != "..");
        if !relative {
            return Err(format!(
                "\"folders.{k}\" must be a relative folder written with /, like \"{}\"",
                FOLDERS[i].1
            ));
        }
        out[i] = Box::leak(v.into_boxed_str());
    }
    for (i, a) in out.iter().enumerate() {
        if let Some(j) = out[i + 1..].iter().position(|b| a.eq_ignore_ascii_case(b)) {
            return Err(format!(
                "\"folders\": {} and {} both name \"{a}\"",
                FOLDERS[i].0,
                FOLDERS[i + 1 + j].0
            ));
        }
    }
    Ok(out)
}

/// `knowledge config [<key>]`: the resolved config as JSON, or one value, for a skill or command
/// that needs a path (`skeleton-config.mjs`'s keys, plus `home` and `skeleton`). It is what lets a
/// skill loaded by Codex or opencode run with no `~/.claude/hooks`.
pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let cfg = load()?;
    let shown = |p: &Path| p.display().to_string();
    // The checkout the bun scripts are in, until Stage 9 ports them; empty when none is found.
    let skeleton = crate::setup::checkout(&cfg.dev).map_or_else(String::new, |p| shown(&p));
    let values = serde_json::json!({
        "vault": shown(&cfg.vault),
        "devRoot": shown(&cfg.dev),
        "os": cfg.os,
        "machine": cfg.machine,
        "rolloverHour": cfg.rollover,
        "pullTrust": if cfg.pull_trust { "signed" } else { "" },
        "trustDir": shown(&cfg.trust_dir),
        "source": shown(&config_path()),
        "home": shown(&cfg.home),
        "skeleton": skeleton,
    });
    match raw {
        [] => crate::emit(&format!("{}\n", serde_json::to_string_pretty(&values)?)),
        [key] => match (values.get(key), FOLDERS.iter().position(|(k, _)| k == key)) {
            (Some(serde_json::Value::String(s)), _) if key == "skeleton" && s.is_empty() => {
                return Err(Fault::Config(
                    "no skeleton checkout found: run `knowledge setup --plan`".into(),
                ));
            }
            (Some(serde_json::Value::String(s)), _) => crate::emit(&format!("{s}\n")),
            (Some(v), _) => crate::emit(&format!("{v}\n")),
            (None, Some(i)) => crate::emit(&format!("{}\n", layout()[i])),
            (None, None) => {
                let keys: Vec<&str> = values
                    .as_object()
                    .into_iter()
                    .flat_map(|o| o.keys().map(String::as_str))
                    .chain(FOLDERS.iter().map(|(k, _)| *k))
                    .collect();
                return Err(Fault::Usage(format!(
                    "config: unknown key \"{key}\": {}",
                    keys.join(", ")
                )));
            }
        },
        _ => return Err(Fault::Usage("knowledge config [<key>]".into())),
    }
    Ok(0)
}

/// The path with its on-disk case, when it exists. Windows' canonical form starts `\\?\`, which
/// no other tool prints, so it is dropped.
fn canonical(p: &Path) -> PathBuf {
    match fs::canonicalize(p) {
        Ok(c) => {
            let s = c.to_string_lossy();
            s.strip_prefix(r"\\?\")
                .filter(|r| !r.starts_with("UNC"))
                .map_or(c.clone(), PathBuf::from)
        }
        Err(_) => p.to_path_buf(),
    }
}

fn slashes(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}
