//! `knowledge update --plan` (Stage 9, D17): on request only, one HTTPS call for the latest
//! release, then the download, verify and replace steps for this platform. It downloads nothing
//! and replaces nothing, and nothing runs it on its own: an updater that phones home is a
//! telemetry surface ([[0038-no-in-app-updater]]). `--apply` is not built: on Windows the replace
//! needs every app that runs `knowledge mcp` quit first, and that stays a step the user takes.
use crate::{Args, Fault, config, emit, fetch, setup, state};

const USAGE: &str = "knowledge update --plan";

/// The release key, built in from the repo's `allowed_signers` (docs/release.md): an update is
/// checked against the key the user already trusts, not one fetched beside the new archive.
const SIGNERS: &str = include_str!("../allowed_signers");

/// The release archive this binary came from: one of `build.yml`'s eight targets.
const TARGET: &str = if cfg!(all(windows, target_arch = "x86_64")) {
    "x86_64-pc-windows-msvc"
} else if cfg!(all(windows, target_arch = "aarch64")) {
    "aarch64-pc-windows-msvc"
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    "x86_64-apple-darwin"
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    "aarch64-apple-darwin"
} else if cfg!(all(
    target_os = "linux",
    target_arch = "x86_64",
    target_env = "musl"
)) {
    "x86_64-unknown-linux-musl"
} else if cfg!(all(
    target_os = "linux",
    target_arch = "aarch64",
    target_env = "musl"
)) {
    "aarch64-unknown-linux-musl"
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    "x86_64-unknown-linux-gnu"
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    "aarch64-unknown-linux-gnu"
} else {
    ""
};

pub fn run(raw: &[String]) -> Result<u8, Fault> {
    let args = Args::parse(raw, &[])?;
    if args.has("apply") {
        return Err(Fault::Usage(format!(
            "{USAGE}: --apply is not built. Replacing the binary needs every app that runs \
             `knowledge mcp` quit first, so the plan prints the steps and you take them"
        )));
    }
    if !args.has("plan") || !args.words.is_empty() {
        return Err(Fault::Usage(USAGE.into()));
    }
    let version = env!("CARGO_PKG_VERSION");
    let repo = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    let slug = repo.trim_start_matches("https://github.com/");
    let target = if TARGET.is_empty() {
        "a target with no release archive"
    } else {
        TARGET
    };
    let mut out = format!("knowledge {version}, {target}\n");
    let api = format!("https://api.github.com/repos/{slug}/releases/latest");
    let (status, _, _, body) = fetch::request(&api, false)?;
    let sk = config::load()
        .ok()
        .and_then(|c| setup::checkout(&c.dev))
        .map_or_else(|| "<checkout>".to_string(), |p| state::native(&p));
    if status == 404 {
        // GitHub answers 404 for a repo with no published release (a draft is not one).
        out.push_str(&format!(
            "No release is published yet at {repo}/releases.\n\
             Update from the checkout instead:\n  \
             git -C {sk} pull --ff-only\n  \
             cargo install --path {sk} --locked\n  \
             knowledge setup --plan     (then --apply <id>)\n"
        ));
        emit(&out);
        return Ok(0);
    }
    if !(200..300).contains(&status) {
        return Err(Fault::Net(format!("{api}: HTTP {status}")));
    }
    let rel: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| Fault::Net(format!("{api}: not the release JSON: {e}")))?;
    let tag = rel["tag_name"].as_str().unwrap_or_default();
    let page = rel["html_url"].as_str().unwrap_or_default();
    out.push_str(&format!("latest release: {tag}  {page}\n"));
    if tag.trim_start_matches('v') == version {
        out.push_str("This binary is the latest release. Nothing to do.\n");
        emit(&out);
        return Ok(0);
    }
    let assets = rel["assets"].as_array().map_or(&[][..], Vec::as_slice);
    let url = |pick: &dyn Fn(&str) -> bool| {
        assets.iter().find_map(|a| {
            let name = a["name"].as_str()?;
            pick(name).then(|| a["browser_download_url"].as_str().map(str::to_string))?
        })
    };
    let marker = format!("-{TARGET}.");
    let Some(archive) = url(&|n| !TARGET.is_empty() && n.contains(&marker)) else {
        out.push_str(&format!(
            "{tag} has no archive for {target}: build it from the checkout (docs/install.md).\n"
        ));
        emit(&out);
        return Ok(1);
    };
    let file = archive.rsplit('/').next().unwrap_or(&archive).to_string();
    let sums = url(&|n| n == "SHA256SUMS").unwrap_or_else(|| "(no SHA256SUMS: stop here)".into());
    let sig =
        url(&|n| n == "SHA256SUMS.sig").unwrap_or_else(|| "(no SHA256SUMS.sig: stop here)".into());
    let exe =
        std::env::current_exe().map_err(|e| Fault::Config(format!("this binary's path: {e}")))?;
    let dir = exe
        .parent()
        .map_or_else(|| "<its folder>".to_string(), state::native);
    // docs/verify.md's three checks, in its order. PowerShell has no `<`, and a pipe through it
    // would change the file's bytes, so `cmd` feeds SHA256SUMS to ssh-keygen.
    let who = SIGNERS.split_whitespace().next().unwrap_or_default();
    let verify = format!(
        "ssh-keygen -Y verify -f allowed_signers -I {who} -n file -s SHA256SUMS.sig < SHA256SUMS"
    );
    let (signed, check) = if cfg!(windows) {
        (
            format!("cmd /c \"{verify}\""),
            format!(
                "$f = \"{file}\"; ((Get-FileHash $f -Algorithm SHA256).Hash + \"  $f\") -in (Get-Content SHA256SUMS)"
            ),
        )
    } else {
        let sum = if cfg!(target_os = "macos") {
            "shasum -a 256"
        } else {
            "sha256sum"
        };
        (
            verify,
            format!("grep -F \"  {file}\" SHA256SUMS | {sum} -c -"),
        )
    };
    let provenance = format!(
        "gh attestation verify {file} --repo {slug} --signer-workflow \
         {slug}/.github/workflows/release.yml --source-ref refs/tags/{tag} --deny-self-hosted-runners"
    );
    let quit = if cfg!(windows) {
        "Quit every app that runs `knowledge mcp` (the Claude desktop app, its tray icon too; Codex; \
         opencode): Windows will not replace a running .exe."
    } else {
        "Quit every app that runs `knowledge mcp` (the Claude desktop app, Codex, opencode)."
    };
    out.push_str(&format!(
        "\n1. Download into an empty folder:\n   {archive}\n   {sums}\n   {sig}\n\
         2. Verify before extracting (docs/verify.md has each step). Save this line as\n   \
         allowed_signers beside them: the release key this binary was built with, so the new\n   \
         release is checked against a key you already trust, not one fetched beside it:\n     \
         {signers}\n   \
         {signed}\n   {check}\n   {provenance}\n\
         3. {quit}\n   Then extract knowledge and knowledge-code over the ones in {dir}\n\
         4. Move the checkout to the release, so its plugins carry the new version (a checkout on \
         a branch pulls instead):\n   \
         git -C {sk} fetch --tags\n   \
         git -C {sk} -c advice.detachedHead=false checkout {tag}\n\
         5. knowledge setup --plan, then --apply <id>: the Codex and opencode plugins move to the \
         new version, and Codex asks for its hook review again\n",
        signers = SIGNERS.trim()
    ));
    emit(&out);
    Ok(0)
}
