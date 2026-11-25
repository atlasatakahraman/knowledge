//! `knowledge <verb> --help` (S9.8): one verb's usage, what it does, when to use it and why.
//! `--help` or `-h` anywhere in the arguments prints this and runs nothing, so asking about
//! `records` or `setup` never syncs or writes. The README's Commands table, docs/verbs.md and the
//! wiki's Commands page say the same in short; change them together.

const VERBS: &[(&[&str], &str)] = &[
    (
        &["index"],
        "knowledge index [--rebuild] [<note.md>...]
knowledge index --check [--json]
knowledge index --repo <checkout>

Brings the vault index level with the notes on disk: only changed notes are read, and --rebuild
starts from nothing. --check compares notes on disk with the index (ghosts, missing, stale) and
writes nothing. --repo indexes a checkout's docs/, CLAUDE.md and README.md for `find --repo`.

When: notes changed outside a session (a pull, an edit in Obsidian), or `find` misses a note you
know is there. The hooks keep the index level otherwise.
Why:  find, graph, the prompt hook and the MCP server all read this index; a stale row answers
with an old passage.
",
    ),
    (
        &["find"],
        "knowledge find <query> [--limit N] [--kind K|all] [--json]
knowledge find <query> --captures [--budget N]
knowledge find <query> --repo [<name>]
knowledge find <symbol> --code [--repo <name>] [--limit N] [--budget N]

Ranked notes from the vault index, each with its best passage. --captures searches what `keep`
and `fetch` stored (14 days), --repo the checkouts' docs, --code the code map: where a symbol is
defined, one line each (run by knowledge-code).

When: \"what did I write about X\", once the MOCs did not answer in one hop; a page or output you
kept earlier (--captures); a repo's docs (--repo); where a function or type lives (--code).
Why:  one call returns the note and the passage that matched, in tens of milliseconds, instead of
reading or grepping files into the agent's context.
",
    ),
    (
        &["keep"],
        "knowledge keep <label> [--q <query>]... [--budget N] < input

Stores stdin as a capture (kept 14 days) and prints only the sections the queries match, 2,000
chars at most; --budget sets another cap.

When: a command's output is over ~2,000 chars and you need part of it: a build log, a long
listing, a test run. Shorter output is cheaper read whole.
Why:  the context gets the matching sections, not the whole output, and the rest stays
searchable with `find --captures`.
",
    ),
    (
        &["fetch"],
        "knowledge fetch <url> [--q <query>]... [--budget N] [--allow-private]

One web page as Markdown, stored as a capture and answered the way `keep` answers. Private and
local addresses are refused unless --allow-private (docs/security.md).

When: reading a page whose address you already have: a search result, a library's docs, a
changelog.
Why:  Markdown cut to your query instead of a whole page in context, and no request reaches your
own network by accident.
",
    ),
    (
        &["events"],
        "knowledge events <transcript.jsonl> [--json]

A session transcript's decisions, constraints, plans and refusals.

When: reviewing what a past session decided, or checking what the capture hook would take from a
transcript.
Why:  prints only the turns that matter, without opening a transcript that runs to megabytes.
",
    ),
    (
        &["inject"],
        "knowledge inject <text> [--format lines|tagged] [--no-passage | --passage-max N --passage-score S]

What the prompt hook would add to a prompt with this text.

When: a prompt brought the wrong notes, or none, and you want to see why; tuning the passage
limits.
Why:  it is the hook's own code path run by hand, so what you see is what the agent got.
",
    ),
    (
        &["graph"],
        "knowledge graph <note> [<other note>] [--json]
knowledge graph <note> --text
knowledge graph <symbol> [<other symbol>] --code [--repo <name>] [--budget N]

A note's links in and out, or the shortest path between two notes; --text prints the note
itself. With --code: what uses a symbol and what it uses, or the chain of uses between two
(4 hops at most), from the code map (run by knowledge-code).

When: \"how does X connect to Y\", before reading notes one by one; what calls a function before
you change it (--code).
Why:  the links are already in the index, so the answer costs ~25 ms and reads no file.
",
    ),
    (
        &["code"],
        "knowledge code index [<repo>|--all] [--rebuild]
knowledge code duplicate [--staged] [<repo>] [--budget N]

Run by knowledge-code. index brings a checkout's code map level (only changed files are parsed);
--rebuild starts it from nothing. duplicate names a new or changed definition whose body matches
another's, or whose name a sibling app under apps/ defines; it warns and exits 0.

When: index, to level every map at once or rebuild a broken one (find and graph --code level a
map on their own); duplicate, before a commit in a monorepo.
Why:  one SQLite file per checkout answers symbol questions at zero tokens; duplicate catches a
copy before it becomes two things to maintain.
",
    ),
    (
        &["note"],
        "knowledge note log <text>
knowledge note decision <text> [--question <q>] [--promoted <path>]

log: one bullet in today's session note (the day rolls over at rolloverHour), indexed at once.
decision: one line in the decision ledger, 50-Ops/decisions.jsonl.

When: in a harness that runs no hooks (the Claude app, a generic agent), or to record a decision
the capture missed.
Why:  the same write-back the hooks do, so the record exists whether or not the harness can run
hooks.
",
    ),
    (
        &["ledger"],
        "knowledge ledger list
knowledge ledger promote <src> <path|not-a-decision>

list: the captured decisions not yet promoted to a note. promote: marks one as promoted, to the
note that records it, or as not a decision.

When: the session start says decisions are waiting; `/knowledge new decision --ledger` drives it.
Why:  a captured decision is raw; promoting it to a decision record, or ruling it out, keeps the
ledger from piling up unread.
",
    ),
    (
        &["config"],
        "knowledge config [<key>]

This machine's paths and settings, as the hooks see them; with a key, that one value (a folder
key prints the path in effect).

When: a script needs the vault or checkouts folder (`knowledge config vault`), or a hook acts on
the wrong folder.
Why:  each machine's config.json is the one source; asking the binary keeps a path that differs
between machines out of your scripts.
",
    ),
    (
        &["rules"],
        "knowledge rules list [--json] | kernel
knowledge rules check [--staged] [--restore]
knowledge rules lock [--seed <file>]
knowledge rules enable|disable <pack>
knowledge rules guard add --rule <id> --action deny|ask --reason <text> [--words ...] [--paths ...]
                          [--tools ...] [--backstop ...] [--unless ...] [--repo ...] [--branch ...] [--os ...]

list: the packs, rules and guards in the lock; kernel: the rules that are code, not text. check:
Hard-Rules against the lock (--staged the staged copy, --restore a changed rule back from HEAD).
lock: re-stamps the lock after you edit Hard-Rules. enable|disable: a pack's guards. guard add:
a new guard; add-only, and the rules-files guard asks first.

When: after editing Hard-Rules (lock); when the doctor's rules check fails (check); to turn a
rule into a guard the agent cannot talk its way past (guard add).
Why:  a rule in a file is advice; a guard on every tool call is enforcement, and the lock shows
any edit that did not go through you.
",
    ),
    (
        &["hook"],
        "knowledge hook session|prompt|pre-tool|post-edit|post-bash|capture --harness <id> [--pin <version>]

The hooks themselves: each reads the harness's JSON on stdin and always exits 0. With --pin, a
binary of another version stands down.

When: not by hand in normal use; setup writes these into each harness's hook config. Run one by
hand only to debug, with a saved payload on stdin.
Why:  one binary serves every harness, so Claude Code, Codex and opencode run the same code on
the same vault.
",
    ),
    (
        &["doctor"],
        "knowledge doctor [--pretty] [--repair] [--record] [--only <id,...>]
knowledge doctor --only portable [--json]
knowledge doctor budget [<transcript.jsonl>] [--json]
knowledge doctor profile <note>

Retrieval health at zero tokens: the index, the vault's own questions, links, pointers, priming,
rules, sync and auth. --repair rebuilds what is derived; --record writes the result as a health
line. portable: names and paths that break on the other OS. budget: what a session's start
cost, by source. profile: the per-project settings that leave unlisted connectors off.

When: first, whenever something seems off; after setup or an update; on a new machine.
Why:  a pipeline like this fails by going quiet; the doctor is the one place that says which
part did.
",
    ),
    (
        &["setup"],
        "knowledge setup --plan [--vault <path>] [--dev <path>] [--machine <text>] [--from-template | --adopt]
                       [--signing] [--skeleton <checkout>]
knowledge setup --apply <id> [the same options]
knowledge setup --remove --plan | --remove --apply <id>
knowledge setup --harness claude|codex|opencode|claude-desktop|generic ...

Plans this machine's setup across every harness it finds (config, hooks, plugins, links, git
hooks) and prints it with an id; --apply <id> carries out exactly that plan. --remove plans and
applies the undo. --harness limits it to one harness (docs/install.md, docs/remove.md).

When: a new machine, a new harness, after an update, or after a rules or decisions change.
Why:  nothing is written before you have read the plan, and the id ties the apply to the plan
you read.
",
    ),
    (
        &["update"],
        "knowledge update --plan

One HTTPS call for the latest release, then this platform's steps. Downloads nothing and installs
nothing (docs/update.md).

When: you choose to check for a new version; nothing checks on its own.
Why:  no telemetry and no auto-update: the network is touched only when you ask.
",
    ),
    (
        &["flush"],
        "knowledge flush <session-id> [<cwd>] [--transcript <file>] [--date <YYYY-MM-DD>] [--dry-run [--from-start]]

A session's new turns into the daily log, once, through a headless run of the harness's model.
--dry-run prints what it would send; --from-start ignores where the last flush stopped.

When: the end-of-session hook runs it; by hand when the doctor's flush canary says a session
never reached the log.
Why:  the daily log is what compile turns into notes; a session never flushed is never
remembered.
",
    ),
    (
        &["compile"],
        "knowledge compile [--force] [--dry-run]

A finished daily log into notes and decisions, once per day; --force runs it again.

When: the session hook runs it for the day before; by hand after a missed day.
Why:  the vault grows from the day's log without anyone writing it up.
",
    ),
    (
        &["post-bash"],
        "knowledge post-bash [--backfill <since>] [--dry-run]

One note per commit in 50-Ops/Commits. The post-bash hook runs it after a git commit;
--backfill <since> records commits made outside a session.

When: commits were made in a terminal with no agent, and the commit notes are missing them.
Why:  every commit's why-body lands in the vault, so \"why did X change\" is one find away.
",
    ),
    (
        &["records"],
        "knowledge records [<session-id>]

Commits the records the hooks wrote (session notes, commit notes, the ledger, health lines) after
a gitleaks scan, and pushes only when every unpushed commit is a record; otherwise it holds them.
With a session id, it waits for that session's flush first.

When: the hooks run it at session start and end; by hand after the doctor's auth check names a
failed run.
Why:  the other machine pulls what this one recorded, and nothing you committed yourself is
pushed without your yes.
",
    ),
    (
        &["hubs"],
        "knowledge hubs

Rewrites the generated hub notes from the folders, so every note stays within two hops of
00-MOC-Root (records within three). `doctor --repair` runs it too.

When: a note you added or moved is not yet reachable from 00-MOC-Root.
Why:  the hubs are written from the folders every time, so they cannot drift; never edit one by
hand.
",
    ),
    (
        &["usage"],
        "knowledge usage [--days N] | --month <YYYY-MM> [--projects] [--blocks]

The token, model and cost digest from Claude Code's transcripts: the last N days (30) to stdout,
or one month to 50-Ops/Usage. --projects splits it by project, --blocks by rolling 5-hour window.

When: what a month cost, which project used the most, how close a 5-hour window came to the
limit.
Why:  read from the local transcripts and sent nowhere. Token counts are exact; the prices are
estimates.
",
    ),
    (
        &["mcp"],
        "knowledge mcp

An MCP server on stdio with four tools: find, graph, read (read-only) and note; find and graph
take code: true for the code map.

When: an app that speaks MCP but runs no hooks, such as the Claude app; setup registers it.
Why:  the same index and code map as the command line, for apps that cannot run a shell.
",
    ),
    (
        &["--version", "-V", "version"],
        "knowledge --version

The version, and whether SQLite's FTS5 is on; exits 1 when it is off.

When: after an install or update, and in a bug report.
Why:  an index without FTS5 cannot search; this says so before find fails.
",
    ),
];

/// One verb's help, or `None` for a verb that has none (the caller prints the full usage).
pub fn verb(name: &str) -> Option<&'static str> {
    VERBS
        .iter()
        .find(|(names, _)| names.contains(&name))
        .map(|(_, text)| *text)
}
