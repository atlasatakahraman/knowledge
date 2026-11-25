//! Hook-written records committed and pushed, so the other boot pulls them (brain-lib's
//! syncRecords and brain-records.mjs, ported in Stage 6), and the pull-trust test every move of
//! HEAD onto its upstream asks first ([[0039-pulls-trust-signed-commits-verified-before-fast-forward]]).
//!
//!   knowledge records [<session-id>]   with an id: wait for that session's flush, then sync.
//!                                      Without: sync now and print the result.
use crate::config::Config;
use crate::recorder::is_record_path;
use crate::state::{self, J, OPS, Out, SESSIONS};
use crate::{Args, Fault, emit};
use std::path::Path;
use std::time::Duration;

pub const UNTRUSTED_TIP: &str = "the tip is not signed by a known key";

/// May HEAD take @{u}? Yes when pullTrust is unset or nothing is incoming; else only when the tip's
/// `%G?` is `G` against the allowed_signers passed per call (nothing inside the repo decides).
pub fn tip_trusted(cfg: &Config, git: &dyn Fn(&[&str]) -> Out) -> bool {
    if !cfg.pull_trust {
        return true;
    }
    if git(&["rev-list", "--count", "HEAD..@{u}"]).out() == "0" {
        return true;
    }
    let signers = cfg.trust_dir.join("allowed_signers");
    let flag = format!("gpg.ssh.allowedSignersFile={}", state::native(&signers));
    signers.exists() && git(&["-c", &flag, "log", "-1", "--format=%G?", "@{u}"]).out() == "G"
}

fn record_paths() -> [String; 6] {
    [
        SESSIONS.to_string(),
        format!("{OPS}/Commits"),
        format!("{OPS}/decisions.jsonl"),
        format!("{OPS}/Health"),
        format!("{OPS}/Usage"),
        format!("{OPS}/00-MOC-Ops.md"),
    ]
}

/// Commit the records and push them. `{ busy }`, `{ skipped }`, or
/// `{ committed, pushed, pulled, held?, refused?, failed? }`, keys in bun's order.
/// - Records only, and nothing when other changes are staged.
/// - gitleaks scans the staged records; a finding or a missing gitleaks commits nothing.
/// - Pushes only when every unpushed commit is a record: a push sends the owner's work too.
/// - Behind origin, it rebases (the records merge by union); an untrusted tip is never taken.
pub fn sync(cfg: &Config) -> J {
    let vault = cfg.vault.clone();
    state::with_lock("records", 300_000, || sync_locked(cfg, &vault))
        .unwrap_or_else(|| state::obj([("busy", J::Bool(true))]))
}

fn sync_locked(cfg: &Config, vault: &Path) -> J {
    let git = |a: &[&str]| state::git(vault, a, Duration::from_secs(30));
    let list =
        |a: &[&str]| -> Vec<String> { git(a).lines().into_iter().map(str::to_string).collect() };
    let skipped = |why: &str| state::obj([("skipped", J::s(why))]);
    let git_dir = vault.join(".git");
    if !git(&["symbolic-ref", "-q", "HEAD"]).ok() {
        return skipped("detached HEAD");
    }
    if ["rebase-merge", "rebase-apply", "MERGE_HEAD"]
        .iter()
        .any(|f| git_dir.join(f).exists())
    {
        return skipped("a merge or rebase is in progress");
    }
    if list(&["diff", "--cached", "--name-only"])
        .iter()
        .any(|f| !is_record_path(f))
    {
        return skipped("other changes are staged");
    }
    // One missing pathspec aborts the whole `git add`: only what exists or is tracked (deleted).
    let paths: Vec<String> = record_paths()
        .into_iter()
        .filter(|p| vault.join(p).exists() || !list(&["ls-files", "--", p]).is_empty())
        .collect();
    let path_refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    fn with_paths<'a>(head: &[&'a str], paths: &[&'a str]) -> Vec<&'a str> {
        let mut v = head.to_vec();
        v.extend(paths.iter().copied());
        v
    }
    if !paths.is_empty() {
        git(&with_paths(&["add", "-A", "--"], &path_refs));
    }
    let staged = list(&["diff", "--cached", "--name-only"]);
    let mut res = state::obj([
        ("committed", J::n(staged.len() as i64)),
        ("pushed", J::n(0)),
        ("pulled", J::n(0)),
    ]);
    let with = |mut r: J, k: &str, v: J| {
        r.set(k, v);
        r
    };
    if !staged.is_empty() {
        let leak = state::run(
            Path::new("gitleaks"),
            &[
                "git",
                "--pre-commit",
                "--staged",
                "--no-banner",
                "-l",
                "warn",
            ],
            Some(vault),
            None,
            &[],
            Duration::from_secs(60),
        );
        if leak.missing {
            git(&with_paths(&["reset", "-q", "--"], &path_refs));
            return with(
                with(res, "committed", J::n(0)),
                "skipped",
                J::s("gitleaks not installed"),
            );
        }
        if !leak.ok() {
            git(&with_paths(&["reset", "-q", "--"], &path_refs));
            return with(
                with(res, "committed", J::n(0)),
                "failed",
                J::s("gitleaks flagged the staged records; nothing committed"),
            );
        }
        let subject = format!("chore(knowledge): records from the {} boot", cfg.os);
        let body = format!(
            "Written by the hooks after the last commit, committed by `knowledge records` so the other\nboot pulls them:\n{}",
            staged
                .iter()
                .map(|f| format!("- {f}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let c = git(&["commit", "-q", "-m", &subject, "-m", &body]);
        if !c.ok() {
            // Left staged, the records would ride along in the owner's next commit by hand.
            git(&with_paths(&["reset", "-q", "--"], &path_refs));
            return with(
                with(res, "committed", J::n(0)),
                "failed",
                J::s(format!("commit: {}", state::why(&c, true))),
            );
        }
    }
    if !git(&["rev-parse", "--abbrev-ref", "@{u}"]).ok() {
        return with(res, "skipped", J::s("no upstream"));
    }
    // The pull's lock, not "records": a session-start pull fetching at the same moment moved
    // origin/main under this fetch and failed "cannot lock ref" (2026-11-06). An outside fetcher
    // the lock cannot see is what `state::fetch`'s one retry covers.
    let name = vault
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(f) = state::with_lock(&format!("pull-{name}"), 60_000, || {
        state::fetch(vault, Duration::from_secs(30))
    }) else {
        return with(res, "busy", J::Bool(true));
    };
    if !f.ok() {
        return with(
            res,
            "failed",
            J::s(format!("fetch: {}", state::why(&f, true))),
        );
    }
    let unpushed = list(&["rev-list", "@{u}..HEAD"]);
    if unpushed.iter().any(|h| {
        list(&["show", "--name-only", "--format=", h])
            .iter()
            .any(|p| !is_record_path(p))
    }) {
        return with(res, "held", J::Bool(true));
    }
    let pulled: i64 = git(&["rev-list", "--count", "HEAD..@{u}"])
        .out()
        .parse()
        .unwrap_or(0);
    res.set("pulled", J::n(pulled));
    if pulled > 0 && !tip_trusted(cfg, &git) {
        state::record_failure("pull-vault", UNTRUSTED_TIP);
        return with(with(res, "pulled", J::n(0)), "refused", J::Bool(true));
    }
    // Origin changed files that are also changed here, uncommitted: nothing is merged, the records
    // wait committed, and the session pull right after says why in its own line. Not a failure.
    if pulled > 0 && state::incoming_overlaps(vault) {
        return with(
            with(res, "pulled", J::n(0)),
            "skipped",
            J::s("incoming commits touch uncommitted files"),
        );
    }
    if pulled > 0 && !unpushed.is_empty() {
        let r = git(&["rebase", "--quiet", "--autostash", "@{u}"]);
        if !r.ok() {
            git(&["rebase", "--abort"]);
            return with(
                res,
                "failed",
                J::s(format!("rebase onto origin: {}", state::why(&r, true))),
            );
        }
    } else if pulled > 0 {
        let m = git(&["merge", "--ff-only", "--quiet", "@{u}"]);
        if !m.ok() {
            return with(
                res,
                "failed",
                J::s(format!("fast-forward: {}", state::why(&m, true))),
            );
        }
    }
    if unpushed.is_empty() {
        return res;
    }
    let p = git(&["push", "--quiet"]);
    if !p.ok() {
        return with(
            res,
            "failed",
            J::s(format!("push: {}", state::why(&p, true))),
        );
    }
    res.set("pushed", J::n(unpushed.len() as i64));
    res
}

/// What a sync's result means for the failure record: a failure is kept, a done sync clears it.
pub fn note_result(r: &J) {
    if let Some(f) = r.get("failed").and_then(J::str) {
        state::record_failure("records", f);
    } else if r.get("busy").is_none() {
        state::clear_failure("records");
    }
}

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &[])?;
    let cfg = crate::config::load()?;
    let sid = state::clean_sid(args.words.first().map_or("", String::as_str));
    if !sid.is_empty() {
        // The flush takes its lock a moment after capture detaches it, and holds it through its
        // `claude -p` (120 s timeout).
        std::thread::sleep(Duration::from_secs(5));
        let lock = state::state_dir().join(format!("flush-{sid}.lock"));
        let mut waited = 0;
        while lock.exists() && waited < 200_000 {
            std::thread::sleep(Duration::from_secs(2));
            waited += 2_000;
        }
    }
    let r = sync(&cfg);
    let mut fields = vec![
        ("hook".to_string(), J::s("records")),
        (
            "sid".to_string(),
            if sid.is_empty() { J::Null } else { J::s(&sid) },
        ),
    ];
    if let J::Obj(items) = &r {
        fields.extend(items.iter().cloned());
    }
    state::log_event(&cfg, fields);
    note_result(&r);
    if sid.is_empty() {
        emit(&format!("{}\n", r.text()));
    }
    Ok(0)
}
