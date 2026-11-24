// Draws the README's images with their text outlined, so they render the same with no fonts
// installed: the banner, the hero, the fixed pills, the logo and the live pills' glyph table.
//
//   bun --bun assets/build.ts <fonts-dir>
//
// <fonts-dir> holds static instances of the variable fonts the previews use (Google Fonts, OFL 1.1).
// opentype.js reads variable outlines but not their kerning, so the instances are cut first:
//
//   fonttools varLib.instancer Newsreader[opsz].ttf opsz=72 -o Newsreader-72.ttf
//   fonttools varLib.instancer Newsreader[opsz].ttf opsz=38 -o Newsreader-38.ttf
//   fonttools varLib.instancer HankenGrotesk[wght].ttf wght=400 -o HankenGrotesk-400.ttf   (and 500)
//   fonttools varLib.instancer JetBrainsMono[wght].ttf wght=400 -o JetBrainsMono-400.ttf   (and 500)
//
// Newsreader's optical size follows the type size, as `font-optical-sizing: auto` does: 72 (its
// maximum) for the 120px wordmark, 38 for the hero's 38px lines. Layout follows the CSS of the
// approved previews (.superpowers/previews/brand.html and badges.html), metric for metric.

import opentype from "opentype.js";
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { T, icon, pillBody, r, svg, switched } from "./draw.mjs";
import { LANGS } from "./i18n.mjs";

const fontsDir = process.argv[2];
if (!fontsDir) throw new Error("usage: bun --bun assets/build.ts <fonts-dir>");
const out = import.meta.dir;
mkdirSync(join(out, "badges"), { recursive: true });

const load = (name: string) => {
  const b = readFileSync(join(fontsDir, name));
  return opentype.parse(b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength));
};
const NR72 = load("Newsreader-72.ttf"), NR38 = load("Newsreader-38.ttf");
const HK400 = load("HankenGrotesk-400.ttf"), HK500 = load("HankenGrotesk-500.ttf");
const JB400 = load("JetBrainsMono-400.ttf"), JB500 = load("JetBrainsMono-500.ttf");

type Face = { font: opentype.Font; size: number; ls: number };
const face = (font: opentype.Font, size: number, ls = 0): Face => ({ font, size, ls });
// features is spelled out: opentype.js 2.0.0 drops its default liga when any options are passed
// (updateFeatures filters the default array by itself), and Chrome sets "fi" as one glyph.
const opts = (f: Face) => ({ letterSpacing: f.ls / f.size, features: { liga: true, rlig: true } });
// A character the font lacks would be drawn as its .notdef box: a translation stops the build instead.
const adv = (f: Face, s: string) => {
  for (const c of s) if (f.font.charToGlyph(c).index === 0) throw new Error(`${f.font.names.fullName.en} has no glyph for ${JSON.stringify(c)} in ${JSON.stringify(s)}`);
  return f.font.getAdvanceWidth(s, f.size, opts(f));
};
// Path data written here, not by toPathData: in opentype.js 2.0.0 its object form flips every path
// about its own bounding box (flipY defaults to true), and its rounding writes NaN when a fraction
// prints in exponent form ("1e-7" + "e+2").
const num = (v: number) => String(Math.round(v * 100) / 100);
const d = (p: opentype.Path) =>
  p.commands.map(c =>
    c.type === "Z" ? "Z"
    : c.type + (c.type === "C" ? [c.x1, c.y1, c.x2, c.y2, c.x, c.y] : c.type === "Q" ? [c.x1, c.y1, c.x, c.y] : [c.x, c.y]).map(num).join(" "),
  ).join("");
const ink = (f: Face, s: string, x: number, y: number) => `<path d="${d(f.font.getPath(s, x, y, f.size, opts(f)))}"/>`;
// The hero repeats its letters hundreds of times: each glyph is defined once and placed with <use>.
const fontIds = new Map([[NR72, "a"], [NR38, "b"], [HK400, "c"], [HK500, "d"], [JB400, "e"], [JB500, "f"]]);
class Glyphs {
  defs = new Map<string, string>();
  ink(f: Face, s: string, x: number, y: number) {
    let uses = "";
    adv(f, s); // the coverage check
    f.font.forEachGlyph(s, x, y, f.size, opts(f), (g, gx, gy) => {
      if (!g.path.commands.length) return; // a space
      const id = `${fontIds.get(f.font)}${f.size}-${g.index}`.replace(".", "_");
      if (!this.defs.has(id)) this.defs.set(id, `<path id="${id}" d="${d(g.getPath(0, 0, f.size))}"/>`);
      uses += `<use href="#${id}" x="${r(gx)}" y="${r(gy)}"/>`;
    });
    return uses;
  }
  toString() { return `<defs>${[...this.defs.values()].join("")}</defs>`; }
}
// Every font here sets USE_TYPO_METRICS, so Chrome's content area is typo ascender to descender,
// each rounded to a whole pixel, with the half-leading above it floored (measured against the
// previews: Newsreader 38px is 28 + 10, Hanken 26px is 26 + 8).
const asc = (f: Face) => Math.round((f.font.tables.os2.sTypoAscender / f.font.unitsPerEm) * f.size);
const desc = (f: Face) => Math.round((-f.font.tables.os2.sTypoDescender / f.font.unitsPerEm) * f.size);
const baseline = (f: Face, line: number) => Math.floor((line - asc(f) - desc(f)) / 2) + asc(f);

const themes = ["light", "dark"] as const;
const write = (name: string, body: string) => Bun.write(join(out, name), body);

// ---------- the mark: C · Linked k, a 24 grid at Lucide's stroke ----------
const mark = (x: number, y: number, size: number, base: string, acc: string) =>
  `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">` +
  `<path ${base} d="M6 6v12M6 16l10-6M9.33 14 16 20"/>` +
  `<rect ${base} x="4" y="2" width="4" height="4" rx="1"/><rect ${base} x="4" y="18" width="4" height="4" rx="1"/>` +
  `<rect ${acc} x="16" y="8" width="4" height="4" rx="1"/><rect ${acc} x="16" y="18" width="4" height="4" rx="1"/></svg>`;

// The logo files are used outside the README, so one file follows the viewer's scheme.
const scheme = (light: string, dark: string) => `<style>${light}@media (prefers-color-scheme: dark){${dark}}</style>`;
await write("logo.svg", svg(96, 96, "knowledge",
  scheme(`.b{stroke:${T.light.icon}}.a{stroke:${T.light.accent}}`, `.b{stroke:${T.dark.icon}}.a{stroke:${T.dark.accent}}`) +
  mark(0, 0, 96, 'class="b"', 'class="a"')));
await write("logo-tile.svg", svg(120, 120, "knowledge",
  scheme(`.t{fill:${T.light.accentSoft};stroke:${T.light.accentBorder}}.b{stroke:${T.light.accentText}}.a{stroke:${T.light.accent}}`,
    `.t{fill:${T.dark.accentSoft};stroke:${T.dark.accentBorder}}.b{stroke:${T.dark.accentText}}.a{stroke:${T.dark.accent}}`) +
  `<rect class="t" x=".5" y=".5" width="119" height="119" rx="27.5"/>` + mark(26, 26, 68, 'class="b"', 'class="a"')));

// ---------- the banner: 1280 x 320 ----------
// The motif: a field of linked notes with one path lit, as `knowledge graph A B` draws it. Laid out
// on 760..1220 and squeezed into 850..1230, so the wordmark stays clear.
const nodes = [[760, 70], [840, 150], [800, 240], [910, 90], [960, 200], [1040, 60], [1060, 150], [1010, 260], [1140, 100], [1150, 220], [1220, 160], [880, 280]]
  .map(([x, y]) => [r(850 + ((x - 760) * 380) / 460), y]);
const edges = [[0, 1], [1, 2], [1, 3], [3, 5], [3, 6], [1, 4], [4, 7], [4, 6], [6, 8], [6, 9], [8, 10], [9, 10], [2, 11], [11, 7], [5, 8]];
const litEdges = new Set(["1-4", "4-6", "6-8"]), litNodes = new Set([1, 4, 6, 8]);
const wordmark = face(NR72, 120, -2.4);
for (const th of themes) {
  const t = T[th];
  let motif = "";
  for (const [a, b] of edges) {
    const on = litEdges.has(`${a}-${b}`);
    motif += `<line x1="${nodes[a][0]}" y1="${nodes[a][1]}" x2="${nodes[b][0]}" y2="${nodes[b][1]}" stroke="${on ? t.accent : t.cardBorder}" stroke-width="${on ? 3 : 2}"/>`;
  }
  nodes.forEach(([x, y], i) => {
    const on = litNodes.has(i);
    motif += `<rect x="${r(x - 11)}" y="${y - 11}" width="22" height="22" rx="5" fill="${on ? t.accentSoft : t.chrome}" stroke="${on ? t.accent : t.cardBorder}" stroke-width="2"/>`;
  });
  const body = motif +
    `<rect x="72" y="100" width="120" height="120" rx="28" fill="${t.accentSoft}" stroke="${t.accentBorder}"/>` +
    mark(98, 126, 68, `stroke="${t.accentText}"`, `stroke="${t.accent}"`) +
    `<g fill="${t.text}">${ink(wordmark, "knowledge", 224, 190)}</g>`;
  await write(`banner-${th}.svg`, svg(1280, 320, "knowledge",
    `<rect x=".5" y=".5" width="1279" height="319" rx="12" fill="${t.card}" stroke="${t.cardBorder}"/>` + body));
  // GitHub's social preview (1280 x 640, PNG only): the banner, full bleed, in light. The PNG is
  // rendered from this file; see docs/architecture.md.
  if (th === "light") await write("social.svg", svg(1280, 640, "knowledge",
    `<rect width="1280" height="640" fill="${t.card}"/><g transform="translate(0 160)">${body}</g>`));
}
// The avatar (512 x 512, light): the tile's mark on a full square, since GitHub rounds it itself.
{
  const t = T.light;
  await write("avatar.svg", svg(512, 512, "knowledge",
    `<rect width="512" height="512" fill="${t.accentSoft}"/>` + mark(112, 112, 288, `stroke="${t.accentText}"`, `stroke="${t.accent}"`)));
}

// ---------- inline text: runs, wrapped at spaces by measured width, as CSS wraps them ----------
type Run = { t: string; f: Face; ink: keyof typeof T.light; u?: true; code?: true };
type Placed = { t: string; f: Face; ink: Run["ink"]; u?: true; code?: true; x: number; w: number; padL: number };
const CHIP_PAD = 6;

function wrap(runs: Run[], max: number): Placed[][] {
  // Words are the stretches between spaces; one word may span runs ("memory" + ".").
  // Chrome shapes a same-font stretch as one run, so a piece kerns against the next one even across
  // a space or an element ("They␣", "memory" + "."): each piece's advance takes the kern to the next.
  type Item = { space: boolean; parts: Placed[]; w: number; end: number };
  const bits = runs.flatMap(run => {
    const b = run.t.split(/( )/).filter(Boolean);
    return b.map((t, i) => ({ t, run, padL: run.code && i === 0 ? CHIP_PAD : 0, padR: run.code && i === b.length - 1 ? CHIP_PAD : 0 }));
  });
  const items: Item[] = [];
  bits.forEach(({ t, run, padL, padR }, i) => {
    const next = bits[i + 1], alone = adv(run.f, t);
    const kerned = next && next.run.f === run.f ? adv(run.f, t + next.t[0]) - adv(run.f, next.t[0]) : alone;
    const p: Placed = { t, f: run.f, ink: run.ink, u: run.u, code: run.code, x: 0, w: padL + kerned + padR, padL };
    const last = items.at(-1);
    // `end` is the word's width when it ends a line, with no kern to a space that is dropped.
    if (t === " ") items.push({ space: true, parts: [p], w: p.w, end: p.w });
    else if (last && !last.space) { last.end = last.w + padL + alone + padR; last.parts.push(p); last.w += p.w; }
    else items.push({ space: false, parts: [p], w: p.w, end: padL + alone + padR });
  });
  const lines: Placed[][] = [[]];
  let x = 0, pending: Item | null = null;
  for (const it of items) {
    if (it.space) { pending = it; continue; }
    const gap = lines.at(-1)!.length && pending ? pending.w : 0;
    if (lines.at(-1)!.length && x + gap + it.end > max) { lines.push([]); x = 0; }
    else if (gap) { lines.at(-1)!.push({ ...pending!.parts[0], x }); x += gap; }
    for (const p of it.parts) { lines.at(-1)!.push({ ...p, x }); x += p.w; }
    pending = null;
  }
  return lines;
}

// One line at (x0, base). CSS paints backgrounds, then underlines, then the text over both.
function drawLine(line: Placed[], x0: number, base: number, t: (typeof T)["light"], gl: Glyphs) {
  let chips = "", text = "", under = "";
  for (let i = 0; i < line.length; i++) {
    const p = line[i];
    if (p.code && (i === 0 || !line[i - 1].code)) {
      let j = i;
      while (j + 1 < line.length && line[j + 1].code) j++;
      const end = line[j].x + line[j].w;
      chips += `<rect x="${r(x0 + p.x)}" y="${r(base - asc(p.f))}" width="${r(end - p.x)}" height="${r(asc(p.f) + desc(p.f))}" rx="6" fill="${t.accentSoft}"/>`;
    }
    if (p.t !== " ") text += `<g fill="${t[p.ink]}">${gl.ink(p.f, p.t, x0 + p.x + p.padL, base)}</g>`;
    // text-decoration: underline 3px, text-underline-offset 7px, under the word's whole advance.
    if (p.u) under += `<rect x="${r(x0 + p.x)}" y="${r(base + 7)}" width="${r(p.w)}" height="3" fill="${t.accent}"/>`;
  }
  return chips + under + text;
}

// ---------- the hero: 880 wide, Newsreader lines left, what knowledge does right ----------
const P = face(NR38, 38, -0.76), PL = 38 * 1.1; // .p: 38px, line-height 1.1, letter-spacing -.02em
const F = face(HK400, 26), FL = 26 * 1.3; // .f: 26px, line-height 1.3
const CODE = face(JB400, 22);
const KICK = face(HK500, 20, 1.6), FOOT = face(HK400, 20), FOOTM = face(JB400, 20), BODYL = 30;
const LEFT = 330, GAP = 28, CW = 880 - 2 - 80, X0 = 41;
type Lang = (typeof LANGS)[number];
// i18n.mjs marks an underlined stretch with _…_ and a code chip with `…`.
const runs = (s: string, f: Face, ink: Run["ink"]): Run[] =>
  s.split(/(_[^_]+_|`[^`]+`)/).filter(Boolean).map(t =>
    t[0] === "_" ? { t: t.slice(1, -1), f, ink, u: true }
    : t[0] === "`" ? { t: t.slice(1, -1), f: CODE, ink: "accentText", code: true }
    : { t, f, ink });
const plain = (s: string) => s.replace(/[_`]/g, "");
const dec = (L: Lang, s: string) => s.replace(".", L.dec);
const heroAlt = (L: Lang) => L.rows.map(([a, b]) => `${plain(a)} ${plain(b)}`).join(" ");

// One language's hero, card included, at its own height.
function hero(L: Lang, t: (typeof T)["light"], gl: Glyphs) {
  let body = "", y = 1 + 36;
  // Kicker: a 22px book-open centred on a 30px line, 10px before the caps.
  body += icon("book-open", X0, y + 4, 22, t.accent);
  body += `<g fill="${t.textDim}">${gl.ink(KICK, L.kicker, X0 + 22 + 10, y + baseline(KICK, BODYL))}</g>`;
  y += BODYL + 18;
  for (const [a, b] of L.rows) {
    const left = runs(a, P, "text"), right = runs(b, F, "textDim");
    body += `<rect x="${X0}" y="${y}" width="${CW}" height="1" fill="${t.separator}"/>`;
    y += 1 + 18;
    const pl = wrap(left, LEFT), fl = wrap(right, CW - LEFT - GAP);
    // align-items: baseline puts both first baselines on one line.
    const bp = baseline(P, PL), bf = baseline(F, FL), B = Math.max(bp, bf);
    pl.forEach((l, i) => (body += drawLine(l, X0, y + B + i * PL, t, gl)));
    fl.forEach((l, i) => (body += drawLine(l, X0 + LEFT + GAP, y + B + i * FL, t, gl)));
    y += Math.max(B - bp + pl.length * PL, B - bf + fl.length * FL) + 18;
  }
  // .row:last-of-type matches no row (the foot is the last div), so the rows close without a rule.
  y += 18;
  const fb = y + baseline(FOOT, BODYL);
  body += `<g fill="${t.textDim}">${gl.ink(FOOT, L.foot, X0, fb)}</g>`;
  const footR: Run[] = [
    { t: "prompt hook ", f: FOOT, ink: "textDim" }, { t: dec(L, "~17.8 ms"), f: FOOTM, ink: "text" },
    { t: " · guard ", f: FOOT, ink: "textDim" }, { t: dec(L, "~24.9 ms"), f: FOOTM, ink: "text" },
  ];
  const fr = wrap(footR, Infinity)[0], fw = fr.at(-1)!.x + fr.at(-1)!.w;
  body += drawLine(fr, X0 + CW - fw, fb, t, gl);
  const H = Math.round(y + BODYL + 28 + 1); // whole pixels keep the bottom border sharp
  return { tags: L.tags, w: 880, h: H, body:
    `<rect width="880" height="${H}" rx="12" fill="${t.cardBorder}"/><rect x="1" y="1" width="878" height="${H - 2}" rx="11" fill="${t.card}"/>` + body };
}

// hero-<theme>.svg holds every language (README.md); hero-<lang>-<theme>.svg one (README.<lang>.md).
const en = LANGS.at(-1)!;
for (const th of themes) {
  const gl = new Glyphs(), all = LANGS.map(L => hero(L, T[th], gl));
  await write(`hero-${th}.svg`, switched(all, heroAlt(en), `${gl}`));
  for (const L of LANGS.slice(0, -1)) {
    const one = new Glyphs(), h = hero(L, T[th], one);
    await write(`hero-${L.id}-${th}.svg`, svg(880, h.h, heroAlt(L), `${one}` + h.body));
  }
}

// ---------- pills: 13px Hanken labels, values in Hanken 500 or JetBrains Mono 500 at 12.5 ----------
const LABEL = face(HK400, 13), VAL = face(HK500, 13), MONO = face(JB500, 12.5);
const text = (f: Face, s: string) => ({ w: adv(f, s), body: ink(f, s, 0, 1 + baseline(f, 26)) });
type Val = [kind: string, f: Face, s: string];
const pillsOf = (L: Lang): Record<string, [icon: string, label: string, vals: Val[]]> => ({
  harnesses: ["bot", L.pills.harnesses, ["Claude Code", "Codex", "opencode", L.pills.claudeApp].map((s): Val => ["ok", VAL, s])],
  platforms: ["monitor", L.pills.platforms, [["soft", VAL, "Windows"], ["warn", VAL, L.pills.pending]]],
  "prompt-hook": ["zap", "prompt hook", [["soft", MONO, dec(L, "~17.8 ms")]]],
  guard: ["zap", "guard", [["soft", MONO, dec(L, "~24.9 ms")]]],
  tokens: ["gauge", L.pills.tokens, [["soft", MONO, L.pills.lower]]],
  find: ["search", "knowledge find", [["ok", MONO, "33/33"], ["soft", MONO, "~33 ms"]]],
});
// One language's pill, its letters placed from gl, as the hero's are.
const drawPill = (th: (typeof themes)[number], L: Lang, file: string, gl: Glyphs) => {
  const [ic, label, vals] = pillsOf(L)[file];
  const at = (f: Face, s: string) => ({ w: adv(f, s), body: gl.ink(f, s, 0, 1 + baseline(f, 26)) });
  const alt = `${label}: ${vals.map(v => v[2]).join(", ")}`;
  return { tags: L.tags, alt, ...pillBody(th, ic, at(LABEL, label), vals.map(([kind, f, s]) => ({ kind, ...at(f, s) })), `i-${L.id}`) };
};
// badges/<pill>-<theme>.svg holds every language (README.md); <pill>-<lang>-<theme>.svg one.
for (const file of Object.keys(pillsOf(en)))
  for (const th of themes) {
    const gl = new Glyphs(), all = LANGS.map(L => drawPill(th, L, file, gl));
    await write(`badges/${file}-${th}.svg`, switched(all, all.at(-1)!.alt, `${gl}`));
    for (const L of LANGS.slice(0, -1)) {
      const one = new Glyphs(), p = drawPill(th, L, file, one);
      await write(`badges/${file}-${L.id}-${th}.svg`, svg(p.w, p.h, p.alt, `${one}` + p.body));
    }
  }

// The live pills (stars, latest) are drawn from this table by assets/live.mjs in the badges workflow:
// each language's labels, already outlined, and the digits every language writes its numbers with.
const glyphs: Record<string, string> = {};
for (const c of "0123456789.,kMv") glyphs[c] = d(MONO.font.charToGlyph(c).getPath(0, 1 + baseline(MONO, 26), MONO.size));
const live = {
  adv: adv(MONO, "0"),
  glyphs,
  langs: LANGS.map(L => ({
    id: L.id, tags: L.tags, dec: L.dec,
    keys: {
      stars: { icon: "star", ...text(LABEL, L.pills.stars), label: L.pills.stars },
      latest: { icon: "package", ...text(LABEL, L.pills.latest), label: L.pills.latest },
    },
  })),
};
await write("badges/live.json", JSON.stringify(live) + "\n");
