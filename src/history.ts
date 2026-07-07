// history — visualisations SVG « maison » de l'historique (D5), re-adaptees des composants
// IakaCockpit (ActivityTimeline / TreemapPanel / EconomyPanel) SANS les vendorer ni ajouter de lib
// de charting. Presentationnel pur : on recoit les series `iatc-core` (ProjectActivity /
// ProjectEconomy) et on rend du SVG/DOM. Aucun I/O, aucun MQTT. Empty-state honnete si serie vide.

import type { DayTokens, ProjectActivity, ProjectEconomy } from "./types";

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

function legendItem(cls: string, text: string): HTMLElement {
  const s = el("span", `split-legitem ${cls}`);
  s.append(el("i"), document.createTextNode(text));
  return s;
}
