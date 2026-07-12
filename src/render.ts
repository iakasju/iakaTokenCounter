// Rendu des cartes de reservoir dans la popover (D1) : hypothese 1 « barres horizontales »
// (fuel bars). Par compte, une carte ; par fenetre (5h / 7j), une barre horizontale dont la
// largeur = % restant, teintee par niveau, avec compte a rebours (`resetsAt`) + badge de
// confiance. Repli 1 barre si le compte n'a qu'une fenetre. Aucune logique MQTT ici.
//
// Reference design : docs/design/popover-reservoir-hypotheses.html (H1). Les teintes suivent la
// meme palette carburant que l'icone de tray (ok ≥50 / moyen 20–49 / alerte <20).

import type {
  Confidence,
  ReservoirCard,
  StateSnapshot,
  WindowState,
} from "./types";

// Seuils de fraicheur locaux (s) : au-dela, la derniere valeur connue est marquee « perimee ».
// Alignes sur les defauts du daemon (config.json : 1200 / 21600). Exportes pour la vue analytics
// qui reutilise la meme barre en tete (quota courant du compte).
export const FRESHNESS_5H = 1200;
export const FRESHNESS_7D = 21600;

let onOpenAnalytics: (provider: string, account: string) => void = () => {};

/** Enregistre le callback declenche par le double-clic sur une carte (hook analytics D6). */
export function setAnalyticsHandler(
  fn: (provider: string, account: string) => void,
): void {
  onOpenAnalytics = fn;
}

function nowS(): number {
  return Math.floor(Date.now() / 1000);
}

/** Une fenetre a recu de la donnee des qu'elle a un horodatage de fraicheur. Sert au repli. */
function hasWindow(w: WindowState): boolean {
  return w.updatedAt !== null;
}

/** Une fenetre porte une **vraie jauge de quota** (donc une barre a rendre) ssi elle a un `remainingPct`. */
export function shouldRenderGauge(w: WindowState): boolean {
  return w.remainingPct !== null;
}

/**
 * Conso portee par la carte : `usedTokens` maximal non-null parmi ses fenetres. Pour la branche 4
 * (mesure sans quota) toutes les fenetres portent la meme valeur ; ailleurs on garde la plus grande.
 */
export function cardUsedTokens(r: ReservoirCard): number | null {
  let max: number | null = null;
  for (const w of [r.fiveH, r.sevenD]) {
    if (w.usedTokens !== null && (max === null || w.usedTokens > max)) {
      max = w.usedTokens;
    }
  }
  return max;
}

/** Formate un nombre de tokens de facon compacte (`26894 -> "26.9k"`, `1_500_000 -> "1.5M"`). */
export function formatTokens(n: number): string {
  if (n < 1000) return `${n}`;
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/** Une fenetre est perimee si sa derniere valeur est trop vieille ou si la recharge est passee. */
function isStale(w: WindowState, freshness: number): boolean {
  const now = nowS();
  if (w.updatedAt !== null && now - w.updatedAt > freshness) return true;
  if (w.resetsAt !== null && now > w.resetsAt) return true;
  return false;
}

/** Niveau de remplissage → classe de teinte (memes seuils que l'icone : ≥50 / 20–49 / <20). */
function levelClass(pct: number): "f-ok" | "f-mid" | "f-low" {
  if (pct >= 50) return "f-ok";
  if (pct >= 20) return "f-mid";
  return "f-low";
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

/** Style de confiance (D3.1) : classe CSS, prefixe de valeur, glyphe et libelle du badge. */
function confidenceStyle(c: Confidence | null): {
  cls: string;
  prefix: string;
  glyph: string;
  badge: string;
} {
  switch (c) {
    case "official":
      return { cls: "conf-official", prefix: "", glyph: "✓", badge: "officiel" };
    case "official_stale":
      return { cls: "conf-stale", prefix: "", glyph: "⟳", badge: "date" };
    case "local_estimate":
      return { cls: "conf-estimate", prefix: "~", glyph: "~", badge: "estime" };
    case "none":
    default:
      return { cls: "conf-none", prefix: "", glyph: "?", badge: "inconnu" };
  }
}

/**
 * Rend une **barre de reservoir horizontale** (une fenetre) : entete (fenetre + valeur a gauche,
 * countdown + badge de confiance a droite) puis la piste avec son remplissage. Reutilisee par la
 * popover ET l'en-tete de la vue analytics. Le nom `gauge` est conserve pour cette derniere.
 */
export function gauge(title: string, w: WindowState, freshness: number): HTMLElement {
  const row = document.createElement("div");
  row.className = "h1-row";

  const style = confidenceStyle(w.confidence);
  const unknown = w.remainingPct === null || w.confidence === "none";
  const stale = !unknown && isStale(w, freshness);
  const pct = w.remainingPct;
  const clamped = pct === null ? 0 : Math.max(0, Math.min(100, pct));
  const lvl = unknown || pct === null ? "f-none" : levelClass(clamped);

  // Entete : fenetre + valeur (gauche) ; recharge + confiance (droite).
  const head = document.createElement("div");
  head.className = "gh";

  const left = document.createElement("span");
  left.className = "gh-l";
  const win = document.createElement("span");
  win.className = "gh-win";
  win.textContent = title;
  const value = document.createElement("span");
  value.className = `gv ${unknown ? "gv-none" : lvl}`;
  value.textContent =
    unknown || pct === null ? "?" : `${style.prefix}${Math.round(clamped)}%`;
  left.append(win, value);

  const right = document.createElement("span");
  right.className = "gh-r";
  const cd = document.createElement("span");
  cd.className = "cd";
  cd.textContent = `↺ ${countdown(w.resetsAt)}`;
  const badge = document.createElement("span");
  badge.className = `conf ${stale ? "conf-stale" : style.cls}`;
  const glyph = document.createElement("i");
  glyph.textContent = stale ? "⟳" : style.glyph;
  badge.append(glyph, document.createTextNode(stale ? "perime" : style.badge));
  if (w.source) badge.title = `source : ${w.source}`;
  right.append(cd, badge);

  head.append(left, right);

  // Piste + remplissage. Inconnu → piste pointillee vide (jamais de faux plein).
  const track = document.createElement("div");
  track.className = `h1-track${unknown ? " none" : ""}`;
  if (!unknown) {
    const fill = document.createElement("div");
    const treat =
      w.confidence === "local_estimate"
        ? "hatch"
        : stale
          ? "attenuated"
          : "solid";
    fill.className = `h1-fill ${lvl} ${treat}`;
    fill.style.width = `${clamped}%`;
    track.appendChild(fill);
  }

  row.append(head, track);
  return row;
}

/** Rend une carte de compte : entete + jauges (fenetres a quota) + ligne de conso / empty-state. */
function card(r: ReservoirCard): HTMLElement {
  const el = document.createElement("section");
  el.className = "acard";
  el.title = "double-clic : analytics";

  // Fenetres candidates = celles ayant recu de la donnee.
  const candidates: Array<[string, WindowState, number]> = [];
  if (hasWindow(r.fiveH)) candidates.push(["5h", r.fiveH, FRESHNESS_5H]);
  if (hasWindow(r.sevenD)) candidates.push(["7j", r.sevenD, FRESHNESS_7D]);
  // Jauges = uniquement les fenetres portant un vrai quota (remainingPct non null).
  const gaugeWindows = candidates.filter(([, w]) => shouldRenderGauge(w));
  const used = cardUsedTokens(r);

  const header = document.createElement("header");
  header.className = "acard-head";
  const name = document.createElement("span");
  name.className = "acard-name";
  name.textContent = r.provider;
  const plan = document.createElement("span");
  plan.className = "plan";
  plan.textContent = `/ ${r.account}`;
  name.appendChild(plan);
  const tier = document.createElement("span");
  tier.className = "acard-tier";
  tier.textContent =
    gaugeWindows.length === 0
      ? "conso seule"
      : `${gaugeWindows.length} jauge${gaugeWindows.length > 1 ? "s" : ""}`;
  header.append(name, tier);
  el.appendChild(header);

  // Ligne de conso : rendue des qu'une fenetre porte un `usedTokens`.
  if (used !== null) {
    const conso = document.createElement("div");
    conso.className = "acard-conso";
    const lbl = document.createElement("span");
    lbl.className = "acard-conso-lbl";
    lbl.textContent = "conso";
    const val = document.createElement("span");
    val.className = "acard-conso-val";
    val.textContent = `${formatTokens(used)} tokens`;
    conso.append(lbl, val);
    el.appendChild(conso);
  }

  // Jauges de quota (aucune si le provider est mesure sans quota exploitable).
  for (const [title, w, freshness] of gaugeWindows) {
    el.appendChild(gauge(title, w, freshness));
  }

  // Empty-state honnete : aucun quota exploitable, mais la carte reste ouvrable (analytics).
  if (gaugeWindows.length === 0) {
    const nq = document.createElement("div");
    nq.className = "acard-noquota";
    nq.textContent = "pas de jauge de quota disponible";
    el.appendChild(nq);
  }

  el.addEventListener("dblclick", () => onOpenAnalytics(r.provider, r.account));
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
