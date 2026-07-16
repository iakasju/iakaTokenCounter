// history — visualisations SVG « maison » de l'historique (D5), re-adaptees des composants
// IakaCockpit (ActivityTimeline / TreemapPanel / EconomyPanel) SANS les vendorer ni ajouter de lib
// de charting. Presentationnel pur : on recoit les series `iatc-core` (ProjectActivity /
// ProjectEconomy) et on rend du SVG/DOM. Aucun I/O, aucun MQTT. Empty-state honnete si serie vide.

import type { DayTokens, MemorySample, ProjectActivity, ProjectEconomy } from "./types";

const SVGNS = "http://www.w3.org/2000/svg";
const DAY_MS = 86_400_000;

/** Palette par projet (par index) — calque de `IakaCockpit/treemapColor`. */
const TREEMAP_HUES = [210, 150, 270, 35, 0, 190];
export function treemapColor(i: number): string {
  return `hsl(${TREEMAP_HUES[i % TREEMAP_HUES.length]} 60% 55%)`;
}

/** Humanise un nombre de tokens (1234 -> « 1,2k »). Pur. */
export function fmtTokens(n: number): string {
  if (n >= 1000) {
    const k = n / 1000;
    const s = Number.isInteger(k) ? String(k) : k.toFixed(1);
    return `${s.replace(".", ",")}k`;
  }
  return String(n);
}

function el(tag: string, cls?: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function svg(tag: string, attrs: Record<string, string | number>): SVGElement {
  const e = document.createElementNS(SVGNS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
  return e;
}

function emptyState(label: string, hint: string): HTMLElement {
  const wrap = el("div", "viz-empty");
  wrap.append(el("p", "viz-empty-title", label), el("p", "viz-empty-hint", hint));
  return wrap;
}

// ============================ Timeline tokens/jour (ref. ActivityTimeline) ============================

interface Row {
  name: string;
  color: string;
  pts: { t: number; v: number; date: string }[];
}

/** Prepare les lignes de la timeline : 1 ligne = 1 projet, bulles datables uniquement. Pur. */
export function timelineRows(activity: readonly ProjectActivity[]): Row[] {
  return activity
    .filter((p) => p.days.length > 0)
    .map((p, i) => ({
      name: p.project,
      color: treemapColor(i),
      pts: p.days.map((d: DayTokens) => ({
        t: Date.parse(d.date),
        v: d.tokens,
        date: d.date,
      })),
    }))
    .filter((r) => r.pts.every((b) => !Number.isNaN(b.t)));
}

/** Timeline scatter : 1 ligne/projet, 1 bulle/jour, rayon ∝ tokens du jour. */
export function historyTimeline(activity: readonly ProjectActivity[]): HTMLElement {
  const section = el("section", "viz viz-timeline");
  section.append(el("h2", "viz-title", "Activite tokens/jour par projet"));

  const rows = timelineRows(activity);
  if (rows.length === 0) {
    section.append(
      emptyState("Aucune activite datee", "Aucun transcript exploitable pour ce provider."),
    );
    return section;
  }

  let minT = Infinity;
  let maxT = -Infinity;
  let maxV = 1;
  for (const r of rows) {
    for (const b of r.pts) {
      minT = Math.min(minT, b.t);
      maxT = Math.max(maxT, b.t);
      maxV = Math.max(maxV, b.v);
    }
  }
  const pad = Math.max(DAY_MS, (maxT - minT) * 0.06);
  minT -= pad;
  maxT += pad;

  const W = 1000;
  const L = 150;
  const R = 24;
  const TOP = 30;
  const rowH = 30;
  const B = 16;
  const H = TOP + rows.length * rowH + B;
  const span = maxT - minT || 1;
  const x = (ts: number): number => L + ((ts - minT) / span) * (W - L - R);
  const rad = (v: number): number => 3 + Math.sqrt(v / maxV) * 10;

  const root = svg("svg", {
    class: "viz-svg",
    viewBox: `0 0 ${W} ${H}`,
    width: "100%",
    preserveAspectRatio: "xMinYMin meet",
    role: "img",
  });

  // Quadrillage temporel : jours (<= 45 j) sinon mois.
  const p2 = (n: number): string => String(n).padStart(2, "0");
  const spanDays = (maxT - minT) / DAY_MS;
  const MONTHS = ["janv", "fevr", "mars", "avr", "mai", "juin", "juil", "aout", "sept", "oct", "nov", "dec"];
  const ticks: { px: number; label: string }[] = [];
  if (spanDays <= 45) {
    const step = spanDays <= 12 ? 1 : spanDays <= 24 ? 2 : 3;
    for (let ts = Math.ceil(minT / DAY_MS) * DAY_MS; ts <= maxT; ts += step * DAY_MS) {
      const dt = new Date(ts);
      ticks.push({ px: x(ts), label: `${p2(dt.getDate())}/${p2(dt.getMonth() + 1)}` });
    }
  } else {
    const m = new Date(new Date(minT).getFullYear(), new Date(minT).getMonth(), 1);
    while (m.getTime() <= maxT) {
      if (m.getTime() >= minT) {
        ticks.push({ px: x(m.getTime()), label: `${MONTHS[m.getMonth()]} ${String(m.getFullYear()).slice(2)}` });
      }
      m.setMonth(m.getMonth() + 1);
    }
  }
  for (const tk of ticks) {
    root.append(
      svg("line", { x1: tk.px.toFixed(1), y1: TOP - 6, x2: tk.px.toFixed(1), y2: H - B, class: "viz-grid" }),
    );
    const tx = svg("text", { x: (tk.px + 4).toFixed(1), y: TOP - 10, class: "viz-ax" });
    tx.textContent = tk.label;
    root.append(tx);
  }

  rows.forEach((r, i) => {
    const y = TOP + i * rowH + rowH / 2;
    const lab = svg("text", { x: L - 10, y: y + 3, class: "viz-rowlab", "text-anchor": "end" });
    lab.textContent = r.name;
    root.append(lab);
    root.append(svg("line", { x1: L, y1: y, x2: W - R, y2: y, class: "viz-rowline" }));
    for (const b of r.pts) {
      const c = svg("circle", {
        cx: x(b.t).toFixed(1),
        cy: y,
        r: rad(b.v).toFixed(1),
        fill: r.color,
        "fill-opacity": 0.8,
        stroke: r.color,
      });
      const title = document.createElementNS(SVGNS, "title");
      title.textContent = `${r.name} · ${b.date} · ${fmtTokens(b.v)} tokens`;
      c.append(title);
      root.append(c);
    }
  });

  const scroll = el("div", "viz-scroll");
  scroll.append(root);
  section.append(scroll);
  return section;
}

// ============================ Treemap par projet (ref. TreemapPanel) ============================

/** Treemap : une tuile par projet, largeur ∝ tokens totaux, pilule coord/sub. */
export function historyTreemap(economy: readonly ProjectEconomy[]): HTMLElement {
  const section = el("section", "viz viz-treemap");
  const items = economy.filter((e) => e.input + e.output > 0);
  const total = items.reduce((s, it) => s + it.input + it.output, 0);
  const head = el("h2", "viz-title", "Tokens par projet");
  if (total > 0) head.append(el("span", "viz-title-sub", ` · ${fmtTokens(total)}`));
  section.append(head);

  if (items.length === 0) {
    section.append(emptyState("Aucun projet", "Aucun cout exploitable pour ce provider."));
    return section;
  }

  const max = items.reduce((m, it) => Math.max(m, it.input + it.output), 1);
  const tmap = el("div", "tmap");
  items.forEach((it, i) => {
    const tokens = it.input + it.output;
    const cell = el("div", "tcell");
    cell.style.width = `${34 + (tokens / max) * 30}%`;
    cell.style.background = treemapColor(i);
    cell.title = `${it.project} · ${fmtTokens(tokens)}`;
    cell.append(el("span", "tnm", it.project));
    cell.append(
      el("span", "tv", `${fmtTokens(tokens)} · ${Math.round((tokens / total) * 100)}%`),
    );
    // Pilule coord/sub (part de la SORTIE par agent).
    const segTotal = it.coord + it.sub || 1;
    const seg = el("span", "tseg");
    for (const [j, part] of [it.coord, it.sub].entries()) {
      const i2 = el("i");
      i2.style.width = `${(part / segTotal) * 100}%`;
      i2.style.background = `color-mix(in srgb, #fff ${Math.round((0.9 - j * 0.35) * 100)}%, transparent)`;
      seg.append(i2);
    }
    cell.append(seg);
    tmap.append(cell);
  });
  section.append(tmap);

  const legend = el("div", "viz-legend");
  items.forEach((it, i) => {
    const s = el("span");
    const dot = el("i");
    dot.style.background = treemapColor(i);
    s.append(dot, document.createTextNode(it.project));
    legend.append(s);
  });
  section.append(legend);
  return section;
}

// ============================ Split coordinateur / sous-agent (ref. EconomyPanel) ============================

/** Agrege les totaux du split coord/sub sur tous les projets. Pur. */
export function splitTotals(economy: readonly ProjectEconomy[]): {
  input: number;
  output: number;
  coord: number;
  sub: number;
} {
  return economy.reduce(
    (a, e) => ({
      input: a.input + e.input,
      output: a.output + e.output,
      coord: a.coord + e.coord,
      sub: a.sub + e.sub,
    }),
    { input: 0, output: 0, coord: 0, sub: 0 },
  );
}

/** Split coordinateur vs sous-agent + totaux input/output. */
export function historySplit(economy: readonly ProjectEconomy[]): HTMLElement {
  const section = el("section", "viz viz-split");
  section.append(el("h2", "viz-title", "Coordinateur vs sous-agents"));

  const t = splitTotals(economy);
  if (t.input + t.output === 0) {
    section.append(emptyState("Aucun tour mesure", "Aucune sortie exploitable pour ce provider."));
    return section;
  }

  const stats = el("div", "split-stats");
  stats.append(statBox(fmtTokens(t.output), "sortie"), statBox(fmtTokens(t.input), "entree"));
  section.append(stats);

  const outTotal = t.coord + t.sub || 1;
  const bar = el("div", "split-bar");
  const coord = el("div", "split-seg split-coord");
  coord.style.width = `${(t.coord / outTotal) * 100}%`;
  const sub = el("div", "split-seg split-sub");
  sub.style.width = `${(t.sub / outTotal) * 100}%`;
  bar.append(coord, sub);
  section.append(bar);

  const legend = el("div", "split-legend");
  legend.append(
    legendItem("split-coord", `Coordinateur ${fmtTokens(t.coord)}`),
    legendItem("split-sub", `Delegues ${fmtTokens(t.sub)}`),
  );
  section.append(legend);
  return section;
}

function statBox(value: string, label: string): HTMLElement {
  const box = el("div", "split-stat");
  box.append(el("b", undefined, value), document.createTextNode(` ${label}`));
  return box;
}

// ============================ Moniteur memoire (line chart % RAM) ============================

/** Formate des octets en Go decimaux (« 9,8 Go »). Pur. */
export function fmtGb(bytes: number): string {
  return `${(bytes / 1e9).toFixed(1).replace(".", ",")} Go`;
}

/** `hh:mm` local d'un epoch en secondes. Pur. */
function fmtHm(epochSecs: number): string {
  const d = new Date(epochSecs * 1000);
  const p2 = (n: number): string => String(n).padStart(2, "0");
  return `${p2(d.getHours())}:${p2(d.getMinutes())}`;
}

/**
 * Line chart SVG « maison » du % RAM (used/total) dans le temps. Axe Y **fixe 0–100 %** (borne
 * stable, comparable entre postes), axe X = temps de la fenetre (labels `hh:mm`). Readout courant
 * `% · Go` + tooltips `<title>` par point. Empty-state si `< 2` points. Pur (aucun I/O).
 */
export function memoryChart(samples: readonly MemorySample[]): HTMLElement {
  const section = el("section", "viz viz-memory");

  if (samples.length < 2) {
    section.append(
      emptyState(
        "Aucun echantillon memoire pour l'instant",
        "La courbe apparaitra apres quelques mesures (une toutes les 60 s).",
      ),
    );
    return section;
  }

  const pct = (s: MemorySample): number =>
    s.totalBytes > 0 ? (s.usedBytes / s.totalBytes) * 100 : 0;

  // Readout courant (dernier point) : « 62 % · 9,8 / 16 Go ».
  const last = samples[samples.length - 1];
  section.append(
    el(
      "div",
      "viz-mem-readout",
      `${Math.round(pct(last))} % · ${fmtGb(last.usedBytes)} / ${fmtGb(last.totalBytes)}`,
    ),
  );

  const W = 1000;
  const L = 44;
  const R = 16;
  const TOP = 12;
  const BOT = 26;
  const H = 220;
  const plotH = H - TOP - BOT;
  const minT = samples[0].t;
  const maxT = samples[samples.length - 1].t;
  const span = maxT - minT || 1;
  const x = (t: number): number => L + ((t - minT) / span) * (W - L - R);
  const y = (p: number): number => TOP + (1 - p / 100) * plotH;

  const root = svg("svg", {
    class: "viz-svg",
    viewBox: `0 0 ${W} ${H}`,
    width: "100%",
    preserveAspectRatio: "xMinYMin meet",
    role: "img",
  });

  // Quadrillage horizontal + labels d'axe Y (0 / 50 / 100 %).
  for (const g of [0, 50, 100]) {
    const gy = y(g);
    root.append(
      svg("line", { x1: L, y1: gy.toFixed(1), x2: W - R, y2: gy.toFixed(1), class: "viz-grid" }),
    );
    const tx = svg("text", { x: L - 6, y: (gy + 3).toFixed(1), class: "viz-ax", "text-anchor": "end" });
    tx.textContent = `${g}%`;
    root.append(tx);
  }

  // Labels d'axe X (temps) : debut / milieu / fin de la fenetre.
  for (const frac of [0, 0.5, 1]) {
    const t = minT + span * frac;
    const anchor = frac === 0 ? "start" : frac === 1 ? "end" : "middle";
    const tx = svg("text", { x: x(t).toFixed(1), y: H - 8, class: "viz-ax", "text-anchor": anchor });
    tx.textContent = fmtHm(t);
    root.append(tx);
  }

  // Polyligne du %.
  const points = samples.map((s) => `${x(s.t).toFixed(1)},${y(pct(s)).toFixed(1)}`).join(" ");
  root.append(svg("polyline", { points, class: "viz-memline", fill: "none" }));

  // Points + tooltips (`hh:mm` · `62 %` · `9,8 Go`).
  for (const s of samples) {
    const c = svg("circle", {
      cx: x(s.t).toFixed(1),
      cy: y(pct(s)).toFixed(1),
      r: 2,
      class: "viz-mempt",
    });
    const title = document.createElementNS(SVGNS, "title");
    title.textContent = `${fmtHm(s.t)} · ${Math.round(pct(s))} % · ${fmtGb(s.usedBytes)}`;
    c.append(title);
    root.append(c);
  }

  section.append(root);
  return section;
}

function legendItem(cls: string, text: string): HTMLElement {
  const s = el("span", `split-legitem ${cls}`);
  s.append(el("i"), document.createTextNode(text));
  return s;
}
