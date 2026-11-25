//! `knowledge setup`: the engine on this machine, planned before anything is written (Stage 6; it
//! replaces install.ts after the Arch pass). The targets: the vault (new from template/, or
//! adopted), config.json, the links into ~/.claude, the Claude plugin that carries the hooks (and
//! its `knowledge_bin` option in ~/.claude/settings.json), the CLAUDE.md imports, the githooks path of every clone, the compile state,
//! and with --signing this boot's signing key. The graphify MCP entry that setup wrote before
//! Stage 8 is planned for removal (S8.5: the code map replaced it); one the owner added stays.
//!
//! `--plan` writes nothing. Its last line is the plan id: a hash of every line and of what each
//! target holds now, so `--apply <id>` recomputes the plan and refuses when anything moved in
//! between. Nothing is deleted: a real file where a link belongs moves to ~/.claude/.pre-skeleton/,
//! and settings.json is copied beside itself before a change. `--remove` never touches the vault.
//! `--harness codex` (S7.4) adds Codex beside Claude Code: its plugin, its AGENTS.md block, the
//! `knowledge mcp` retrieval server, from the config.json the Claude setup wrote. It never writes hook trust.
//! `--harness opencode` (S7.5): its plugin file, compiled into this binary, and its AGENTS.md block.
//! `--harness generic --instructions <file>` (S7.6): the inline block in any file a harness reads.
//! Nothing restores it at session start (a generic harness has no hooks): plan again to check it.
//! `--harness claude-desktop` (S7.7): `knowledge mcp` in the app's claude_desktop_config.json.
//! With no `--harness` (S7.7), one plan covers Claude Code and then every harness an adapter
//! detects, each under its own `found` line; `--remove` covers the same set.
use crate::config::{self, Config};
use crate::instructions::{self, Form};
use crate::state::{self, J};
use crate::{Args, Fault, compile, emit, session};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const USAGE: &str = "usage:
  knowledge setup --plan [--vault <path>] [--dev <path>] [--machine <text>] [--from-template | --adopt]
                         [--signing] [--skeleton <checkout>]       (Claude Code and every harness found)
  knowledge setup --apply <id> [the same options]
  knowledge setup --remove --plan | --remove --apply <id>
  knowledge setup --harness claude [the options above]               (Claude Code alone)
  knowledge setup --harness codex --plan | --apply <id> [--skeleton <checkout>]   (after the Claude setup)
  knowledge setup --harness codex --remove --plan | --remove --apply <id>
  knowledge setup --harness opencode [--remove] --plan | --apply <id>
  knowledge setup --harness claude-desktop [--remove] --plan | --apply <id>
  knowledge setup --harness generic --instructions <file> [--remove] --plan | --apply <id>";

const GIT: Duration = Duration::from_secs(20);

/// One write. A step holds the writes its line promises, in order.
enum Act {
    Template {
        from: PathBuf,
        vault: PathBuf,
        version: String,
    },
    Write {
        path: PathBuf,
        text: String,
        backup: Option<PathBuf>,
    },
    Mkdir(PathBuf),
    Unlink(PathBuf),
    Aside {
        path: PathBuf,
        to: PathBuf,
    },
    Link {
        path: PathBuf,
        target: PathBuf,
        dir: bool,
    },
    Git {
        repo: PathBuf,
        key: &'static str,
        value: Option<String>,
    },
    Keygen {
        key: PathBuf,
        comment: String,
    },
    Signer {
        file: PathBuf,
        key: PathBuf,
        email: String,
    },
    /// `claude mcp remove --scope local graphify`, run in this vault.
    McpRemove(PathBuf),
    /// Through the `claude` CLI: add the marketplace from this url, install the plugin, or
    /// uninstall both.
    Plugin {
        add: Option<String>,
        install: bool,
        remove: bool,
    },
    /// One `codex` CLI call: the marketplace, the plugin, or an MCP server.
    Codex(Vec<String>),
    /// config.toml read again when it runs (the `codex` calls before it write the same file),
    /// kept beside itself, then written without our `[hooks.state]` tables. Clearing trust is the
    /// one write setup makes there; it never adds trust (C1).
    Untrust(PathBuf),
    /// settings.json read again when it runs, since `claude plugin` writes it in the same apply,
    /// then [`swapped`] and written, kept beside itself first.
    Swap {
        file: PathBuf,
        bin: PathBuf,
        backup: Option<PathBuf>,
    },
}

struct Step {
    status: &'static str,
    name: &'static str,
    detail: String,
    /// What the targets hold now: folded into the plan id, never printed.
    seen: String,
    acts: Vec<Act>,
}

#[derive(Default)]
struct Plan {
    steps: Vec<Step>,
    next: Vec<String>,
}

impl Plan {
    fn say(&mut self, status: &'static str, name: &'static str, detail: impl Into<String>) {
        self.act(status, name, detail, String::new(), Vec::new());
    }
    fn act(
        &mut self,
        status: &'static str,
        name: &'static str,
        detail: impl Into<String>,
        seen: String,
        acts: Vec<Act>,
    ) {
        self.steps.push(Step {
            status,
            name,
            detail: detail.into(),
            seen,
            acts,
        });
    }
    /// Another plan's steps and next lines, after this one's.
    fn extend(&mut self, other: Plan) {
        self.steps.extend(other.steps);
        for n in other.next {
            if !self.next.contains(&n) {
                self.next.push(n);
            }
        }
    }
    fn failed(&self) -> bool {
        self.steps.iter().any(|s| s.status == "FAIL")
    }
    fn changes(&self) -> usize {
        self.steps.iter().filter(|s| !s.acts.is_empty()).count()
    }
    /// 64 bits of SHA-256: it tells a stale plan from a current one, it guards nothing secret.
    fn id(&self) -> String {
        let mut text = String::new();
        for s in &self.steps {
            text.push_str(&format!(
                "{}\t{}\t{}\n{}\n",
                s.status, s.name, s.detail, s.seen
            ));
        }
        hex(text.as_bytes())[..16].to_string()
    }
    fn lines(&self) -> String {
        self.steps
            .iter()
            .map(|s| format!("  {:<7} {:<16} {}\n", s.status, s.name, s.detail))
            .collect()
    }
}

fn hex(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn fwd(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn seen_file(p: &Path) -> String {
    fs::read(p).map_or_else(|_| "absent".into(), |b| hex(&b))
}

/// A link's state: absent, the target it names, or a real file or folder in its place.
fn seen_link(p: &Path) -> String {
    match fs::symlink_metadata(p) {
        Err(_) => "absent".into(),
        Ok(m) if m.file_type().is_symlink() => format!(
            "link {}",
            fs::read_link(p).map(|t| fwd(&t)).unwrap_or_default()
        ),
        Ok(_) => "real".into(),
    }
}

/// `~` expanded, made absolute: node's `resolve(p.replace(/^~/, homedir()))`.
fn expand(p: &str) -> PathBuf {
    let p = match p.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            state::home().join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(p),
    };
    std::path::absolute(&p).unwrap_or(p)
}

/// Links resolved, on-disk case, without Windows' `\\?\`.
fn real(p: &Path) -> Option<PathBuf> {
    let c = fs::canonicalize(p).ok()?;
    let s = c.to_string_lossy();
    Some(
        s.strip_prefix(r"\\?\")
            .map_or_else(|| c.clone(), PathBuf::from),
    )
}

fn same(a: &Path, b: &Path) -> bool {
    real(a).is_some_and(|x| real(b).is_some_and(|y| x == y))
}

/// A program on PATH, as the shell would find it (PATHEXT on Windows).
fn which(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_lowercase)
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|d| exts.iter().map(move |e| d.join(format!("{name}{e}"))))
        .find(|p| p.is_file())
}

fn git_get(repo: &Path, key: &str) -> String {
    state::git(repo, &["config", key], GIT).out().to_string()
}

fn os_tag() -> String {
    match std::env::consts::OS {
        "linux" => fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|t| {
                t.lines().find_map(|l| {
                    l.strip_prefix("ID=")
                        .map(|v| v.trim_matches(['"', '\'']).to_string())
                })
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "linux".into()),
        other => other.into(),
    }
}

/// The checkout the links point into: --skeleton, else the one ~/.claude/hooks already links,
/// else the one this binary was built in, else `<devRoot>/knowledge-skeleton`.
fn skeleton(args: &Args, dev: &Path) -> Option<PathBuf> {
    match args.get("skeleton") {
        Some(p) => Some(expand(p)).filter(|p| is_checkout(p)),
        None => checkout(dev),
    }
}

/// `~/.agents/skills/brain`: the shared skills folder's link to the /brain skill.
fn agents_brain() -> PathBuf {
    state::home().join(".agents").join("skills").join("brain")
}

fn is_checkout(p: &Path) -> bool {
    p.join(HOOKS).is_file() && p.join("template").is_dir()
}

/// The checkout found without `--skeleton`; `knowledge config skeleton` prints it.
pub fn checkout(dev: &Path) -> Option<PathBuf> {
    let linked = session::skeleton_root();
    if is_checkout(&linked) {
        return Some(linked);
    }
    if let Some(a) = std::env::current_exe()
        .ok()
        .and_then(|e| real(&e))
        .and_then(|e| {
            e.ancestors()
                .find(|a| is_checkout(a))
                .map(Path::to_path_buf)
        })
    {
        return Some(a);
    }
    let d = dev.join("knowledge-skeleton");
    is_checkout(&d).then_some(d)
}

fn pages(d: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(d) else { return };
    let mut es: Vec<_> = rd.flatten().collect();
    es.sort_by_key(fs::DirEntry::file_name);
    for e in es {
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            pages(&p, out);
        } else if p.extension().is_some_and(|x| x == "md") {
            out.push(p);
        }
    }
}

/// Each checkout a 10-Repos page names with `checkout:` (owner, Stage 5: githooks per checkout,
/// never a global core.hooksPath).
fn checkouts(vault: &Path, dev: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    pages(&vault.join(state::REPOS), &mut files);
    files
        .iter()
        .filter_map(|f| {
            state::read(f).lines().find_map(|l| {
                let v = l.strip_prefix("checkout:")?.trim();
                (!v.is_empty() && !v.contains(char::is_whitespace))
                    .then(|| dev.join(v.trim_matches(['"', '\''])))
            })
        })
        .collect()
}

/// ~/.claude/.pre-skeleton/<same path>, or -2, -3 when an earlier run kept one there. Outside
/// skills/ and commands/: Claude Code loads every folder in skills/, so a copy kept beside the
/// original would load as a second skill of the same name.
fn aside(claude: &Path, path: &Path) -> PathBuf {
    let rel = path.strip_prefix(claude).unwrap_or(path);
    (1..)
        .map(|n| {
            claude
                .join(if n == 1 {
                    ".pre-skeleton".into()
                } else {
                    format!(".pre-skeleton-{n}")
                })
                .join(rel)
        })
        .find(|to| fs::symlink_metadata(to).is_err())
        .unwrap_or_default()
}

// ---- the hook block ------------------------------------------------------------------------------

/// Ours: a brain-*.mjs under ~/.claude/hooks, or the knowledge binary's `hook` verb. Every other
/// hook (a plugin's, the user's) stays.
fn ours(cmd: &str) -> bool {
    let c = cmd.replace('\\', "/");
    const BRAIN: &str = "/.claude/hooks/brain-";
    if let Some(i) = c.find(BRAIN) {
        let rest = &c[i + BRAIN.len()..];
        let n = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
            .count();
        if n > 0 && rest[n..].starts_with(".mjs") {
            return true;
        }
    }
    c.match_indices(" hook ").any(|(i, _)| {
        let before = c[..i].trim_end_matches('"');
        before.ends_with("/knowledge") || before.ends_with("/knowledge.exe")
    })
}

fn arr(v: Option<&J>) -> &[J] {
    match v {
        Some(J::Arr(a)) => a,
        _ => &[],
    }
}

fn command(h: &J) -> &str {
    h.get("command").and_then(J::str).unwrap_or("")
}

/// Our hooks, one sortable line each: event, matcher, type, command, timeout, async.
fn sig(hooks: Option<&J>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(J::Obj(evs)) = hooks {
        for (ev, gs) in evs {
            for g in arr(Some(gs)) {
                for h in arr(g.get("hooks")).iter().filter(|h| ours(command(h))) {
                    let field = |k: &str, d: J| h.get(k).cloned().unwrap_or(d);
                    out.push(
                        J::Arr(vec![
                            J::s(ev.clone()),
                            J::s(g.get("matcher").and_then(J::str).unwrap_or("")),
                            field("type", J::Null),
                            J::s(command(h)),
                            field("timeout", J::Null),
                            field("async", J::Bool(false)),
                        ])
                        .text(),
                    );
                }
            }
        }
    }
    out.sort();
    out
}

/// An event's groups without our hooks; a group left with none goes.
fn strip(gs: Option<&J>) -> Vec<J> {
    arr(gs)
        .iter()
        .filter_map(|g| {
            let hs: Vec<J> = arr(g.get("hooks"))
                .iter()
                .filter(|h| !ours(command(h)))
                .cloned()
                .collect();
            (!hs.is_empty()).then(|| {
                let mut g = g.clone();
                g.set("hooks", J::Arr(hs));
                g
            })
        })
        .collect()
}

/// settings.json with every hook of ours taken out; a group or an event left empty goes.
fn without_ours(settings: &J) -> J {
    let mut hooks = Vec::new();
    if let Some(J::Obj(evs)) = settings.get("hooks") {
        for (ev, gs) in evs {
            let gs = strip(Some(gs));
            if !gs.is_empty() {
                hooks.push((ev.clone(), J::Arr(gs)));
            }
        }
    }
    let mut next = settings.clone();
    if hooks.is_empty() {
        next.remove("hooks");
    } else {
        next.set("hooks", J::Obj(hooks));
    }
    next
}

// ---- the Claude plugin ---------------------------------------------------------------------------

/// The plugin's hooks, in the checkout; it also marks a folder as a skeleton checkout.
const HOOKS: &str = "claude/hooks/hooks.json";
/// `<plugin>@<marketplace>`, as `.claude-plugin/marketplace.json` names them.
const PLUGIN: &str = "knowledge@knowledge-skeleton";
const MARKET: &str = "knowledge-skeleton";

/// settings.json after the swap (S7.3): no hook of ours, since the plugin carries them, and the
/// plugin's `knowledge_bin` set to the binary its exec-form hooks run.
fn swapped(settings: &J, bin: &Path) -> J {
    let mut next = without_ours(settings);
    let empty = || J::Obj(Vec::new());
    let mut configs = next.get("pluginConfigs").cloned().unwrap_or_else(empty);
    let mut mine = configs.get(PLUGIN).cloned().unwrap_or_else(empty);
    let mut options = mine.get("options").cloned().unwrap_or_else(empty);
    options.set("knowledge_bin", J::s(fwd(bin)));
    mine.set("options", options);
    configs.set(PLUGIN, mine);
    next.set("pluginConfigs", configs);
    next
}

/// settings.json with nothing of ours: no hook, and no option for the plugin.
fn unswapped(settings: &J) -> J {
    let mut next = without_ours(settings);
    if let Some(mut configs) = next.get("pluginConfigs").cloned() {
        configs.remove(PLUGIN);
        if configs == J::Obj(Vec::new()) {
            next.remove("pluginConfigs");
        } else {
            next.set("pluginConfigs", configs);
        }
    }
    next
}

/// The marketplace Claude Code knows, and the plugin version it installed, from its own records.
fn plugin_state(claude: &Path) -> (bool, Option<String>) {
    let plugins = claude.join("plugins");
    let known =
        read_obj(&plugins.join("known_marketplaces.json")).is_ok_and(|m| m.get(MARKET).is_some());
    let version = read_obj(&plugins.join("installed_plugins.json"))
        .ok()
        .and_then(|i| match i.get("plugins").and_then(|p| p.get(PLUGIN)) {
            Some(J::Arr(v)) => v
                .iter()
                .find_map(|r| r.get("version").and_then(J::str).map(String::from)),
            _ => None,
        });
    (known, version)
}

fn read_obj(file: &Path) -> Result<J, String> {
    match fs::read_to_string(file) {
        Err(_) => Ok(J::Obj(Vec::new())),
        Ok(t) => match serde_json::from_str::<J>(&t) {
            Ok(v @ J::Obj(_)) => Ok(v),
            _ => Err(format!(
                "{} is not a JSON object: fix it and rerun",
                fwd(file)
            )),
        },
    }
}

fn pretty(v: &J) -> String {
    format!("{}\n", serde_json::to_string_pretty(v).unwrap_or_default())
}

/// `<name>.pre-skeleton-<hash>` beside a JSON file setup is about to change, when there is one.
fn backup(file: &Path, seen: &str) -> Option<PathBuf> {
    let name = file.file_name()?.to_string_lossy();
    file.exists()
        .then(|| file.with_file_name(format!("{name}.pre-skeleton-{}", &seen[..8])))
}

// ---- CLAUDE.md -----------------------------------------------------------------------------------

/// The `@path` imports, as brain-doctor's `priming` check reads them: one per line, `~` expanded.
fn imports(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let p = l.strip_prefix('@')?.trim_end();
            (!p.is_empty() && !p.contains(char::is_whitespace)).then(|| {
                let p = match p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
                    Some(rest) => fwd(&state::home().join(rest)),
                    None => p.replace('\\', "/"),
                };
                p.to_lowercase()
            })
        })
        .collect()
}

// ---- the setup plan ------------------------------------------------------------------------------

/// What each missing tool costs, and how to install it.
const PREREQS: [(&str, &str, &str, &str); 2] = [
    (
        "claude",
        "the harness",
        "curl -fsSL https://claude.ai/install.sh | bash",
        "irm https://claude.ai/install.ps1 | iex",
    ),
    // records commits nothing without it: records push on their own, never unscanned.
    (
        "gitleaks",
        "records commit nothing unscanned",
        "your package manager (pacman -S gitleaks)",
        "winget install Gitleaks.Gitleaks",
    ),
];

/// The vault and the checkouts folder: the flags, else config.json, else ~/knowledge and ~/dev.
fn place(args: &Args, config: &J) -> Place {
    let pick = |flag: &str, key: &str, default: &str| {
        expand(
            args.get(flag)
                .or_else(|| config.get(key).and_then(J::str).filter(|s| !s.is_empty()))
                .unwrap_or(default),
        )
    };
    (
        pick("vault", "vault", "~/knowledge"),
        pick("dev", "devRoot", "~/dev"),
    )
}

fn plan_setup(args: &Args) -> Plan {
    let mut p = Plan::default();
    let home = state::home();
    let claude = home.join(".claude");
    if std::env::var_os("CLAUDE_CONFIG_DIR").is_some_and(|v| !v.is_empty()) {
        p.say("FAIL", "prerequisites", "CLAUDE_CONFIG_DIR is set; the hooks read ~/.claude and would not see that folder. Unset it and rerun");
        return p;
    }
    if which("git").is_none() {
        let how = if cfg!(windows) {
            "winget install Git.Git"
        } else {
            "your package manager (pacman -S git)"
        };
        p.say(
            "FAIL",
            "prerequisites",
            format!("git is not on PATH. Install: {how}"),
        );
        return p;
    }
    let mut have = vec!["git"];
    for (name, why, unix, win) in PREREQS {
        if which(name).is_some() {
            have.push(name);
        } else {
            p.say(
                "WARN",
                "prerequisites",
                format!(
                    "{name} is not on PATH ({why}). Install: {}",
                    if cfg!(windows) { win } else { unix }
                ),
            );
        }
    }
    p.say("ok", "prerequisites", have.join(" · "));
    if args.has("from-template") && args.has("adopt") {
        p.say("FAIL", "vault", "--from-template and --adopt do not mix");
        return p;
    }

    let cpath = config::config_path();
    let existing = match read_obj(&cpath) {
        Ok(v) => v,
        Err(e) => {
            p.say("FAIL", "config", e);
            return p;
        }
    };
    let str_of = |k: &str| {
        existing
            .get(k)
            .and_then(J::str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let (vault, dev) = place(args, &existing);
    // Claude Code's @import and the doctor's `priming` check read a path up to the first space.
    if fwd(&vault).contains(char::is_whitespace) {
        p.say(
            "FAIL",
            "vault",
            format!(
                "the vault path has a space in it ({}); ~/.claude/CLAUDE.md cannot import from it",
                fwd(&vault)
            ),
        );
        return p;
    }
    let Some(root) = skeleton(args, &dev) else {
        p.say(
            "FAIL",
            "skeleton",
            "no knowledge-skeleton checkout found: pass --skeleton <checkout>",
        );
        return p;
    };
    let version = session::json_version(&root.join("package.json")).unwrap_or_default();
    p.say("ok", "skeleton", format!("{} ({version})", fwd(&root)));

    // The vault: an empty path gets template/, anything else is adopted.
    let non_empty = fs::read_dir(&vault).is_ok_and(|mut d| d.next().is_some());
    let template = args.has("from-template") || (!args.has("adopt") && !non_empty);
    if template && non_empty {
        p.say(
            "FAIL",
            "vault",
            format!(
                "--from-template: {} is not empty. Pick an empty path, or --adopt it",
                fwd(&vault)
            ),
        );
    } else if !template && !vault.join(state::META).join("00-MOC-Root.md").is_file() {
        p.say("FAIL", "vault", format!("{} has no {}/00-MOC-Root.md, so it is not a vault this engine knows. Pass --vault <empty path> for a new one", fwd(&vault), state::META));
    } else if template {
        let mut n = 0;
        count(&root.join("template"), &mut n);
        let detail = format!(
            "{}: new, from template/ ({n} files), .skeleton.json pins {version}, git init, a first commit when git knows your user.email",
            fwd(&vault)
        );
        let seen = if vault.exists() { "empty" } else { "absent" };
        p.act(
            "create",
            "vault",
            detail,
            seen.into(),
            vec![Act::Template {
                from: root.join("template"),
                vault: vault.clone(),
                version: version.clone(),
            }],
        );
    } else {
        p.say("ok", "vault", format!("{} (adopted)", fwd(&vault)));
        // The pin is the vault's to commit, so an adopted vault without one is told, not written
        // to: two machines each writing it untracked would block each other's pull.
        match session::json_version(&vault.join(".skeleton.json")) {
            Some(pin) if pin == version => p.say("ok", "pin", format!(".skeleton.json pins {pin}")),
            Some(pin) => p.say("FAIL", "pin", format!(".skeleton.json pins {pin}, this skeleton is {version}: check out the skeleton the vault pins, or adapt the vault and bump the pin")),
            None => p.say("WARN", "pin", format!("no .skeleton.json: commit {{\"version\":\"{version}\"}} at the vault root so every machine checks it runs this skeleton")),
        }
    }

    // Obsidian on Linux rewrites .obsidian/*.json as 755, and a mode-only change blocks the pull.
    let has_git = |r: &Path| r.join(".git").exists() || (template && r == vault);
    if has_git(&vault) {
        let was = git_get(&vault, "core.fileMode");
        if was == "false" {
            p.say("ok", "core.fileMode", format!("{} false", fwd(&vault)));
        } else {
            let detail = format!(
                "{}/.git/config: false (Obsidian's mode-only rewrites would block the pull)",
                fwd(&vault)
            );
            p.act(
                "change",
                "core.fileMode",
                detail,
                was,
                vec![Act::Git {
                    repo: vault.clone(),
                    key: "core.fileMode",
                    value: Some("false".into()),
                }],
            );
        }
    } else {
        p.say(
            "WARN",
            "core.fileMode",
            format!(
                "{} is not a git repository; sync between machines needs one",
                fwd(&vault)
            ),
        );
    }

    // githooks/pre-commit guards every commit in the vault, the skeleton and each named checkout.
    // A checkout's own hooks folder is kept in knowledge.ownHooksPath and runs first.
    let hooks_dir = fwd(&root.join("githooks"));
    let mut repos: Vec<PathBuf> = Vec::new();
    for r in [vault.clone(), root.clone()]
        .into_iter()
        .chain(checkouts(&vault, &dev))
    {
        if has_git(&r) && !repos.iter().any(|x| x == &r || same(x, &r)) {
            repos.push(r);
        }
    }
    for repo in &repos {
        let was = git_get(repo, "core.hooksPath");
        // Compared as folders, not strings: after an apply the skeleton is found through the
        // `~/.claude/hooks` link, whose target can be spelled otherwise (an 8.3 name, a drive
        // letter's case, a symlinked dev folder). As strings, our own githooks read as the
        // checkout's own, and the next plan would chain it to itself (2026-11-24).
        if was == hooks_dir || (!was.is_empty() && same(&repo.join(&was), &root.join("githooks"))) {
            p.say("ok", "hooksPath", format!("{} -> {hooks_dir}", fwd(repo)));
            continue;
        }
        let own = (!was.is_empty() && repo.join(&was).exists()).then(|| was.clone());
        let mut acts = Vec::new();
        if let Some(o) = &own {
            acts.push(Act::Git {
                repo: repo.clone(),
                key: "knowledge.ownHooksPath",
                value: Some(o.clone()),
            });
        }
        acts.push(Act::Git {
            repo: repo.clone(),
            key: "core.hooksPath",
            value: Some(hooks_dir.clone()),
        });
        let why = match &own {
            Some(o) => format!("; its own {o} runs first"),
            None if !was.is_empty() => format!("; the old {was} is gone"),
            None => String::new(),
        };
        p.act(
            "change",
            "hooksPath",
            format!(
                "{}/.git/config: -> {hooks_dir} (secret scan, rules backstops{why})",
                fwd(repo)
            ),
            was,
            acts,
        );
    }

    let os = str_of("os").unwrap_or_else(os_tag);
    let trust = cpath.parent().map(Path::to_path_buf).unwrap_or_default();
    if args.has("signing") {
        signing(
            &mut p,
            &trust,
            &os,
            &[vault.clone(), root.clone()]
                .into_iter()
                .filter(|r| has_git(r))
                .collect::<Vec<_>>(),
            &vault,
        );
    }

    // config.json: `{ ...existing, vault, devRoot, os, machine, rolloverHour }`, keys in place.
    let mut cfg_json = existing.clone();
    cfg_json.set("vault", J::s(vault.to_string_lossy()));
    cfg_json.set("devRoot", J::s(dev.to_string_lossy()));
    cfg_json.set("os", J::s(os.clone()));
    let machine = args
        .get("machine")
        .map(str::to_string)
        .or_else(|| existing.get("machine").and_then(J::str).map(str::to_string))
        .unwrap_or_default();
    cfg_json.set("machine", J::s(machine.clone()));
    if existing.get("rolloverHour").is_none_or(|v| *v == J::Null) {
        cfg_json.set("rolloverHour", J::n(5));
    }
    if cfg_json.text() == existing.text() {
        p.say("ok", "config", fwd(&cpath));
    } else {
        let detail = format!(
            "{}: vault {}, devRoot {}, os {os}",
            fwd(&cpath),
            fwd(&vault),
            fwd(&dev)
        );
        let status = if cpath.exists() { "change" } else { "create" };
        p.act(
            status,
            "config",
            detail,
            seen_file(&cpath),
            vec![Act::Write {
                path: cpath.clone(),
                text: pretty(&cfg_json),
                backup: None,
            }],
        );
    }
    if !dev.exists() {
        p.act(
            "create",
            "devRoot",
            fwd(&dev),
            "absent".into(),
            vec![Act::Mkdir(dev.clone())],
        );
    }

    // Links into ~/.claude: the hooks folder, the /brain skill, every brain-* command. And the
    // skill again in ~/.agents/skills, the folder Codex, Gemini CLI, Cursor, opencode, Amp and Zed
    // read (S7.6). opencode reads both folders and keeps one skill per name, so it loads once.
    let mut links = vec![
        (root.join("hooks"), claude.join("hooks"), true),
        (
            root.join("skills").join("brain"),
            claude.join("skills").join("brain"),
            true,
        ),
        (root.join("skills").join("brain"), agents_brain(), true),
    ];
    let mut cmds: Vec<String> = fs::read_dir(root.join("commands"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|f| f.ends_with(".md"))
                .collect()
        })
        .unwrap_or_default();
    cmds.sort();
    links.extend(cmds.iter().map(|f| {
        (
            root.join("commands").join(f),
            claude.join("commands").join(f),
            false,
        )
    }));
    for (target, path, dir) in links {
        if same(&path, &target) {
            p.say("ok", "link", fwd(&path));
            continue;
        }
        let seen = seen_link(&path);
        let mut acts = Vec::new();
        let mut note = String::new();
        if seen.starts_with("link ") {
            acts.push(Act::Unlink(path.clone()));
            note = " (repointed)".into();
        } else if seen == "real" {
            let to = aside(&claude, &path);
            note = format!(" (the old one kept as {})", fwd(&to));
            acts.push(Act::Aside {
                path: path.clone(),
                to,
            });
        }
        let detail = format!("{} -> {}{note}", fwd(&path), fwd(&target));
        acts.push(Act::Link { path, target, dir });
        p.act(
            if seen == "absent" { "create" } else { "change" },
            "link",
            detail,
            seen,
            acts,
        );
    }

    // The binary the hooks run: cargo's, or $KNOWLEDGE_BIN.
    let bin = state::knowledge_bin();
    // A build folder (target/release) is rebuilt under the running hooks; cargo's bin is not.
    let installed = std::env::var_os("KNOWLEDGE_BIN").is_some()
        || bin
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|d| d == "bin");
    if installed {
        p.say(
            "ok",
            "knowledge",
            format!("{} ({})", fwd(&bin), env!("CARGO_PKG_VERSION")),
        );
    } else {
        p.say("WARN", "knowledge", format!("the hooks would run {}, a build folder: `cargo install --path {} --locked` puts it in ~/.cargo/bin, then plan again", fwd(&bin), fwd(&root)));
    }

    // The hooks live in the Claude plugin (S7.3): its marketplace is this checkout's origin, as
    // git with no autoUpdate, so an update is the owner's `claude plugin update` (K14), after a
    // `marketplace update`: the update compares against the marketplace's local clone, which
    // nothing refreshes on its own, so it reports a stale version as the latest. It installs
    // first, so a failed install leaves the old settings.json hooks running.
    let want = serde_json::from_str::<J>(&state::read(
        &root.join("claude/.claude-plugin/plugin.json"),
    ))
    .ok()
    .and_then(|m| m.get("version").and_then(J::str).map(String::from))
    .unwrap_or_default();
    let (known, version) = plugin_state(&claude);
    match version {
        Some(v) if v == want => p.say("ok", "plugin", format!("{PLUGIN} {v}")),
        Some(v) => p.say("WARN", "plugin", format!("{PLUGIN} {v} is installed and the checkout has {want}: run `claude plugin marketplace update {MARKET}`, then `claude plugin update {PLUGIN}`, then /reload-plugins")),
        None => {
            let url = git_get(&root, "remote.origin.url");
            if !known && url.is_empty() {
                p.say("FAIL", "plugin", format!("{} has no origin remote to add as the plugin's marketplace", fwd(&root)));
            } else {
                let from = if known { format!("the known {MARKET} marketplace") } else { url.clone() };
                p.act(
                    "create",
                    "plugin",
                    format!("claude plugin install {PLUGIN}, from {from} (git, no autoUpdate)"),
                    if known { "marketplace known".into() } else { "absent".into() },
                    vec![Act::Plugin { add: (!known).then_some(url), install: true, remove: false }],
                );
            }
        }
    }

    // settings.json: our old hook block out, the plugin's knowledge_bin option in. Every other
    // hook and key stays.
    let file = claude.join("settings.json");
    match read_obj(&file) {
        Err(e) => p.say("FAIL", "settings.json", e),
        Ok(settings) => {
            if swapped(&settings, &bin) == settings {
                p.say(
                    "ok",
                    "settings.json",
                    format!("{}: no hook of ours, knowledge_bin set", fwd(&file)),
                );
            } else {
                let seen = seen_file(&file);
                let backup = backup(&file, &seen);
                let n = sig(settings.get("hooks")).len();
                let kept = backup
                    .as_ref()
                    .map(|b| format!(" (the old file kept as {})", fwd(b)))
                    .unwrap_or_default();
                let detail = format!(
                    "{}: our {n} hooks out (the plugin carries them), knowledge_bin = {}{kept}",
                    fwd(&file),
                    fwd(&bin)
                );
                p.act(
                    if file.exists() { "change" } else { "create" },
                    "settings.json",
                    detail,
                    seen,
                    vec![Act::Swap {
                        file: file.clone(),
                        bin: bin.clone(),
                        backup,
                    }],
                );
            }
        }
    }

    // Identity and Hard-Rules imported whole by ~/.claude/CLAUDE.md.
    let md = claude.join("CLAUDE.md");
    let want: Vec<String> = ["Identity.md", "Hard-Rules.md"]
        .iter()
        .map(|f| fwd(&vault.join(state::META).join(f)))
        .collect();
    let text = state::read(&md);
    let have = imports(&text);
    if want.iter().all(|w| have.contains(&w.to_lowercase())) {
        p.say("ok", "CLAUDE.md", "imports Identity and Hard-Rules");
    } else {
        let next = instructions::block(&vault, Form::Imports)
            .map(|sec| instructions::splice(&text, &sec))
            .unwrap_or_default();
        let detail = format!("{}: imports {}", fwd(&md), want.join(", "));
        p.act(
            if md.exists() { "change" } else { "create" },
            "CLAUDE.md",
            detail,
            seen_file(&md),
            vec![Act::Write {
                path: md.clone(),
                text: next,
                backup: None,
            }],
        );
    }

    // Compile turns each finished daily log into notes once, remembered by hash in an untracked
    // state file. A vault cloned onto a new machine has none, so the first session would compile
    // every log it ever had (2026-11-01, Arch): the backlog is marked done.
    if template {
        p.say("ok", "compile state", "a new vault has no finished log");
    } else {
        let rollover = existing.get("rolloverHour").and_then(J::i64).unwrap_or(5);
        let cfg = Config {
            vault: vault.clone(),
            home: trust.clone(),
            dev: dev.clone(),
            db: trust.join("vault-index.db"),
            os: os.clone(),
            machine,
            rollover,
            pull_trust: false,
            trust_dir: trust.clone(),
        };
        let file = compile::state_file(&cfg);
        let mut st = state::read_json(&file)
            .filter(|v| matches!(v, J::Obj(_)))
            .unwrap_or(J::Obj(Vec::new()));
        let today = state::note_path(&cfg, state::now_ms());
        let mut added = 0;
        for log in compile::logs(&cfg) {
            let key = state::native(&log);
            if log != today && st.get(&key).and_then(J::str).is_none_or(str::is_empty) {
                st.set(&key, J::s(compile::hash_of(&log)));
                added += 1;
            }
        }
        if added == 0 {
            p.say("ok", "compile state", "nothing to seed");
        } else {
            let detail = format!("{}: {added} finished log(s) marked compiled", fwd(&file));
            p.act(
                if file.exists() { "change" } else { "create" },
                "compile state",
                detail,
                seen_file(&file),
                vec![Act::Write {
                    path: file.clone(),
                    text: state::pretty1(&st),
                    backup: None,
                }],
            );
        }
    }

    graphify_retire(&mut p, &vault);

    p.next
        .push("knowledge doctor --repair --pretty: the first index, hubs and commit notes".into());
    p.next.push("knowledge rules list --json, then knowledge rules enable <pack> for each pack you want: the rules interview".into());
    if template {
        p.next.push(format!(
            "fill the FILL: placeholders in {}, then start Claude Code in {}",
            fwd(&vault.join(state::META)),
            fwd(&vault)
        ));
    }
    p
}

fn count(d: &Path, n: &mut usize) {
    for e in fs::read_dir(d).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            count(&e.path(), n);
        } else {
            *n += 1;
        }
    }
}

/// The graphify server setup registered before Stage 8 (S8.5: the code map replaced it): ours when
/// it serves the vault's merged graph, and then planned for removal; a graphify server the owner
/// added stays.
fn graphify_retire(p: &mut Plan, v: &Path) {
    let graph = fwd(&v.join("graphify-out").join("merged-graph.json"));
    let Some(c) = mcp_entry(v) else { return };
    let ours = arr(c.get("args"))
        .first()
        .and_then(J::str)
        .is_some_and(|a| a.replace('\\', "/").eq_ignore_ascii_case(&graph));
    if !ours {
        p.say(
            "ok",
            "graphify MCP",
            "a graphify server not set by setup: left as it is",
        );
    } else if which("claude").is_none() {
        p.say(
            "WARN",
            "graphify MCP",
            "claude is not on PATH, so the server cannot be removed",
        );
    } else {
        let detail = format!(
            "{}: graphify, local scope for {}",
            fwd(&state::home().join(".claude.json")),
            fwd(v)
        );
        p.act(
            "remove",
            "graphify MCP",
            detail,
            c.text(),
            vec![Act::McpRemove(v.to_path_buf())],
        );
    }
}

/// ~/.claude.json's graphify entry for this vault, under either spelling of its path.
fn mcp_entry(vault: &Path) -> Option<J> {
    let cj = state::read_json(&state::home().join(".claude.json"))?;
    let ps = cj.get("projects")?;
    ps.get(&vault.to_string_lossy())
        .or_else(|| ps.get(&fwd(vault)))?
        .get("mcpServers")?
        .get("graphify")
        .cloned()
}

/// --signing: this boot's ed25519 key beside config.json (no passphrase: hooks commit with nobody
/// present), the clones signing with it, and its line in allowed_signers. The other boot's key is
/// added by hand, after its fingerprint is compared (docs/security.md § Signing and pull trust).
fn signing(p: &mut Plan, trust: &Path, os: &str, repos: &[PathBuf], vault: &Path) {
    if which("ssh-keygen").is_none() {
        p.say(
            "FAIL",
            "signing",
            "--signing needs ssh-keygen (OpenSSH) on PATH",
        );
        return;
    }
    let key = trust.join("signing_ed25519");
    let public = trust.join("signing_ed25519.pub");
    if key.exists() {
        p.say("ok", "signing key", fwd(&key));
    } else {
        p.act(
            "create",
            "signing key",
            format!("{} (ed25519, signing only)", fwd(&key)),
            "absent".into(),
            vec![Act::Keygen {
                key: key.clone(),
                comment: format!("knowledge-signing {os}"),
            }],
        );
    }
    for repo in repos {
        let want = [
            ("gpg.format", "ssh".to_string()),
            ("user.signingkey", fwd(&public)),
            ("commit.gpgsign", "true".into()),
            ("tag.gpgsign", "true".into()),
        ];
        let off: Vec<_> = want
            .into_iter()
            .map(|(k, v)| (k, v, git_get(repo, k)))
            .filter(|(_, v, was)| v != was)
            .collect();
        if off.is_empty() {
            p.say("ok", "signing", fwd(repo));
            continue;
        }
        let detail = format!(
            "{}/.git/config: {}",
            fwd(repo),
            off.iter()
                .map(|(k, _, _)| *k)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let seen = off
            .iter()
            .map(|(_, _, was)| was.as_str())
            .collect::<Vec<_>>()
            .join("\t");
        let acts = off
            .into_iter()
            .map(|(k, v, _)| Act::Git {
                repo: repo.clone(),
                key: k,
                value: Some(v),
            })
            .collect();
        p.act("change", "signing", detail, seen, acts);
    }
    let signers = trust.join("allowed_signers");
    let have = state::read(&signers);
    let current: Option<String> = fs::read_to_string(&public)
        .ok()
        .map(|t| t.split_whitespace().take(2).collect::<Vec<_>>().join(" "));
    if current.is_some_and(|k| have.contains(&k)) {
        p.say("ok", "allowed_signers", fwd(&signers));
        return;
    }
    let cwd = if vault.join(".git").exists() {
        vault.to_path_buf()
    } else {
        state::home()
    };
    let email = git_get(&cwd, "user.email");
    if email.is_empty() {
        p.say(
            "FAIL",
            "allowed_signers",
            "--signing needs git's user.email set",
        );
        return;
    }
    let detail = format!("{}: this boot's key, as {email}", fwd(&signers));
    p.act(
        "change",
        "allowed_signers",
        detail,
        seen_file(&signers),
        vec![Act::Signer {
            file: signers.clone(),
            key: public,
            email,
        }],
    );
}

// ---- the remove plan -----------------------------------------------------------------------------

fn plan_remove(args: &Args) -> Plan {
    let mut p = Plan::default();
    let claude = state::home().join(".claude");
    let existing = read_obj(&config::config_path()).unwrap_or(J::Obj(Vec::new()));
    let str_of = |k: &str| {
        existing
            .get(k)
            .and_then(J::str)
            .filter(|s| !s.is_empty())
            .map(expand)
    };
    let vault = str_of("vault");
    let dev = str_of("devRoot").unwrap_or_else(|| expand("~/dev"));
    let root = skeleton(args, &dev);

    let file = claude.join("settings.json");
    match read_obj(&file) {
        Err(e) => p.say("FAIL", "settings.json", e),
        Ok(settings) => {
            let n = sig(settings.get("hooks")).len();
            let next = unswapped(&settings);
            if next == settings {
                p.say("ok", "settings.json", "no hook or plugin option of ours");
            } else {
                let seen = seen_file(&file);
                let backup = backup(&file, &seen);
                let kept = backup
                    .as_ref()
                    .map(|b| format!(" (the old file kept as {})", fwd(b)))
                    .unwrap_or_default();
                let detail = format!(
                    "{}: our {n} hooks and the plugin's option removed, every other hook kept{kept}",
                    fwd(&file)
                );
                let text = pretty(&next);
                p.act(
                    "remove",
                    "settings.json",
                    detail,
                    seen,
                    vec![Act::Write {
                        path: file.clone(),
                        text,
                        backup,
                    }],
                );
            }
        }
    }

    // After settings.json's own write, since `claude plugin uninstall` edits that file too.
    let (known, version) = plugin_state(&claude);
    if known || version.is_some() {
        let what = match &version {
            Some(v) => {
                format!("claude plugin uninstall {PLUGIN} ({v}), then marketplace remove {MARKET}")
            }
            None => format!("claude plugin marketplace remove {MARKET}"),
        };
        p.act(
            "remove",
            "plugin",
            what,
            version.unwrap_or_else(|| "marketplace known".into()),
            vec![Act::Plugin {
                add: None,
                install: false,
                remove: true,
            }],
        );
    } else {
        p.say("ok", "plugin", "not installed");
    }

    let md = claude.join("CLAUDE.md");
    let text = state::read(&md);
    match instructions::section(&text) {
        Some((a, e)) => {
            let head = text[..a].trim_end();
            let tail = text[e..].trim_start_matches(['\r', '\n']);
            let next = match (head.is_empty(), tail.is_empty()) {
                (true, _) => tail.to_string(),
                (false, true) => format!("{head}\n"),
                (false, false) => format!("{head}\n\n{tail}"),
            };
            p.act(
                "remove",
                "CLAUDE.md",
                format!("{}: the imports section setup wrote", fwd(&md)),
                seen_file(&md),
                vec![Act::Write {
                    path: md.clone(),
                    text: next,
                    backup: None,
                }],
            );
        }
        None if imports(&text)
            .iter()
            .any(|i| i.ends_with("/00-meta/identity.md")) =>
        {
            p.say(
                "info",
                "CLAUDE.md",
                "imports written by hand: left as they are",
            )
        }
        None => p.say("ok", "CLAUDE.md", "no imports section"),
    }

    match &root {
        None => p.say("info", "link", "no knowledge-skeleton checkout found, so no link is known to be ours: pass --skeleton <checkout>"),
        Some(root) => {
            let inside = |path: &Path| real(path).is_some_and(|r| real(root).is_some_and(|root| r.starts_with(root)));
            let mut paths = vec![claude.join("hooks"), claude.join("skills").join("brain"), agents_brain()];
            let mut cmds: Vec<PathBuf> = fs::read_dir(claude.join("commands")).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
            cmds.sort();
            paths.extend(cmds);
            for path in paths {
                let seen = seen_link(&path);
                if seen.starts_with("link ") && inside(&path) {
                    p.act("remove", "link", fwd(&path), seen, vec![Act::Unlink(path.clone())]);
                }
            }
            let hooks_dir = fwd(&root.join("githooks"));
            let mut repos: Vec<PathBuf> = vault.iter().cloned().chain([root.clone()]).collect();
            if let Some(v) = &vault {
                repos.extend(checkouts(v, &dev));
            }
            let mut done: Vec<PathBuf> = Vec::new();
            for repo in repos.into_iter().filter(|r| r.join(".git").exists()) {
                if done.iter().any(|d| same(d, &repo)) {
                    continue;
                }
                done.push(repo.clone());
                if git_get(&repo, "core.hooksPath") != hooks_dir {
                    continue;
                }
                let own = git_get(&repo, "knowledge.ownHooksPath");
                let (acts, back) = if own.is_empty() {
                    (vec![Act::Git { repo: repo.clone(), key: "core.hooksPath", value: None }], "unset".to_string())
                } else {
                    (
                        vec![
                            Act::Git { repo: repo.clone(), key: "core.hooksPath", value: Some(own.clone()) },
                            Act::Git { repo: repo.clone(), key: "knowledge.ownHooksPath", value: None },
                        ],
                        format!("back to its own {own}"),
                    )
                };
                p.act("remove", "hooksPath", format!("{}/.git/config: core.hooksPath {back}", fwd(&repo)), format!("{hooks_dir}\t{own}"), acts);
            }
        }
    }

    if let Some(v) = &vault {
        graphify_retire(&mut p, v);
    }
    p.say(
        "info",
        "kept",
        "the vault, config.json, the compile state and any signing key: --remove deletes no data",
    );
    p
}

// ---- writing -------------------------------------------------------------------------------------

fn io(p: &Path) -> impl FnOnce(std::io::Error) -> String + '_ {
    move |e| format!("{}: {e}", fwd(p))
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(io(to))?;
    for e in fs::read_dir(from).map_err(io(from))?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            copy_dir(&src, &dst)?;
        } else {
            fs::copy(&src, &dst).map_err(io(&dst))?;
        }
    }
    Ok(())
}

fn checked(o: state::Out, what: &str) -> Result<(), String> {
    if o.ok() {
        Ok(())
    } else {
        Err(format!("{what}: {}", state::why(&o, true)))
    }
}

/// A link, as install.ts made it: a junction for a folder on Windows (no privilege needed), a
/// symlink otherwise. A file symlink on Windows needs Developer Mode or an elevated shell.
fn link(target: &Path, path: &Path, dir: bool) -> Result<(), String> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(io(d))?;
    }
    #[cfg(windows)]
    {
        if dir {
            let o = state::run(
                Path::new("cmd"),
                &[
                    "/d",
                    "/c",
                    "mklink",
                    "/J",
                    &state::native(path),
                    &state::native(target),
                ],
                None,
                None,
                &[],
                GIT,
            );
            return checked(o, "mklink /J");
        }
        std::os::windows::fs::symlink_file(target, path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(1314) {
                format!(
                    "{}: turn on Developer Mode (Settings > For developers) or run elevated",
                    fwd(path)
                )
            } else {
                format!("{}: {e}", fwd(path))
            }
        })
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        std::os::unix::fs::symlink(target, path).map_err(io(path))
    }
}

fn unlink(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink_dir()) {
            return fs::remove_dir(path).map_err(io(path));
        }
    }
    fs::remove_file(path).map_err(io(path))
}

fn exec(a: &Act) -> Result<(), String> {
    match a {
        Act::Template {
            from,
            vault,
            version,
        } => {
            copy_dir(from, vault)?;
            let pin = vault.join(".skeleton.json");
            fs::write(
                &pin,
                format!("{}\n", serde_json::json!({ "version": version })),
            )
            .map_err(io(&pin))?;
            checked(
                state::git(vault, &["init", "-q", "-b", "main"], GIT),
                "git init",
            )?;
            // An identity is the user's to set; without one the vault stays uncommitted and says so.
            if !git_get(vault, "user.email").is_empty() {
                checked(state::git(vault, &["add", "-A"], GIT), "git add")?;
                let subject = format!("chore: new vault from knowledge-skeleton {version}");
                let body = format!(
                    "Copied from the skeleton's template/ by knowledge setup. Fill the FILL: placeholders in {} next.",
                    state::META
                );
                checked(
                    state::git(vault, &["commit", "-q", "-m", &subject, "-m", &body], GIT),
                    "git commit",
                )?;
            }
            Ok(())
        }
        Act::Write { path, text, backup } => {
            if let Some(b) = backup {
                fs::copy(path, b).map_err(io(b))?;
            }
            if let Some(d) = path.parent() {
                fs::create_dir_all(d).map_err(io(d))?;
            }
            fs::write(path, text).map_err(io(path))
        }
        Act::Swap { file, bin, backup } => exec(&Act::Write {
            path: file.clone(),
            text: pretty(&swapped(&read_obj(file)?, bin)),
            backup: backup.clone(),
        }),
        Act::Plugin {
            add,
            install,
            remove,
        } => {
            let claude = which("claude").ok_or("claude is not on PATH")?;
            let t = Duration::from_secs(120);
            let run = |argv: &[&str], what: &str| {
                checked(state::run(&claude, argv, None, None, &[], t), what)
            };
            if let Some(url) = add {
                run(
                    &["plugin", "marketplace", "add", url],
                    "claude plugin marketplace add",
                )?;
            }
            if *install {
                run(
                    &["plugin", "install", PLUGIN, "--scope", "user"],
                    "claude plugin install",
                )?;
            }
            if *remove {
                if plugin_state(&state::home().join(".claude")).1.is_some() {
                    run(&["plugin", "uninstall", PLUGIN], "claude plugin uninstall")?;
                }
                run(
                    &["plugin", "marketplace", "remove", MARKET],
                    "claude plugin marketplace remove",
                )?;
            }
            Ok(())
        }
        Act::Codex(argv) => {
            let codex = which("codex").ok_or("codex is not on PATH")?;
            let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
            let what = format!("codex {}", argv[..argv.len().min(3)].join(" "));
            checked(
                state::run(&codex, &argv, None, None, &[], Duration::from_secs(120)),
                &what,
            )
        }
        Act::Untrust(file) => {
            let text = fs::read_to_string(file).map_err(io(file))?;
            let next = untrusted(&text);
            if next == text {
                return Ok(());
            }
            let backup = file.with_extension("toml.pre-skeleton");
            fs::copy(file, &backup).map_err(io(&backup))?;
            fs::write(file, next).map_err(io(file))
        }
        Act::Mkdir(d) => fs::create_dir_all(d).map_err(io(d)),
        Act::Unlink(p) => unlink(p),
        Act::Aside { path, to } => {
            if let Some(d) = to.parent() {
                fs::create_dir_all(d).map_err(io(d))?;
            }
            fs::rename(path, to).map_err(io(path))
        }
        Act::Link { path, target, dir } => link(target, path, *dir),
        Act::Git { repo, key, value } => match value {
            Some(v) => checked(state::git(repo, &["config", key, v], GIT), "git config"),
            None => checked(
                state::git(repo, &["config", "--unset", key], GIT),
                "git config --unset",
            ),
        },
        Act::Keygen { key, comment } => {
            if let Some(d) = key.parent() {
                fs::create_dir_all(d).map_err(io(d))?;
            }
            let o = state::run(
                Path::new("ssh-keygen"),
                &[
                    "-q",
                    "-t",
                    "ed25519",
                    "-N",
                    "",
                    "-C",
                    comment,
                    "-f",
                    &key.to_string_lossy(),
                ],
                None,
                None,
                &[],
                GIT,
            );
            checked(o, "ssh-keygen")
        }
        Act::Signer { file, key, email } => {
            let public = fs::read_to_string(key).map_err(io(key))?;
            let public = public
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ");
            let have = state::read(file);
            let sep = if have.is_empty() || have.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            fs::write(
                file,
                format!("{have}{sep}{email} namespaces=\"git\" {public}\n"),
            )
            .map_err(io(file))
        }
        Act::McpRemove(vault) => {
            let claude = which("claude").ok_or("claude is not on PATH")?;
            checked(
                state::run(
                    &claude,
                    &["mcp", "remove", "--scope", "local", "graphify"],
                    Some(vault),
                    None,
                    &[],
                    Duration::from_secs(60),
                ),
                "claude mcp remove",
            )
        }
    }
}

// ---- Codex (S7.4) --------------------------------------------------------------------------------

/// A TOML table header line: `[a.b."c"]`, or `[[array]]`.
fn header(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

/// The body of the table whose header line is `head`, up to the next header.
fn table(text: &str, head: &str) -> Option<String> {
    let mut lines = text.lines().skip_while(|l| l.trim() != head);
    lines.next()?;
    Some(
        lines
            .take_while(|l| !header(l))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// One of our hooks' trust tables: `[hooks.state."knowledge@knowledge-skeleton:<event>:i:j"]`.
fn ours_trusted(line: &str) -> bool {
    let l = line.trim_start();
    l.starts_with("[hooks.state.") && l.contains(&format!("{PLUGIN}:"))
}

/// config.toml without our trust tables, each header with its body; every other byte kept.
fn untrusted(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut skip = false;
    for line in text.split_inclusive('\n') {
        if header(line) {
            skip = ours_trusted(line);
        }
        if !skip {
            out.push_str(line);
        }
    }
    out
}

/// What Codex holds of ours. config.toml is read as text, one header per line, since setup only
/// needs to know which tables exist; the `codex` CLI does every write but [`Act::Untrust`].
struct CodexState {
    known: bool,
    /// The plugin's cached versions, when it is enabled.
    versions: Vec<String>,
    /// The `[mcp_servers.graphify]` body.
    mcp: Option<String>,
    /// The `[mcp_servers.knowledge]` body.
    retrieval: Option<String>,
    trusted: usize,
}

fn codex_state(codex: &Path) -> CodexState {
    let text = state::read(&codex.join("config.toml"));
    let has = |head: String| text.lines().any(|l| l.trim() == head);
    let (name, _) = PLUGIN.split_once('@').unwrap_or((PLUGIN, ""));
    let versions = if has(format!("[plugins.\"{PLUGIN}\"]")) {
        let cache = codex.join("plugins").join("cache").join(MARKET).join(name);
        fs::read_dir(cache)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    CodexState {
        known: has(format!("[marketplaces.{MARKET}]")),
        versions,
        mcp: table(&text, "[mcp_servers.graphify]"),
        retrieval: table(&text, "[mcp_servers.knowledge]"),
        trusted: text.lines().filter(|l| ours_trusted(l)).count(),
    }
}

/// Our hooks as Codex itself judges them: (trusted, all of ours), from `codex app-server`'s
/// `hooks/list`. Codex trusts a hook by a hash of its definition and of the plugin's content, by a
/// rule it does not document, so only Codex can say whether a trust entry still matches: counting
/// our `[hooks.state]` tables read "9 of 9" after an update had made every one `modified` (S7.7).
/// None when Codex is not on PATH or did not answer within 15 s.
fn codex_hooks() -> Option<(usize, usize)> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    let mut c = Command::new(which("codex")?);
    c.arg("app-server")
        .current_dir(state::home())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    state::hide(&mut c);
    let mut child = c.spawn().ok()?;
    let cwd = state::home().to_string_lossy().into_owned();
    let asks = [
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"clientInfo": {"name": "knowledge", "version": env!("CARGO_PKG_VERSION")}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "hooks/list", "params": {"cwds": [cwd]}}),
    ];
    // stdin stays open until the answer is in: the server exits on EOF before answering.
    let mut stdin = child.stdin.take()?;
    for a in &asks {
        if writeln!(stdin, "{a}").is_err() {
            let _ = child.kill();
            return None;
        }
    }
    let out = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
            if v["id"] == 2 {
                let _ = tx.send(v);
                return;
            }
        }
    });
    let reply = rx.recv_timeout(Duration::from_secs(15));
    // Closing stdin ends the server (`codex` may be a launcher whose kill would orphan it); the
    // kill is only for one that did not answer.
    drop(stdin);
    if reply.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let reply = reply.ok()?;
    let ours: Vec<&serde_json::Value> = reply["result"]["data"]
        .as_array()?
        .iter()
        .filter_map(|d| d["hooks"].as_array())
        .flatten()
        .filter(|h| h["pluginId"] == PLUGIN)
        .collect();
    let trusted = ours
        .iter()
        .filter(|h| h["trustStatus"] == "trusted")
        .count();
    Some((trusted, ours.len()))
}

/// The doctor's `codex-trust`: None when the plugin is not installed, else what Codex answered
/// ([`codex_hooks`]), itself None when it did not.
pub fn codex_trust() -> Option<Option<(usize, usize)>> {
    let st = codex_state(&state::home().join(".codex"));
    (!st.versions.is_empty()).then(codex_hooks)
}

/// Whether a TOML body names `path`, in any of the spellings a Windows path takes there.
fn names_path(body: &str, path: &Path) -> bool {
    body.replace("\\\\", "/")
        .replace('\\', "/")
        .to_lowercase()
        .contains(&fwd(path).to_lowercase())
}

/// `setup --harness codex`: the plugin from this checkout's local marketplace, the AGENTS.md block,
/// and the retrieval MCP server; the graphify server setup once wrote is removed. Trust is never written: the owner reviews the hooks once in the Codex CLI
/// (C1), and this plan only says how many are reviewed.
fn plan_codex(args: &Args, at: Option<&Place>) -> Plan {
    let mut p = Plan::default();
    let codex = state::home().join(".codex");
    let config = codex.join("config.toml");
    if which("codex").is_none() {
        p.say(
            "FAIL",
            "prerequisites",
            "codex is not on PATH. Install the Codex CLI, then plan again",
        );
        return p;
    }
    // The plugin's hooks run `knowledge` from PATH: no option carries an absolute path in Codex.
    let Some(bin) = on_path(&mut p, "Codex") else {
        return p;
    };
    let Some((vault, dev)) = at.cloned().or_else(|| configured(&mut p)) else {
        return p;
    };
    let Some(root) = skeleton(args, &dev) else {
        p.say(
            "FAIL",
            "skeleton",
            "no knowledge-skeleton checkout found: pass --skeleton <checkout>",
        );
        return p;
    };
    let manifest = root.join("codex/.codex-plugin/plugin.json");
    let want = serde_json::from_str::<J>(&state::read(&manifest))
        .ok()
        .and_then(|m| m.get("version").and_then(J::str).map(String::from));
    let Some(want) = want.filter(|_| root.join(".agents/plugins/marketplace.json").is_file())
    else {
        p.say("FAIL", "plugin", format!("{} has no Codex plugin (codex/.codex-plugin/plugin.json and .agents/plugins/marketplace.json)", fwd(&root)));
        return p;
    };

    // The plugin. A local marketplace has no upgrade: `plugin add` again copies the new version,
    // and Codex then asks for the changed hooks to be reviewed again.
    let st = codex_state(&codex);
    let review = "Open the Codex CLI and review the plugin's hooks (it asks, or /hooks): unreviewed hooks never run, and the ChatGPT app never asks";
    if st.versions.contains(&want) {
        p.say("ok", "plugin", format!("{PLUGIN} {want}"));
    } else {
        let mut acts = Vec::new();
        if !st.known {
            let src = root.to_string_lossy().into_owned();
            acts.push(Act::Codex(vec![
                "plugin".into(),
                "marketplace".into(),
                "add".into(),
                src,
            ]));
        }
        acts.push(Act::Codex(vec![
            "plugin".into(),
            "add".into(),
            PLUGIN.into(),
        ]));
        let (status, detail) = match st.versions.first() {
            Some(v) => (
                "update",
                format!("codex plugin add {PLUGIN}: {v} to {want}"),
            ),
            None => (
                "create",
                format!(
                    "codex plugin add {PLUGIN} {want}, from {} (local marketplace)",
                    fwd(&root)
                ),
            ),
        };
        p.act(status, "plugin", detail, seen_file(&config), acts);
        p.next.push(review.into());
    }

    // Trust: as Codex judges it, never written. Only for the version already installed: an update
    // in this plan makes Codex ask again anyway, and the review line above says so.
    if st.versions.contains(&want) {
        match codex_hooks() {
            Some((t, n)) if n > 0 && t == n => p.say(
                "ok",
                "hook trust",
                format!("{t} of {n} hooks trusted in Codex"),
            ),
            Some((t, n)) => {
                p.say("WARN", "hook trust", format!("{t} of {n} hooks trusted in Codex; it reads the rest as new or modified, and they never run. setup never writes trust"));
                p.next.push(review.into());
            }
            None => {
                p.say("WARN", "hook trust", "Codex did not answer hooks/list, so trust is unknown. setup never writes trust");
                p.next.push(review.into());
            }
        }
    }

    // The AGENTS.md block, Identity and Hard-Rules whole (Codex cannot import).
    block_step(&mut p, "codex", &vault);

    // Retrieval as an MCP server: Codex's Windows sandbox denies its shell `~/.config/knowledge`,
    // where the index and config live, and an MCP server runs as the user, outside the sandbox.
    // The same binary the hooks run, by absolute path; its tools are read-only, so never asked.
    match &st.retrieval {
        Some(body) if names_path(body, &bin) => p.say(
            "ok",
            "knowledge MCP",
            format!("~/.codex/config.toml, {} mcp", fwd(&bin)),
        ),
        current => {
            let mut acts = Vec::new();
            if current.is_some() {
                acts.push(Act::Codex(vec![
                    "mcp".into(),
                    "remove".into(),
                    "knowledge".into(),
                ]));
            }
            acts.push(Act::Codex(vec![
                "mcp".into(),
                "add".into(),
                "knowledge".into(),
                "--".into(),
                bin.to_string_lossy().into_owned(),
                "mcp".into(),
            ]));
            p.act(
                if current.is_some() {
                    "update"
                } else {
                    "create"
                },
                "knowledge MCP",
                format!(
                    "codex mcp add knowledge -- {} mcp (read-only find and graph)",
                    fwd(&bin)
                ),
                seen_file(&config),
                acts,
            );
        }
    }

    codex_graphify_retire(&mut p, &st, &config);
    p
}

/// The graphify server setup wrote into config.toml before Stage 8 (S8.5): ours when it serves a
/// merged graph, and then planned for removal; a graphify server of the owner's own stays.
fn codex_graphify_retire(p: &mut Plan, st: &CodexState, config: &Path) {
    match &st.mcp {
        Some(body) if body.contains("merged-graph.json") => p.act(
            "remove",
            "graphify MCP",
            "codex mcp remove graphify",
            seen_file(config),
            vec![Act::Codex(vec![
                "mcp".into(),
                "remove".into(),
                "graphify".into(),
            ])],
        ),
        Some(_) => p.say(
            "ok",
            "graphify MCP",
            "a graphify server not set by setup: left as it is",
        ),
        None => {}
    }
}

/// `setup --harness codex --remove`: the block, our two MCP servers, the plugin and its
/// marketplace, then the trust tables `codex plugin remove` leaves behind.
fn plan_codex_remove() -> Plan {
    let mut p = Plan::default();
    let codex = state::home().join(".codex");
    let config = codex.join("config.toml");
    let st = codex_state(&codex);

    block_remove(&mut p, "codex");

    codex_graphify_retire(&mut p, &st, &config);

    // Ours when it runs a knowledge binary's `mcp` verb.
    match &st.retrieval {
        Some(body) if body.contains("knowledge") && body.contains("\"mcp\"") => p.act(
            "remove",
            "knowledge MCP",
            "codex mcp remove knowledge",
            seen_file(&config),
            vec![Act::Codex(vec![
                "mcp".into(),
                "remove".into(),
                "knowledge".into(),
            ])],
        ),
        Some(_) => p.say(
            "ok",
            "knowledge MCP",
            "a knowledge server not set by setup: left as it is",
        ),
        None => p.say("ok", "knowledge MCP", "not registered"),
    }

    let mut acts = Vec::new();
    if !st.versions.is_empty() {
        acts.push(Act::Codex(vec![
            "plugin".into(),
            "remove".into(),
            PLUGIN.into(),
        ]));
    }
    if st.known {
        acts.push(Act::Codex(vec![
            "plugin".into(),
            "marketplace".into(),
            "remove".into(),
            MARKET.into(),
        ]));
    }
    if acts.is_empty() {
        p.say("ok", "plugin", "not installed");
    } else {
        p.act(
            "remove",
            "plugin",
            format!("codex plugin remove {PLUGIN}, then marketplace remove {MARKET}"),
            seen_file(&config),
            acts,
        );
    }

    if st.trusted == 0 {
        p.say("ok", "hook trust", "no trust entry of ours");
    } else {
        p.act(
            "remove",
            "hook trust",
            format!(
                "{}: our {} [hooks.state] tables (the old file kept as config.toml.pre-skeleton)",
                fwd(&config),
                st.trusted
            ),
            seen_file(&config),
            vec![Act::Untrust(config.clone())],
        );
    }
    p
}

// ---- shared by the harness plans ----------------------------------------------------------------

/// The `knowledge` a harness's hooks find on PATH; None, with a FAIL, when there is none.
fn on_path(p: &mut Plan, harness: &str) -> Option<PathBuf> {
    let ver = env!("CARGO_PKG_VERSION");
    let this = std::env::current_exe().ok().and_then(|e| real(&e));
    match (which("knowledge").and_then(|k| real(&k)), &this) {
        (Some(k), Some(t)) if same(&k, t) => {
            p.say("ok", "knowledge", format!("{} on PATH ({ver})", fwd(&k)));
            Some(k)
        }
        (Some(k), _) => {
            p.say("WARN", "knowledge", format!("the hooks run the knowledge on PATH, {}, not this binary; its version must be {ver}", fwd(&k)));
            Some(k)
        }
        (None, _) => {
            p.say("FAIL", "knowledge", format!("knowledge is not on PATH, and the {harness} hooks find it there: `cargo install --path <checkout> --locked`, then plan again"));
            None
        }
    }
}

/// The vault and the checkouts folder a harness's plan writes for.
type Place = (PathBuf, PathBuf);

/// The vault and checkouts folder config.json names: the Claude setup writes it first.
fn configured(p: &mut Plan) -> Option<Place> {
    let existing = read_obj(&config::config_path()).unwrap_or(J::Obj(Vec::new()));
    let str_of = |k: &str| {
        existing
            .get(k)
            .and_then(J::str)
            .filter(|s| !s.is_empty())
            .map(expand)
    };
    let Some(vault) = str_of("vault") else {
        p.say(
            "FAIL",
            "config",
            "config.json names no vault: run `knowledge setup --plan` (Claude Code) first",
        );
        return None;
    };
    Some((vault, str_of("devRoot").unwrap_or_else(|| expand("~/dev"))))
}

/// The adapter's inline block (Identity and Hard-Rules whole) in its global instructions file.
fn block_step(p: &mut Plan, id: &str, vault: &Path) {
    match crate::harness::adapter(id).and_then(|a| a.instructions()) {
        Some((path, form)) => block_write(p, "AGENTS.md", &path, form, vault),
        None => p.say(
            "FAIL",
            "AGENTS.md",
            format!("the {id} adapter names no instructions file"),
        ),
    }
}

fn block_write(p: &mut Plan, name: &'static str, path: &Path, form: Form, vault: &Path) {
    let Some(want) = instructions::block(vault, form) else {
        p.say(
            "FAIL",
            name,
            format!(
                "cannot read Identity.md and Hard-Rules.md in {}",
                fwd(&vault.join(state::META))
            ),
        );
        return;
    };
    let text = state::read(path);
    match instructions::section(&text) {
        Some((a, e)) if text[a..e] == want => p.say("ok", name, format!("{}: the block is current", fwd(path))),
        found => p.act(
            if found.is_some() { "update" } else { "create" },
            name,
            format!("{}: Identity and Hard-Rules written whole between the markers; text outside them kept", fwd(path)),
            seen_file(path),
            vec![Act::Write { path: path.to_path_buf(), text: instructions::splice(&text, &want), backup: None }],
        ),
    }
}

/// The block out of the adapter's instructions file, the owner's text kept; the file goes when
/// nothing else is left in it.
fn block_remove(p: &mut Plan, id: &str) {
    if let Some((path, _)) = crate::harness::adapter(id).and_then(|a| a.instructions()) {
        block_unwrite(p, "AGENTS.md", &path);
    }
}

fn block_unwrite(p: &mut Plan, name: &'static str, path: &Path) {
    let text = state::read(path);
    match instructions::section(&text) {
        None => p.say("ok", name, "no knowledge block"),
        Some((a, e)) => {
            let rest = format!("{}{}", &text[..a], &text[e..]);
            let rest = rest.trim();
            let act = if rest.is_empty() {
                Act::Unlink(path.to_path_buf())
            } else {
                Act::Write {
                    path: path.to_path_buf(),
                    text: format!("{rest}\n"),
                    backup: None,
                }
            };
            p.act(
                "remove",
                name,
                format!("{}: the knowledge block, text outside it kept", fwd(path)),
                seen_file(path),
                vec![act],
            );
        }
    }
}

// ---- generic (S7.6) ------------------------------------------------------------------------------

/// The file `--instructions` names, refused when it is a folder: the block goes into a file.
fn generic_file(p: &mut Plan, args: &Args) -> Option<PathBuf> {
    let Some(f) = args.get("instructions").map(expand) else {
        p.say(
            "FAIL",
            "instructions",
            "--harness generic needs --instructions <file>: the file the harness reads",
        );
        return None;
    };
    if f.is_dir() {
        p.say(
            "FAIL",
            "instructions",
            format!("{} is a folder, not a file", fwd(&f)),
        );
        return None;
    }
    Some(f)
}

/// Inline: a harness this skeleton has no adapter for follows no `@` imports.
fn plan_generic(args: &Args) -> Plan {
    let mut p = Plan::default();
    if let Some(f) = generic_file(&mut p, args)
        && let Some((vault, _)) = configured(&mut p)
    {
        block_write(&mut p, "instructions", &f, Form::Generic, &vault);
    }
    p
}

fn plan_generic_remove(args: &Args) -> Plan {
    let mut p = Plan::default();
    if let Some(f) = generic_file(&mut p, args) {
        block_unwrite(&mut p, "instructions", &f);
    }
    p
}

// ---- opencode (S7.5) -----------------------------------------------------------------------------

/// The plugin, compiled in: a changed shim is a new binary, and setup writes exactly this text.
const OPENCODE_PLUGIN: &str = include_str!("../opencode/knowledge.ts");

/// `~/.config/opencode/plugins/knowledge.ts`: opencode loads every file there, with no config entry.
fn opencode_plugin() -> PathBuf {
    state::home().join(".config/opencode/plugins/knowledge.ts")
}

/// Ours when it is the skeleton's shim, at any version.
fn opencode_ours(text: &str) -> bool {
    text.starts_with("// knowledge-skeleton's opencode plugin")
}

/// `setup --harness opencode`: the plugin file and the AGENTS.md block. Retrieval is the shell's
/// `knowledge find`: opencode has no sandbox between its shell and `~/.config/knowledge`.
fn plan_opencode(at: Option<&Place>) -> Plan {
    let mut p = Plan::default();
    let Some(opencode) = which("opencode") else {
        p.say(
            "FAIL",
            "prerequisites",
            "opencode is not on PATH. Install opencode, then plan again",
        );
        return p;
    };
    // The shim has opencode 2's plugin shape (`export default { id, setup }`). opencode 1 refuses
    // it at load ("must default export an object with server()"), which left the adapter dead
    // behind an `ok` (Arch, 1.16.2, 2026-11-25). An unreadable version is not held against it.
    let o = state::run(
        &opencode,
        &["--version"],
        None,
        None,
        &[],
        Duration::from_secs(10),
    );
    let v = o.out();
    if v.split('.')
        .next()
        .and_then(|m| m.trim().parse::<u32>().ok())
        .is_some_and(|m| m < 2)
    {
        p.say(
            "FAIL",
            "prerequisites",
            format!("opencode {v} takes 1.x plugins, and the shim is 2.x's: upgrade opencode, then plan again"),
        );
        return p;
    }
    if on_path(&mut p, "opencode").is_none() {
        return p;
    }
    let Some((vault, _)) = at.cloned().or_else(|| configured(&mut p)) else {
        return p;
    };
    let path = opencode_plugin();
    let have = state::read(&path);
    let ver = env!("CARGO_PKG_VERSION");
    if have.replace("\r\n", "\n") == OPENCODE_PLUGIN.replace("\r\n", "\n") {
        p.say("ok", "plugin", format!("{} ({ver})", fwd(&path)));
    } else if !have.is_empty() && !opencode_ours(&have) {
        p.say(
            "FAIL",
            "plugin",
            format!(
                "{} is not the skeleton's: move it, then plan again",
                fwd(&path)
            ),
        );
        return p;
    } else {
        p.act(
            if have.is_empty() { "create" } else { "update" },
            "plugin",
            format!(
                "{}: the hooks shim, pinned to {ver}; opencode loads it at its next start",
                fwd(&path)
            ),
            seen_file(&path),
            vec![Act::Write {
                path: path.clone(),
                text: OPENCODE_PLUGIN.to_string(),
                backup: None,
            }],
        );
        p.next.push(
            "opencode service restart: plugins load in opencode's background service, at its start"
                .into(),
        );
    }
    block_step(&mut p, "opencode", &vault);
    p
}

/// `setup --harness opencode --remove`: the block, then the plugin file if it is ours.
fn plan_opencode_remove() -> Plan {
    let mut p = Plan::default();
    block_remove(&mut p, "opencode");
    let path = opencode_plugin();
    let have = state::read(&path);
    if have.is_empty() {
        p.say("ok", "plugin", "not installed");
    } else if opencode_ours(&have) {
        p.act(
            "remove",
            "plugin",
            fwd(&path),
            seen_file(&path),
            vec![Act::Unlink(path.clone())],
        );
    } else {
        p.say(
            "ok",
            "plugin",
            "a knowledge.ts not written by setup: left as it is",
        );
    }
    p
}

// ---- the Claude desktop app (S7.7) ---------------------------------------------------------------

/// The app's MCP config file, following an MSIX install's marker to the folder it really reads.
/// None, with the reason said, when there is no file to write: the app is not here, or it moved
/// its data to a folder it has not made yet.
fn desktop_config(p: &mut Plan, remove: bool) -> Option<PathBuf> {
    let a = crate::harness::adapter("claude-desktop")?;
    let mcp = a.mcp.as_ref()?;
    let Some(dir) = a.found() else {
        if remove {
            p.say("ok", "knowledge MCP", "no Claude desktop app here");
        } else {
            p.say("FAIL", "prerequisites", "the Claude desktop app is not installed here. Install it, start it once, then plan again");
        }
        return None;
    };
    let package = fs::read_dir(&dir).ok().and_then(|rd| {
        rd.flatten().find_map(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_prefix(".user-data-left.")
                .map(String::from)
        })
    });
    let dir = match (package, &mcp.moved) {
        (Some(pkg), Some(moved)) => {
            let to = crate::harness::at_home(moved).join(pkg);
            if !to.is_dir() {
                let (status, what) = if remove {
                    ("ok", "nothing of ours there")
                } else {
                    ("WARN", "start the app once, then plan again")
                };
                p.say(
                    status,
                    "knowledge MCP",
                    format!(
                        "{} moved the app's data to {}, which does not exist yet: {what}",
                        fwd(&dir),
                        fwd(&to)
                    ),
                );
                return None;
            }
            to
        }
        _ => dir,
    };
    Some(dir.join(&mcp.file))
}

/// Ours: some knowledge binary's `mcp` verb, with nothing else in it.
fn desktop_ours(entry: &J) -> bool {
    entry.get("args") == Some(&J::Arr(vec![J::s("mcp")]))
        && entry
            .get("command")
            .and_then(J::str)
            .and_then(|c| {
                Path::new(c)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .is_some_and(|s| s == "knowledge")
}

/// The config with `mcpServers.knowledge` set (`Some`) or taken out (`None`), every other key kept;
/// an `mcpServers` left empty goes, since setup made it.
fn desktop_with(config: &J, entry: Option<J>) -> J {
    let mut next = config.clone();
    let mut servers = next
        .get("mcpServers")
        .cloned()
        .unwrap_or(J::Obj(Vec::new()));
    match entry {
        Some(e) => servers.set("knowledge", e),
        None => servers.remove("knowledge"),
    }
    if servers == J::Obj(Vec::new()) {
        next.remove("mcpServers");
    } else {
        next.set("mcpServers", servers);
    }
    next
}

/// The step that writes `next` over the config, the old file kept beside it.
fn desktop_step(p: &mut Plan, status: &'static str, file: &Path, next: &J, what: String) {
    let seen = seen_file(file);
    let backup = backup(file, &seen);
    let kept = backup
        .as_ref()
        .map(|b| format!(" (the old file kept as {})", fwd(b)))
        .unwrap_or_default();
    p.act(
        status,
        "knowledge MCP",
        format!("{}: {what}, every other key kept{kept}", fwd(file)),
        seen,
        vec![Act::Write {
            path: file.to_path_buf(),
            text: pretty(next),
            backup,
        }],
    );
}

/// `setup --harness claude-desktop`: the chat's retrieval tools, `knowledge mcp` by absolute path
/// (the app starts it with no shell PATH). The Code tab bridges the same server into its sessions,
/// so the Claude plugin declares none (0.11.2).
fn plan_desktop() -> Plan {
    let mut p = Plan::default();
    let Some(file) = desktop_config(&mut p, false) else {
        return p;
    };
    let config = match read_obj(&file) {
        Ok(c) => c,
        Err(e) => {
            p.say("FAIL", "knowledge MCP", e);
            return p;
        }
    };
    let bin = state::knowledge_bin();
    let have = config.get("mcpServers").and_then(|s| s.get("knowledge"));
    let current = have.is_some_and(|e| {
        desktop_ours(e)
            && e.get("command")
                .and_then(J::str)
                .is_some_and(|c| same(Path::new(c), &bin))
    });
    if current {
        p.say(
            "ok",
            "knowledge MCP",
            format!("{}: {} mcp", fwd(&file), fwd(&bin)),
        );
    } else if have.is_some_and(|e| !desktop_ours(e)) {
        p.say("FAIL", "knowledge MCP", format!("{}: an mcpServers.knowledge not set by setup: rename or remove it, then plan again", fwd(&file)));
    } else {
        let entry = state::obj([
            ("command", J::s(bin.to_string_lossy())),
            ("args", J::Arr(vec![J::s("mcp")])),
        ]);
        let status = if file.exists() { "change" } else { "create" };
        let next = desktop_with(&config, Some(entry));
        desktop_step(
            &mut p,
            status,
            &file,
            &next,
            format!("mcpServers.knowledge = {} mcp", fwd(&bin)),
        );
        p.next.push("Quit the Claude desktop app from its tray icon and start it again: it reads claude_desktop_config.json only at start".into());
    }
    p
}

/// `setup --harness claude-desktop --remove`: our server out; one set by hand stays.
fn plan_desktop_remove() -> Plan {
    let mut p = Plan::default();
    let Some(file) = desktop_config(&mut p, true) else {
        return p;
    };
    let config = match read_obj(&file) {
        Ok(c) => c,
        Err(e) => {
            p.say("FAIL", "knowledge MCP", e);
            return p;
        }
    };
    match config.get("mcpServers").and_then(|s| s.get("knowledge")) {
        Some(e) if desktop_ours(e) => {
            let next = desktop_with(&config, None);
            desktop_step(
                &mut p,
                "remove",
                &file,
                &next,
                "mcpServers.knowledge".into(),
            );
        }
        Some(_) => p.say(
            "ok",
            "knowledge MCP",
            "an mcpServers.knowledge not set by setup: left as it is",
        ),
        None => p.say("ok", "knowledge MCP", "not registered"),
    }
    p
}

// ---- Obsidian's graph view (S7.7a) ---------------------------------------------------------------

/// The graph's colour groups, one per family of note types in Conventions (the owner's pick A,
/// 2026-11-23, from live previews). template/ carries this file, so a new vault opens with them;
/// setup merges the same groups into a vault that already exists.
const GRAPH: &str = include_str!("../template/.obsidian/graph.json");

/// A vault opened in Obsidian (it has `.obsidian/`) gets `colorGroups` in `.obsidian/graph.json`,
/// and nothing else: the file is Obsidian's untracked view state (0040), so zoom, forces and the
/// search filter stay as Obsidian left them.
fn plan_graph(vault: &Path) -> Plan {
    let mut p = Plan::default();
    let dir = vault.join(".obsidian");
    if !dir.is_dir() {
        p.say(
            "ok",
            "graph view",
            "no .obsidian/ in the vault: not opened in Obsidian",
        );
        return p;
    }
    let file = dir.join("graph.json");
    let config = match read_obj(&file) {
        Ok(c) => c,
        Err(e) => {
            p.say("FAIL", "graph view", e);
            return p;
        }
    };
    let Some(groups) = serde_json::from_str::<J>(GRAPH)
        .ok()
        .and_then(|g| g.get("colorGroups").cloned())
    else {
        return p;
    };
    if config.get("colorGroups") == Some(&groups) {
        p.say(
            "ok",
            "graph view",
            format!("{}: colour groups by note type", fwd(&file)),
        );
        return p;
    }
    let mut next = config.clone();
    next.set("colorGroups", groups);
    let seen = seen_file(&file);
    let backup = backup(&file, &seen);
    let kept = backup
        .as_ref()
        .map(|b| format!(" (the old file kept as {})", fwd(b)))
        .unwrap_or_default();
    p.act(
        if file.exists() { "change" } else { "create" },
        "graph view",
        format!(
            "{}: colorGroups by note type, every other key kept{kept}",
            fwd(&file)
        ),
        seen,
        vec![Act::Write {
            path: file,
            text: pretty(&next),
            backup,
        }],
    );
    p.next.push("If Obsidian is open, run 'Reload app without saving' from its command palette: it reads graph.json at start, and writes its own copy back when the graph's settings change".into());
    p
}

// ---- every harness found (S7.7) ------------------------------------------------------------------

/// `setup` with no `--harness`: Claude Code, the base the others build on (config.json, the vault,
/// the links), then every harness an adapter detects, each under its own `found` line. One plan,
/// one id, one apply. The others plan against the vault the Claude part resolves, so a first
/// setup needs no second pass, except when that vault is still to be made from template/. Last,
/// the graph view's colour groups, for a vault opened in Obsidian.
fn plan_all(args: &Args, remove: bool) -> Plan {
    let mut p = Plan::default();
    p.say(
        "found",
        "harness",
        "Claude Code: the vault, config.json, the plugin, the links",
    );
    p.extend(if remove {
        plan_remove(args)
    } else {
        plan_setup(args)
    });
    let at = place(
        args,
        &read_obj(&config::config_path()).unwrap_or(J::Obj(Vec::new())),
    );
    let ready = remove || at.0.join(state::META).join("Identity.md").is_file();
    let mut waited = false;
    for a in crate::harness::all() {
        let Some(dir) = a.found() else {
            continue;
        };
        let name = &a.name;
        if let Some(bin) = a.bin().filter(|b| which(b).is_none()) {
            p.say(
                "WARN",
                "harness",
                format!(
                    "{name}: {} is here, but {bin} is not on PATH: skipped",
                    fwd(&dir)
                ),
            );
            continue;
        }
        if !ready {
            p.say(
                "WARN",
                "harness",
                format!("{name}: {} found, set up once the vault exists", fwd(&dir)),
            );
            waited = true;
            continue;
        }
        p.say("found", "harness", format!("{name}: {}", fwd(&dir)));
        p.extend(match (a.id.as_str(), remove) {
            ("codex", false) => plan_codex(args, Some(&at)),
            ("codex", true) => plan_codex_remove(),
            ("opencode", false) => plan_opencode(Some(&at)),
            ("opencode", true) => plan_opencode_remove(),
            ("claude-desktop", false) => plan_desktop(),
            ("claude-desktop", true) => plan_desktop_remove(),
            _ => Plan::default(),
        });
    }
    if waited {
        p.next.push(
            "knowledge setup --plan again, for the harnesses that waited on the vault".into(),
        );
    }
    // A vault still to be made from template/ gets the graph file with it.
    if remove {
        p.say(
            "ok",
            "graph view",
            "Obsidian's view state: the colour groups stay",
        );
    } else if ready {
        p.extend(plan_graph(&at.0));
    }
    p
}

// ---- the verb ------------------------------------------------------------------------------------

/// The options again, for the apply line: `--apply` must be given the plan's own inputs.
fn options(args: &Args) -> String {
    let mut out = String::new();
    for k in [
        "harness",
        "instructions",
        "vault",
        "dev",
        "machine",
        "skeleton",
    ] {
        if let Some(v) = args.get(k) {
            let v = if v.contains(char::is_whitespace) || v.is_empty() {
                format!("\"{v}\"")
            } else {
                v.to_string()
            };
            out.push_str(&format!(" --{k} {v}"));
        }
    }
    for k in ["from-template", "adopt", "signing"] {
        if args.has(k) {
            out.push_str(&format!(" --{k}"));
        }
    }
    out
}

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(
        raw,
        &[
            "vault",
            "dev",
            "machine",
            "skeleton",
            "apply",
            "harness",
            "instructions",
        ],
    )?;
    let remove = args.has("remove");
    if args.has("plan") == args.get("apply").is_some() {
        return Err(Fault::Usage(USAGE.into()));
    }
    let plan = match (args.get("harness"), remove) {
        (None, _) => plan_all(&args, remove),
        (Some("claude"), false) => plan_setup(&args),
        (Some("claude"), true) => plan_remove(&args),
        (Some("codex"), false) => plan_codex(&args, None),
        (Some("codex"), true) => plan_codex_remove(),
        (Some("opencode"), false) => plan_opencode(None),
        (Some("opencode"), true) => plan_opencode_remove(),
        (Some("claude-desktop"), false) => plan_desktop(),
        (Some("claude-desktop"), true) => plan_desktop_remove(),
        (Some("generic"), false) => plan_generic(&args),
        (Some("generic"), true) => plan_generic_remove(&args),
        (Some(h), _) => {
            return Err(Fault::Usage(format!(
                "setup: no harness `{h}` (claude, codex, opencode, claude-desktop, generic)\n{USAGE}"
            )));
        }
    };
    let id = plan.id();
    let (what, flag) = if remove {
        ("remove", "--remove ")
    } else {
        ("setup", "")
    };
    let ver = env!("CARGO_PKG_VERSION");

    if let Some(want) = args.get("apply") {
        if plan.failed() {
            emit(&format!(
                "knowledge {ver} {what}\n\n{}\nNothing written: a step FAILs. Fix it, then run --plan again.\n",
                plan.lines()
            ));
            return Ok(1);
        }
        if want != id {
            return Err(Fault::Usage(format!(
                "setup: the plan changed since --plan (it is {id} now, not {want}); nothing written. Run `knowledge setup {flag}--plan{}` again and read it",
                options(&args)
            )));
        }
        let mut out = format!("knowledge {ver} {what}: applying plan {id}\n\n");
        for s in plan.steps.iter().filter(|s| !s.acts.is_empty()) {
            for a in &s.acts {
                if let Err(e) = exec(a) {
                    out.push_str(&format!("  FAIL    {:<16} {e}\n\nStopped: the steps above it are done; fix this one and run --plan again.\n", s.name));
                    emit(&out);
                    return Ok(1);
                }
            }
            out.push_str(&format!(
                "  {:<7} {:<16} {}\n",
                if remove { "removed" } else { "set" },
                s.name,
                s.detail
            ));
        }
        if !remove && !plan.next.is_empty() {
            out.push_str("\nNext:\n");
            out.extend(plan.next.iter().map(|n| format!("  {n}\n")));
        }
        out.push_str("\nDone.\n");
        emit(&out);
        return Ok(0);
    }

    let mut out = format!(
        "knowledge {ver} {what} plan: nothing is written\n\n{}",
        plan.lines()
    );
    if plan.failed() {
        out.push_str("\nNot ready: fix what FAILed above, then run --plan again.\n");
        emit(&out);
        return Ok(1);
    }
    if !remove && !plan.next.is_empty() {
        out.push_str("\nAfter apply:\n");
        out.extend(plan.next.iter().map(|n| format!("  {n}\n")));
    }
    match plan.changes() {
        0 => out.push_str("\nNothing to change.\n"),
        n => out.push_str(&format!(
            "\n{n} change(s). Apply exactly these: knowledge setup {flag}--apply {id}{}\n",
            options(&args)
        )),
    }
    out.push_str(&format!("plan {id}\n"));
    emit(&out);
    Ok(0)
}
