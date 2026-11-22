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
}

/// The keys the bun hooks read, validated the same way. Only `vault` is used by the binary today;
/// the rest are checked so a config the hooks would reject is rejected here too.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    vault: Option<String>,
    dev_root: Option<String>,
    os: Option<String>,
    rollover_hour: Option<serde_json::Number>,
    pull_trust: Option<String>,
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn config_path() -> PathBuf {
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
    if let Some(h) = raw.rollover_hour
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
    let _ = raw.os;
    let home = env_path("KNOWLEDGE_HOME")
        .unwrap_or_else(|| path.parent().map(Path::to_path_buf).unwrap_or_default());
    Ok(Config {
        vault: PathBuf::from(vault),
        db: home.join("vault-index.db"),
        dev: PathBuf::from(dev),
        home,
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
    pub fn abs_of(&self, rel: &str) -> PathBuf {
        let sep = std::path::MAIN_SEPARATOR_STR;
        let root = self.vault.to_string_lossy().replace(['/', '\\'], sep);
        PathBuf::from(format!(
            "{}{sep}{}",
            root.trim_end_matches(sep),
            rel.replace('/', sep)
        ))
    }
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
