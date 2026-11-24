//! `knowledge hook pre-tool --harness <id>`: the guard (spec D15, K38, K40). It reads the
//! config and `rules.lock`, nothing else, and answers `deny` or `ask` with the guard's id and
//! reason, which the harness's translator words; when it allows, it answers nothing. A missing or
//! broken lock fails open with one stderr line: the doctor FAILs on the lock and the commit-time
//! backstops still hold.
//!
//! A shell line is split by hand, no crate: segments on `|`, `;`, `&`, `&&`, `||` and newlines,
//! words on whitespace, quotes removed, `>` and `>>` kept as tokens so their targets count as
//! written. A line it cannot judge (`$(…)`, backticks, brace expansion) is judged on the words it
//! does see, so what hides inside passes: K40's backstop is what holds there.
use crate::config;
use crate::harness::{Payload, Tool};
use crate::rules::{self, Guard, Lock};
use std::path::{Path, PathBuf};

/// Where a call happens, for a guard's `repo`, `branch` and `os`.
pub struct Place {
    pub repo: String,
    pub branch: String,
}

/// The strongest guard that matches the call: its action and `id (rule): reason`, or None to allow.
pub fn run(p: &Payload) -> Option<(String, String)> {
    let c = &p.call;
    if c.tool == Tool::None {
        return None;
    }
    let lock = match config::load().and_then(|c| rules::read_lock(&c.abs_of(rules::LOCK))) {
        Ok(Some(l)) => l,
        Ok(None) => return None,
        Err(f) => {
            eprintln!("knowledge guard: no usable rules.lock ({f}); allowing");
            return None;
        }
    };
    let cwd = &p.cwd;
    let place = place_of(Path::new(cwd));
    let hit = match &c.tool {
        Tool::Shell => judge_shell(&lock, &place, &c.command, cwd, false),
        Tool::PowerShell => judge_shell(&lock, &place, &c.command, cwd, true),
        Tool::Edit => judge(&lock, &place, |g| {
            g.paths
                .iter()
                .any(|pat| c.paths.iter().any(|f| path_match(pat, f, cwd)))
        }),
        Tool::Mcp(t) => judge(&lock, &place, |g| {
            g.tools
                .iter()
                .any(|pat| wild(&pat.to_lowercase(), &t.to_lowercase()))
        }),
        Tool::None | Tool::Other => None,
    };
    hit.map(|g| {
        (
            g.action.clone(),
            format!("{} ({}): {}", g.id, g.rule, g.reason),
        )
    })
}

/// The strongest answer among the guards that apply here and match: `deny` over `ask`.
fn judge<'a>(lock: &'a Lock, place: &Place, hit: impl Fn(&Guard) -> bool) -> Option<&'a Guard> {
    let mut best: Option<&Guard> = None;
    for g in lock
        .guards
        .iter()
        .filter(|g| applies(lock, g, place) && hit(g))
    {
        if best.is_none_or(|b| b.action != "deny" && g.action == "deny") {
            best = Some(g);
        }
    }
    best
}

/// A guard applies when its pack is on and its `os`, `repo` and `branch` (if set) match.
pub fn applies(lock: &Lock, g: &Guard, place: &Place) -> bool {
    let pack_on = lock
        .rules
        .iter()
        .find(|r| r.id == g.rule)
        .and_then(|r| lock.packs.iter().find(|p| p.id == r.pack))
        .is_none_or(|p| p.enabled);
    pack_on
        && (g.os.is_empty() || g.os == std::env::consts::OS)
        && (g.repo.is_empty() || g.repo.eq_ignore_ascii_case(&place.repo))
        && (g.branch.is_empty() || g.branch == place.branch)
}

fn judge_shell<'a>(
    lock: &'a Lock,
    place: &Place,
    line: &str,
    cwd: &str,
    ps: bool,
) -> Option<&'a Guard> {
    let segments = segments(line, ps, 0);
    judge(lock, place, |g| {
        segments.iter().any(|seg| {
            let (cmd, rest) = command(seg, ps);
            let words_hit = !g.words.is_empty()
                && g.words.iter().any(|w| seq_match(w, &cmd, &rest))
                && !g.unless.iter().any(|u| rest.iter().any(|w| w == u));
            let path_hit = !g.paths.is_empty()
                && written(&cmd, &rest, seg, ps)
                    .iter()
                    .any(|f| g.paths.iter().any(|pat| path_match(pat, f, cwd)));
            words_hit || path_hit
        })
    })
}

/// The segments a line runs: its own, plus those of a line handed to a shell inside it
/// (`bash -c '…'`, `pwsh -Command '…'`, `cmd /c …`), three levels deep, so `bash -c 'git push'`
/// is judged as `git push`. Extra words after the line are joined in: that can only add a match.
fn segments(line: &str, ps: bool, depth: u8) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for seg in split(line, ps) {
        let (cmd, rest) = command(&seg, ps);
        let after = |flag: &dyn Fn(&str) -> bool| {
            rest.iter()
                .position(|w| flag(&w.to_lowercase()))
                .map(|i| rest[i + 1..].join(" "))
        };
        let inner = match cmd.as_str() {
            // `-c`, or a cluster holding it (`-lc`, `-ec`).
            "bash" | "sh" | "zsh" | "dash" | "ksh" | "fish" => {
                after(&|w| w.starts_with('-') && !w.starts_with("--") && w.contains('c'))
                    .map(|s| (s, false))
            }
            "pwsh" | "powershell" => {
                after(&|w| matches!(w, "-c" | "-command" | "/c" | "/command")).map(|s| (s, true))
            }
            "cmd" => after(&|w| matches!(w, "/c" | "/k")).map(|s| (s, false)),
            _ => None,
        };
        if depth < 3
            && let Some((s, ps)) = inner
        {
            out.extend(segments(&s, ps, depth + 1));
        }
        out.push(seg);
    }
    out
}

/// Segments of a shell line, each a list of tokens. Quotes are removed; `>`/`>>` are tokens.
/// Bash escapes with `\`, PowerShell with a backtick; PowerShell keeps `\` literal (paths).
pub fn split(line: &str, ps: bool) -> Vec<Vec<String>> {
    let esc = if ps { '`' } else { '\\' };
    let mut segs = vec![Vec::new()];
    let mut word = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    let end_word = |segs: &mut Vec<Vec<String>>, word: &mut String, quoted: &mut bool| {
        if !word.is_empty() || *quoted {
            segs.last_mut().unwrap().push(std::mem::take(word));
        }
        *quoted = false;
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                quoted = true;
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    word.push(q);
                }
            }
            '"' => {
                quoted = true;
                while let Some(q) = chars.next() {
                    if q == '"' {
                        break;
                    }
                    if q == esc {
                        if let Some(n) = chars.next() {
                            word.push(n);
                        }
                        continue;
                    }
                    word.push(q);
                }
            }
            c if c == esc => {
                if let Some(n) = chars.next()
                    && n != '\n'
                {
                    word.push(n);
                }
            }
            ' ' | '\t' | '\r' => end_word(&mut segs, &mut word, &mut quoted),
            '>' => {
                end_word(&mut segs, &mut word, &mut quoted);
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                if chars.peek() == Some(&'&') {
                    chars.next(); // 2>&1: a descriptor, not a file
                    while chars.peek().is_some_and(char::is_ascii_digit) {
                        chars.next();
                    }
                    continue;
                }
                segs.last_mut().unwrap().push(">".into());
            }
            '|' | ';' | '&' | '\n' | '(' | ')' | '{' | '}' => {
                end_word(&mut segs, &mut word, &mut quoted);
                while chars.peek().is_some_and(|n| matches!(n, '|' | '&')) {
                    chars.next();
                }
                if !segs.last().unwrap().is_empty() {
                    segs.push(Vec::new());
                }
            }
            _ => word.push(c),
        }
    }
    end_word(&mut segs, &mut word, &mut quoted);
    segs.retain(|s| !s.is_empty());
    segs
}

/// Commands that run the command after them: the name, its options that take a separate value,
/// and how many plain words come before the command (`timeout 120 git push`: one, the duration).
/// `timeout` matters most: the agent's own rules wrap every long command in it, and before it was
/// listed, `timeout 120 git push` passed the push guard as a `timeout` command (2026-11-24).
const WRAPPERS: [(&str, &[&str], usize); 12] = [
    ("sudo", &["-u", "-g", "-h", "-p", "-C", "-D"], 0),
    ("doas", &["-u", "-C"], 0),
    ("env", &["-u", "-C", "--unset", "--chdir"], 0),
    ("command", &[], 0),
    ("exec", &["-a"], 0),
    ("time", &["-f", "-o", "--format", "--output"], 0),
    ("nohup", &[], 0),
    ("timeout", &["-k", "-s", "--kill-after", "--signal"], 1),
    ("nice", &["-n", "--adjustment"], 0),
    ("ionice", &["-c", "-n", "-p", "--class", "--classdata"], 0),
    (
        "stdbuf",
        &["-i", "-o", "-e", "--input", "--output", "--error"],
        0,
    ),
    (
        "xargs",
        &[
            "-a",
            "-d",
            "-E",
            "-I",
            "-L",
            "-n",
            "-P",
            "-s",
            "--arg-file",
            "--delimiter",
            "--max-args",
            "--max-procs",
        ],
        0,
    ),
];

/// The command word (its file name, lower case, without `.exe`, `.cmd`, `.bat`, `.ps1`) and the
/// words after it. Leading `VAR=value` words, PowerShell's `&` and the [`WRAPPERS`] with their
/// options and operands are skipped.
fn command(seg: &[String], ps: bool) -> (String, Vec<String>) {
    let mut i = 0;
    while i < seg.len() {
        let w = &seg[i];
        let assignment = !ps
            && w.contains('=')
            && !w.starts_with('=')
            && w.split('=')
                .next()
                .is_some_and(|k| k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if assignment {
            i += 1;
            continue;
        }
        let Some((_, valued, operands)) = WRAPPERS.iter().find(|(n, ..)| w == n) else {
            break;
        };
        i += 1;
        // Its options: `-k 5` takes the next word, `-k5`, `--signal=KILL` and flags do not.
        while let Some(o) = seg.get(i).filter(|o| o.starts_with('-') && o.len() > 1) {
            i += if valued.contains(&o.as_str()) { 2 } else { 1 };
        }
        i += operands;
    }
    let Some(first) = seg.get(i) else {
        return (String::new(), Vec::new());
    };
    let base = first
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(first)
        .to_lowercase();
    let base = [".exe", ".cmd", ".bat", ".ps1"]
        .iter()
        .find_map(|x| base.strip_suffix(x))
        .unwrap_or(&base)
        .to_string();
    (base, seg[i + 1..].to_vec())
}

/// `seq[0]` is the command; the rest appear among its words in order, `*` matching any one word.
fn seq_match(seq: &[String], cmd: &str, rest: &[String]) -> bool {
    let Some((head, tail)) = seq.split_first() else {
        return false;
    };
    if !head.eq_ignore_ascii_case(cmd) {
        return false;
    }
    let mut it = rest.iter();
    tail.iter()
        .all(|want| it.any(|w| want == "*" || w.eq_ignore_ascii_case(want)))
}

const WRITERS: [&str; 26] = [
    "rm",
    "mv",
    "cp",
    "tee",
    "truncate",
    "dd",
    "install",
    "ln",
    "del",
    "erase",
    "ri",
    "move",
    "mi",
    "copy",
    "cpi",
    "sc",
    "ac",
    "ni",
    "ren",
    "set-content",
    "add-content",
    "out-file",
    "remove-item",
    "move-item",
    "copy-item",
    "new-item",
];

/// The paths a segment writes: every redirect target, and the arguments of a writing command
/// (`sed -i` and `perl -i` included; PowerShell's `*-Content`, `*-Item`, `Out-File`, `Tee-Object`).
fn written(cmd: &str, rest: &[String], seg: &[String], ps: bool) -> Vec<String> {
    let mut out: Vec<String> = seg
        .windows(2)
        .filter(|w| w[0] == ">")
        .map(|w| w[1].clone())
        .collect();
    let inplace = matches!(cmd, "sed" | "perl")
        && rest
            .iter()
            .any(|w| w.starts_with("-i") || w == "--in-place");
    let writer = WRITERS.contains(&cmd)
        || inplace
        || (ps && matches!(cmd, "clear-content" | "rename-item" | "tee-object"));
    if writer {
        out.extend(
            rest.iter()
                .filter(|w| !w.starts_with('-') && w.as_str() != ">")
                .cloned(),
        );
    }
    out
}

/// A path pattern against a path: case-insensitive, `\` read as `/`, `~/` as the home folder, a
/// relative path joined to the call's working folder. `*` matches any run of characters.
pub fn path_match(pat: &str, path: &str, cwd: &str) -> bool {
    let norm = |s: &str| {
        let s = s.replace('\\', "/");
        let s = match s.strip_prefix("~/") {
            Some(r) => format!("{}/{r}", home().display()).replace('\\', "/"),
            None => s,
        };
        s.to_lowercase()
    };
    let mut p = norm(path);
    let absolute = p.starts_with('/') || p.as_bytes().get(1) == Some(&b':');
    if !absolute && !cwd.is_empty() {
        p = format!("{}/{p}", norm(cwd).trim_end_matches('/'));
    }
    wild(&norm(pat), &p)
}

fn home() -> PathBuf {
    std::env::home_dir().unwrap_or_default()
}

/// Glob with `*` only.
pub fn wild(pat: &str, s: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == s;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !s.starts_with(first)
        || !s[first.len()..].ends_with(last)
        || s.len() < first.len() + last.len()
    {
        return false;
    }
    let mut rest = &s[first.len()..s.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

/// The checkout a folder is in: its folder name and current branch, read from `.git` (a worktree's
/// `.git` file points at its own HEAD). Outside a checkout both are empty.
pub fn place_of(dir: &Path) -> Place {
    let mut d = Some(dir);
    while let Some(cur) = d {
        let git = cur.join(".git");
        let head = if git.is_dir() {
            Some(git.join("HEAD"))
        } else {
            std::fs::read_to_string(&git).ok().and_then(|s| {
                s.trim()
                    .strip_prefix("gitdir:")
                    .map(|g| PathBuf::from(g.trim()).join("HEAD"))
            })
        };
        if let Some(head) = head {
            let branch = std::fs::read_to_string(head)
                .ok()
                .and_then(|h| {
                    h.trim()
                        .strip_prefix("ref: refs/heads/")
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let repo = cur
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Place { repo, branch };
        }
        d = cur.parent();
    }
    Place {
        repo: String::new(),
        branch: String::new(),
    }
}
