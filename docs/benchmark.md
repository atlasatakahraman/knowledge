# Benchmark

What the README's numbers are, how each was measured, and how to rerun the ones that can be.

## Tokens to an answer

How much an agent reads to reach the answer to a question: with `knowledge find`, against grep
and the Read tool. **82% fewer tokens overall** on the run below.

| | Found | knowledge | grep + Read | Fewer |
|---|---|---|---|---|
| Notes, 33 questions | 33/33 | ~80,377 tokens | ~320,211 | **75%** (46% against the best single-word grep) |
| Code, 20 symbols | 20/20 | ~2,866 | ~149,766 | **98%** |
| All | | ~83,242 | ~469,977 | **82%** |

`knowledge find` took ~33.4 ms per question (mean of the per-question medians), `find --code`
~65.5 ms, and `find --code` with `graph --code` ~131.6 ms. Windows 10, Ryzen 5 5500, release
build, 2026-11-24.

### Rerun it

```bash
cargo build --release --locked
bun --bun bench/tokens.ts target/release/knowledge
```

It prints one row per question and the three summary lines above, and removes its temp folder.

### The fixture

The number is public because the fixture is this repository: `template/` as the vault, with
`docs/*.md`, `README.md` (as `Overview.md`) and `AGENTS.md` (as `Agent-Install.md`) added under
`20-Knowledge/knowledge/`; and a clone of its `HEAD` as the checkout. Both sit in a temp folder with
their own `KNOWLEDGE_HOME` and an empty `HOME`, so the binary sees nothing else. The fixture copies
the working tree, so the numbers move a little with the docs; a run is pinned by its commit.

The questions are in [`bench/questions.json`](../bench/questions.json): 33 questions a user asks
about notes, each naming the note that answers it, and 20 symbols, each naming the file that
defines it. The symbols were picked by a rule, not by hand: each `fn` defined once in `src/`, with a
name of 6 or more characters and at least 3 uses across at least 2 files; the first 20 by the SHA-1
of the name.

### The method

Characters are counted, the way the agent's context receives them; tokens are estimated at 4
characters each. The percentages are ratios of characters, so they do not depend on that estimate.

- **knowledge, notes:** `find`'s output, then one whole Read of the note it names. A question whose
  note `find` does not list pays `find`'s output and then grep's whole cost on top.
- **grep, notes, two baselines.** Realistic: grep for every word of the question (stopwords and
  words under 3 letters dropped), rank the files by how many of the words they hold, then by total
  matches, and Read whole files in that order until the answer's note is read. Best case: the one
  word of the question that reaches the note for the least, an oracle no agent has, which only errs
  in grep's favour.
- **knowledge, code:** `find <symbol> --code` and `graph <symbol> --code`, both outputs.
- **grep, code:** `git grep -n -w` over `*.rs *.ts *.mjs *.js *.sh`, then a Read of 40 lines above
  and 5 below each use (where the enclosing function starts), merged per file. The definition's own
  line needs no Read.
- **Reads** are counted as the Read tool returns them: `cat -n` format, a 6-wide line number and a
  tab, at most 2,000 lines.
- **Latency** is the median of 5 runs per question, then the mean across questions, measured from
  process start to exit.

## The hooks

| | Measured | Budget |
|---|---|---|
| Prompt hook (`hook prompt`), in process | 12.3 / 17.4 / 17.8 ms over three prompts | 20 ms |
| Guard (`hook pre-tool` on `npm i`, a matched deny), wall | 24.9 ms | 30 ms |

The README's pills quote the slowest of each. The prompt hook's wall time was 36.8–42.5 ms, beside
a floor of 24.5 ms for the bare process start (`knowledge --version`) on the same machine; the
budget is set in process because that floor belongs to the OS, not the hook. These were measured on
the maintainer's own vault (about 2,000 notes, private), not on the fixture, so their method is
public and their inputs are not.

## The code map against graphify

Before the code map, a graphify index answered symbol questions. On the same 20 symbol questions
in the maintainer's checkouts, the code map found 20 to graphify's 15, at a mean of 244 ms to its
1,481 ms, which is why it replaced graphify. The fixture run above measures the code map alone.
