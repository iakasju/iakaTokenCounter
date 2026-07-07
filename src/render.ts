// Rendu des cartes de reservoir dans la popover : deux jauges (5h / 7d) par compte,
// mapping confiance -> style (D3.1), compte a rebours depuis `resetsAt`, etats degrades
// (« inconnu » / « perime » / « broker deconnecte »). Aucune logique MQTT ici.

import type {
  Confidence,
  ReservoirCard,
  StateSnapshot,
  WindowState,
} from "./types";

// Seuils de fraicheur locaux (s) : au-dela, la derniere valeur connue est marquee « perimee ».
// Alignes sur les defauts du daemon (config.json : 1200 / 21600).
const FRESHNESS_5H = 1200;
const FRESHNESS_7D = 21600;

let onOpenAnalytics: (account: string) => void = () => {};

/** Enregistre le callback declenche par le double-clic sur une carte (hook analytics D6). */
export function setAnalyticsHandler(fn: (account: string) => void): void {
  onOpenAnalytics = fn;
}

function nowS(): number {
  return Math.floor(Date.now() / 1000);
}

/** Une fenetre est perimee si sa derniere valeur est trop vieille ou si la recharge est passee. */
function isStale(w: WindowState, freshness: number): boolean {
  const now = nowS();
  if (w.updatedAt !== null && now - w.updatedAt > freshness) return true;
  if (w.resetsAt !== null && now > w.resetsAt) return true;
  return false;
}

/** Compte a rebours humain depuis un epoch de recharge. */
function countdown(resetsAt: number | null): string {
  if (resetsAt === null) return "—";
  const delta = resetsAt - nowS();
  if (delta <= 0) return "recharge due";
  const h = Math.floor(delta / 3600);
  const m = Math.floor((delta % 3600) / 60);
  if (h >= 24) {
    const d = Math.floor(h / 24);
    return `~${d}j ${h % 24}h`;
  }
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

/** Style de jauge selon la confiance (D3.1). Retourne classe CSS + prefixe de valeur. */
function confidenceStyle(c: Confidence | null): {
  cls: string;
  prefix: string;
  badge: string;
} {
  switch (c) {
    case "official":
      return { cls: "conf-official", prefix: "", badge: "officiel" };
    case "official_stale":
      return { cls: "conf-stale", prefix: "", badge: "⏱ date" };
    case "local_estimate":
      return { cls: "conf-estimate", prefix: "~", badge: "estime" };
    case "none":
    default:
      return { cls: "conf-none", prefix: "", badge: "inconnu" };
  }
}

function gauge(title: string, w: WindowState, freshness: number): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "gauge";

  const style = confidenceStyle(w.confidence);
  const unknown = w.remainingPct === null || w.confidence === "none";
  const stale = !unknown && isStale(w, freshness);

  const head = document.createElement("div");
  head.className = "gauge-head";
  const label = document.createElement("span");
  label.className = "gauge-title";
  label.textContent = title;
  const badge = document.createElement("span");
  badge.className = `conf-badge ${style.cls}`;
  badge.textContent = stale ? "⏱ perime" : style.badge;
  head.append(label, badge);

  const bar = document.createElement("div");
  bar.className = `bar ${style.cls}${unknown ? " unknown" : ""}${stale ? " stale" : ""}`;
  const fill = document.createElement("div");
  fill.className = "bar-fill";
  const pct = w.remainingPct;
  fill.style.width = unknown || pct === null ? "0%" : `${Math.max(0, Math.min(100, pct))}%`;
  bar.appendChild(fill);

  const value = document.createElement("div");
  value.className = "gauge-value";
  if (unknown || pct === null) {
    value.textContent = "?";
  } else {
    value.textContent = `${style.prefix}${pct.toFixed(1)} % restant`;
  }

  const meta = document.createElement("div");
  meta.className = "gauge-meta";
  meta.textContent = `recharge : ${countdown(w.resetsAt)}`;
  if (w.source) meta.title = `source : ${w.source}`;

  wrap.append(head, bar, value, meta);
  return wrap;
}

function card(r: ReservoirCard): HTMLElement {
  const el = document.createElement("section");
  el.className = "card";
  el.title = "double-clic : analytics (a venir)";

  const header = document.createElement("header");
  header.className = "card-head";
  header.textContent = `${r.provider} / ${r.account}`;
  el.appendChild(header);

  const gauges = document.createElement("div");
  gauges.className = "gauges";
  gauges.append(
    gauge("5 h", r.fiveH, FRESHNESS_5H),
    gauge("7 j", r.sevenD, FRESHNESS_7D),
  );
  el.appendChild(gauges);

  el.addEventListener("dblclick", () => onOpenAnalytics(r.account));
  return el;
}

function banner(text: string, kind: "warn" | "error"): HTMLElement {
  const b = document.createElement("div");
  b.className = `banner banner-${kind}`;
  b.textContent = text;
  return b;
}

/** Rend l'instantane complet dans la popover. */
export function render(snap: StateSnapshot): void {
  const dot = document.getElementById("conn-dot");
  if (dot) {
    dot.className = `conn-dot ${snap.brokerConnected ? "on" : "off"}`;
    dot.title = snap.brokerConnected ? "broker connecte" : "broker deconnecte";
  }

  const banners = document.getElementById("banners");
  if (banners) {
    banners.replaceChildren();
    if (!snap.brokerConnected) {
      banners.appendChild(banner("Broker deconnecte — valeurs possiblement perimees", "error"));
    }
    if (!snap.daemonAvailable) {
      banners.appendChild(banner("Daemon indisponible — mode subscriber pur", "warn"));
    }
  }

  const root = document.getElementById("reservoirs");
  if (!root) return;
  root.replaceChildren();
  if (snap.reservoirs.length === 0) {
    const p = document.createElement("p");
    p.className = "empty";
    p.textContent = snap.brokerConnected
      ? "Aucun reservoir publie pour l'instant."
      : "En attente de donnees du broker…";
    root.appendChild(p);
    return;
  }
  for (const r of snap.reservoirs) root.appendChild(card(r));
}
