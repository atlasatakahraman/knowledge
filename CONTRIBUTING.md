# Contributing

Thank you for looking. The bar for every change is **Blazingly Fast, Most Efficient, Most
Secure**: the least code that solves the problem, no telemetry, nothing that phones home.

## Where to start

| You have | Go to |
|---|---|
| A question about install, setup or use | [Discussions → Q&A](https://github.com/atlasatakahraman/knowledge-skeleton/discussions/categories/q-a) |
| An idea or a feature request | [Discussions → Ideas](https://github.com/atlasatakahraman/knowledge-skeleton/discussions/categories/ideas) |
| Something broken | [a bug report](https://github.com/atlasatakahraman/knowledge-skeleton/issues/new/choose) |
| A security problem | a private report, never a public issue ([SECURITY.md](SECURITY.md)) |

Open an issue or an idea before a large pull request, so the approach is agreed before the work.

## Making a change

The layout, the build and the checks every commit passes are in
[AGENTS.md § Working on this repository](AGENTS.md#working-on-this-repository). How the parts fit
together is in [docs/architecture.md](docs/architecture.md). In short:

1. Never run a build against your real home: the checks point `HOME`, `USERPROFILE` and
   `KNOWLEDGE_HOME` at a fixture, and so should any probe.
2. No new crate without asking first. `checks/crates.txt` lists every crate in the tree, CI fails on
   any other, and the licence must fit MIT (no GPL).
3. A doc's command output is pasted from a real run on a fixture vault, never written from memory.
4. Commits are by area, each building on its own, with a body that says why.

By contributing you agree that your work is licensed under the [MIT licence](LICENSE).
