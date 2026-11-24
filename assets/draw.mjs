// Shared by assets/build.ts (every committed image) and the badges workflow (the live pills).
// Plain ESM with no imports, so the workflow can run it without the fonts or opentype.js.

// Design.md § Tokens.
export const T = {
  light: {
    chrome: "#f9f5f7", card: "#fffbfd", cardBorder: "#e2dedf", separator: "#e6e1e3",
    text: "#1d1619", textDim: "#675f62", icon: "#423b3e", warning: "#be8214",
    accent: "#dc1360", accentText: "#9c2f47", accentSoft: "#fadae7", accentBorder: "#e989ae",
  },
  dark: {
    chrome: "#171315", card: "#1d191b", cardBorder: "#2b2729", separator: "#292627",
    text: "#f1ebed", textDim: "#9c9598", icon: "#cac3c6", warning: "#f0be5a",
    accent: "#ff3a78", accentText: "#ffc4dd", accentSoft: "#461f2c", accentBorder: "#844453",
  },
};

// Lucide (ISC), on its 24 grid, drawn at stroke 1.75.
export const ICONS = {
  star: '<path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"/>',
  package: '<path d="M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z"/><path d="M12 22V12"/><polyline points="3.29 7 12 12 20.71 7"/><path d="m7.5 4.27 9 5.15"/>',
  bot: '<path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>',
  monitor: '<rect width="20" height="14" x="2" y="3" rx="2"/><line x1="8" x2="16" y1="21" y2="21"/><line x1="12" x2="12" y1="17" y2="21"/>',
  zap: '<path d="M15.914 4a1.5 1.5 0 00-2.474-1.561l-9 9A1.5 1.5 0 005.5 14h4.002a.5.5 0 01.471.666L8.086 20a1.5 1.5 0 002.475 1.56l9-9A1.5 1.5 0 0018.5 10h-3.997a.5.5 0 01-.472-.667z"/>',
  gauge: '<path d="m12 14 4-4"/><path d="M3.34 19a10 10 0 1 1 17.32 0"/>',
  search: '<path d="m21 21-4.34-4.34"/><circle cx="11" cy="11" r="8"/>',
  "book-open": '<path d="M12 5v16"/><path d="M20.001 19A2 2 0 0022 17V5a2 2 0 00-1.999-2L16 3.002A5 5 0 0012 5a5 5 0 00-4-2H4a2 2 0 00-2 2v12a2 2 0 001.999 2H8a5 5 0 014 2 5 5 0 014-2z"/>',
};

export const icon = (name, x, y, size, color) =>
  `<svg x="${x}" y="${y}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="${color}" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">${ICONS[name]}</svg>`;

export const svg = (w, h, label, body) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" role="img" aria-label="${label}"><title>${label}</title>${body}</svg>\n`;

// A pill, as the README preview draws it: 28 high with a 1px border and radius 6; the key (icon and
// label) on chrome, then each value on its own ground, split by 1px rules in the border colour.
// label and each value carry `body` (paths at x = 0, already on the pill's baseline) and `w`.
// A value's kind: "soft" (accent ground), "ok" (card ground), "warn" (card ground, warning text).
// `id` names its clip path, unique per variant when several share one file.
export function pillBody(theme, iconName, label, vals, id = "i") {
  const t = T[theme];
  const keyW = 8 + 15 + 6 + label.w + 9;
  const w = 1 + keyW + vals.reduce((s, v) => s + 1 + 9 + v.w + 9, 0) + 1;
  const W = Math.ceil(w);
  let x = 1 + keyW + 1;
  let grounds = "", texts = "";
  for (const v of vals) {
    const ground = v.kind === "soft" ? t.accentSoft : t.card;
    const ink = v.kind === "soft" ? t.accentText : v.kind === "warn" ? t.warning : t.text;
    grounds += `<rect x="${r(x)}" y="1" width="${r(18 + v.w)}" height="26" fill="${ground}"/>`;
    texts += `<g fill="${ink}" transform="translate(${r(x + 9)} 0)">${v.body}</g>`;
    x += 18 + v.w + 1;
  }
  // The last value's ground runs to the inner edge, so a rounded-up width leaves no seam.
  return { w: W, h: 28, body:
    `<clipPath id="${id}"><rect x="1" y="1" width="${W - 2}" height="26" rx="5"/></clipPath>` +
    `<rect width="${W}" height="28" rx="6" fill="${t.cardBorder}"/>` +
    `<g clip-path="url(#${id})"><rect x="1" y="1" width="${r(keyW)}" height="26" fill="${t.chrome}"/>${grounds}` +
    `<rect x="${r(x - 1)}" y="1" width="${r(W - x + 1)}" height="26" fill="${vals.length ? lastGround(t, vals) : t.chrome}"/></g>` +
    icon(iconName, 9, 6.5, 15, t.accent) +
    `<g fill="${t.textDim}" transform="translate(${r(1 + 8 + 15 + 6)} 0)">${label.body}</g>` + texts };
}

export const pill = (theme, iconName, label, vals, alt) => {
  const p = pillBody(theme, iconName, label, vals);
  return svg(p.w, p.h, alt, p.body);
};

// Every language's variant of one image in one file. The viewer's browser picks through
// systemLanguage: a <switch> draws its first child that matches any language the browser lists, not
// the one listed first, so a non-English language anywhere in the list wins over English, which has
// no tags and comes last as the fallback. Variants are centred on the widest and share its height.
// variants: [{ tags, w, h, body }]; defs sits outside the switch, for every variant's <use>.
export function switched(variants, alt, defs = "") {
  const W = Math.max(...variants.map(v => v.w)), H = Math.max(...variants.map(v => v.h));
  const g = v => `<g${v.tags ? ` systemLanguage="${v.tags}"` : ""}${v.w < W ? ` transform="translate(${r((W - v.w) / 2)} 0)"` : ""}>${v.body}</g>`;
  return svg(W, H, alt, `${defs}<switch>${variants.map(g).join("")}</switch>`);
}

// A value drawn from the glyph table build.ts writes (assets/badges/live.json): JetBrains Mono has
// one advance and no kerning, so a value is its glyphs side by side.
export function liveValue(table, value) {
  const body = [...value].map((c, i) => {
    if (!(c in table.glyphs)) throw new Error(`live.json has no glyph for ${JSON.stringify(c)}`);
    return `<path transform="translate(${r(i * table.adv)} 0)" d="${table.glyphs[c]}"/>`;
  }).join("");
  return { kind: "soft", w: value.length * table.adv, body };
}

const lastGround = (t, vals) => (vals.at(-1).kind === "soft" ? t.accentSoft : t.card);
export const r = n => Math.round(n * 100) / 100;
