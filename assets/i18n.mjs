// The words the README's images carry, in each language they are drawn in. build.ts draws every
// language into one file per image (the viewer's browser picks, see switched() in draw.mjs) and one
// file per language for README.<id>.md. README.md and English are the reference; the others are
// translations of it.
//
// tags: the browser language tags a language answers to, as SVG systemLanguage matches them. A
// browser's bare "pt" matches "pt-BR" here, but its "pt-BR" does not match a bare "pt", so the
// common regions are listed. English has none: it is the fallback, drawn when nothing else matches,
// and it stays last.
// Hero markup: _…_ is underlined, `…` is a code chip. "prompt hook", "guard" and "knowledge find"
// are the engine's own names and stay as they are in every language.

export const LANGS = [
  {
    id: "tr", name: "Türkçe", tags: "tr,tr-TR", dec: ",",
    kicker: "KNOWLEDGE NEDEN VAR",
    rows: [
      ["Ajanlar seni _unutur_.", "Kim olduğun ve nasıl çalıştığın her oturumda önce, eksiksiz yüklenir."],
      ["Kurallarını _yok sayarlar_.", "Her tool call'da bir guard, yasakladığın şeyi reddeder: `git push` senin onayını bekler."],
      ["_Kimse_ not almaz.", "Oturumlar, commit'ler ve kararlar kendiliğinden kaydedilir."],
      ["Her aracın kendi _hafızası_ var.", "Claude Code, Codex, opencode ve Claude uygulaması tek bir vault okur."],
    ],
    foot: "Düz Markdown, gizli, senin.",
    pills: { harnesses: "ajanlar", claudeApp: "Claude uygulaması", platforms: "platformlar", pending: "Arch bekliyor", tokens: "token", lower: "grep'ten %82 düşük", stars: "yıldız", latest: "sürüm" },
  },
  {
    id: "es", name: "Español", tags: "es,es-ES,es-419,es-MX,es-AR,es-CO,es-CL,es-PE,es-VE,es-US", dec: ",",
    kicker: "POR QUÉ EXISTE KNOWLEDGE",
    rows: [
      ["Los agentes te _olvidan_.", "Quién eres y cómo trabajas se cargan primero, completos, en cada sesión."],
      ["_Ignoran_ tus reglas.", "Un guard en cada llamada a una herramienta rechaza lo que descartaste: `git push` espera tu sí."],
      ["_Nadie_ lo apunta.", "Las sesiones, los commits y las decisiones se registran solos."],
      ["Cada herramienta guarda su propia _memoria_.", "Claude Code, Codex, opencode y la app de Claude leen un solo vault."],
    ],
    foot: "Markdown simple, privado, tuyo.",
    pills: { harnesses: "agentes", claudeApp: "app de Claude", platforms: "plataformas", pending: "Arch pendiente", tokens: "tokens", lower: "82 % menos que grep", stars: "estrellas", latest: "versión" },
  },
  {
    id: "pt", name: "Português", tags: "pt,pt-BR,pt-PT", dec: ",",
    kicker: "POR QUE O KNOWLEDGE EXISTE",
    rows: [
      ["Os agentes _esquecem_ você.", "Quem você é e como você trabalha carregam primeiro, inteiros, em toda sessão."],
      ["Eles _ignoram_ suas regras.", "Um guard em cada chamada de ferramenta nega o que você descartou: `git push` espera o seu sim."],
      ["_Ninguém_ anota nada.", "Sessões, commits e decisões se registram sozinhos."],
      ["Cada ferramenta tem sua própria _memória_.", "Claude Code, Codex, opencode e o app do Claude leem um só vault."],
    ],
    foot: "Markdown puro, privado, seu.",
    pills: { harnesses: "agentes", claudeApp: "app do Claude", platforms: "plataformas", pending: "Arch pendente", tokens: "tokens", lower: "82% menos que grep", stars: "estrelas", latest: "versão" },
  },
  {
    id: "fr", name: "Français", tags: "fr,fr-FR,fr-CA,fr-BE,fr-CH", dec: ",",
    kicker: "POURQUOI KNOWLEDGE EXISTE",
    rows: [
      ["Les agents vous _oublient_.", "Qui vous êtes et votre façon de travailler se chargent d’abord, en entier, à chaque session."],
      ["Ils _ignorent_ vos règles.", "Un guard sur chaque appel d’outil refuse ce que vous avez exclu : `git push` attend votre oui."],
      ["_Personne_ ne note rien.", "Les sessions, les commits et les décisions s’enregistrent d’eux-mêmes."],
      ["Chaque outil garde sa propre _mémoire_.", "Claude Code, Codex, opencode et l’app Claude lisent un seul vault."],
    ],
    foot: "Du Markdown simple, privé, à vous.",
    pills: { harnesses: "agents", claudeApp: "app Claude", platforms: "plateformes", pending: "Arch en attente", tokens: "tokens", lower: "82 % de moins que grep", stars: "étoiles", latest: "version" },
  },
  {
    id: "de", name: "Deutsch", tags: "de,de-DE,de-AT,de-CH", dec: ",",
    kicker: "WARUM ES KNOWLEDGE GIBT",
    rows: [
      ["Agenten _vergessen_ dich.", "Wer du bist und wie du arbeitest, lädt zuerst, vollständig, in jeder Sitzung."],
      ["Sie _ignorieren_ deine Regeln.", "Ein Guard bei jedem Tool-Aufruf verweigert, was du ausgeschlossen hast: `git push` wartet auf dein Ja."],
      ["_Niemand_ schreibt es auf.", "Sitzungen, Commits und Entscheidungen zeichnen sich selbst auf."],
      ["Jedes Tool hat sein eigenes _Gedächtnis_.", "Claude Code, Codex, opencode und die Claude-App lesen einen Vault."],
    ],
    foot: "Reines Markdown, privat, deins.",
    pills: { harnesses: "Agenten", claudeApp: "Claude-App", platforms: "Plattformen", pending: "Arch ausstehend", tokens: "Tokens", lower: "82 % weniger als grep", stars: "Sterne", latest: "Version" },
  },
  {
    id: "en", name: "English", tags: "", dec: ".",
    kicker: "WHY KNOWLEDGE EXISTS",
    rows: [
      ["Agents _forget_ you.", "Who you are and how you work load first, whole, in every session."],
      ["They _ignore_ your rules.", "A guard on every tool call denies what you ruled out: `git push` waits for your yes."],
      ["_Nobody_ writes it down.", "Sessions, commits and decisions record themselves."],
      ["Each tool keeps its own _memory_.", "Claude Code, Codex, opencode and the Claude app read one vault."],
    ],
    foot: "Plain Markdown, private, yours.",
    pills: { harnesses: "harnesses", claudeApp: "Claude app", platforms: "platforms", pending: "Arch pending", tokens: "tokens", lower: "82% lower than grep", stars: "stars", latest: "latest" },
  },
];
