// Draws the README's live pills for the badges workflow, from the glyph table build.ts writes:
//
//   bun --bun assets/live.mjs <stars> <tag> <out-dir>
//
// Writes stars-<theme>.svg and latest-<theme>.svg, every language in one file (README.md), and
// stars-<lang>-<theme>.svg and latest-<lang>-<theme>.svg for each translated README.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { liveValue, pill, pillBody, switched } from "./draw.mjs";

const [stars, tag, out] = process.argv.slice(2);
if (!/^\d+$/.test(stars ?? "") || !tag || !out) throw new Error("usage: bun --bun assets/live.mjs <stars> <tag> <out-dir>");
const table = JSON.parse(readFileSync(new URL("badges/live.json", import.meta.url), "utf8"));

// 1234 reads 1.2k and 12345 reads 12.3k (1,2k where the decimal mark is a comma), rounded down so
// a count is never overstated.
const n = Number(stars);
const count = dec => {
  const short = (div, unit) => `${String(Math.floor((n / div) * 10) / 10).replace(".", dec)}${unit}`;
  return n < 1e3 ? String(n) : n < 1e6 ? short(1e3, "k") : short(1e6, "M");
};
const value = (L, key) => (key === "stars" ? count(L.dec) : tag);
const en = table.langs.at(-1);

for (const theme of ["light", "dark"])
  for (const key of ["stars", "latest"]) {
    const variants = table.langs.map(L => {
      const k = L.keys[key];
      return { tags: L.tags, ...pillBody(theme, k.icon, k, [liveValue(table, value(L, key))], `i-${L.id}`) };
    });
    writeFileSync(join(out, `${key}-${theme}.svg`), switched(variants, `${en.keys[key].label}: ${value(en, key)}`));
    for (const L of table.langs.slice(0, -1)) {
      const k = L.keys[key], v = value(L, key);
      writeFileSync(join(out, `${key}-${L.id}-${theme}.svg`), pill(theme, k.icon, k, [liveValue(table, v)], `${k.label}: ${v}`));
    }
  }
