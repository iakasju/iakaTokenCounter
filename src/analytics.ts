// Vue analytics (feature-app-analytics) : historique de consommation d'un provider + quota courant
// du compte double-clique en tete. La webview reste un pur consommateur du backend :
//  - quota courant (5h/7d) du compte via la commande `get_reservoirs` (etat MQTT retained),
//  - historique *all-time* par provider via la commande `get_history` (relecture disque iatc-core).
// SOURCE HONNETE (D4) : l'historique est par PROVIDER (les logs ne portent pas l'ID de compte) ; le
// quota en tete est bien celui du compte. Rafraichissement a l'ouverture + bouton (pas de polling).

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { historySplit, historyTimeline, historyTreemap, memoryChart } from "./history";
import { FRESHNESS_5H, FRESHNESS_7D, gauge } from "./render";
import type { HistoryPayload, MemorySample, ReservoirCard, StateSnapshot } from "./types";

/** Fenetre d'affichage de la courbe memoire : 24 h glissantes (aligne sur la retention backend). */
const MEMORY_RETENTION_SECS = 86_400;

/** Buffer de travail local (copie non persistee) alimente par `get_memory_history` + `tray://memory`. */
let memBuffer: MemorySample[] = [];

const params = new URLSearchParams(window.location.search);
const provider = params.get("provider") ?? "";
const account = params.get("account") ?? "";

/** Libelle humain du provider. */
function providerLabel(p: string): string {
  switch (p) {
    case "claude":
      return "Claude Code";
    case "codex":
      return "Codex";
    default:
      return p || "provider inconnu";
  }
}

function setText(id: string, text: string): void {
  const e = document.getElementById(id);
  if (e) e.textContent = text;
}

/** En-tete : quota courant 5h/7d du compte (provider, account), reutilisant la jauge du tray. */
function renderQuota(snap: StateSnapshot): void {
  const host = document.getElementById("quota");
  if (!host) return;
  host.replaceChildren();
  const card: ReservoirCard | undefined = snap.reservoirs.find(
    (r) => r.provider === provider && r.account === account,
  );
  if (!card) {
    const p = document.createElement("p");
    p.className = "viz-empty-hint";
    p.textContent = snap.brokerConnected
      ? "Aucun quota publie pour ce compte (le daemon ne l'a pas encore mesure)."
      : "Broker deconnecte — quota courant indisponible.";
    host.appendChild(p);
    return;
  }
  const gauges = document.createElement("div");
  gauges.className = "gauges";
  gauges.append(gauge("5 h", card.fiveH, FRESHNESS_5H), gauge("7 j", card.sevenD, FRESHNESS_7D));
  host.appendChild(gauges);
}

/** Corps : les trois visualisations d'historique (timeline, treemap, split). */
function renderHistory(h: HistoryPayload): void {
  const host = document.getElementById("history");
  if (!host) return;
  host.replaceChildren(
    historyTimeline(h.activity),
    historyTreemap(h.economy),
    historySplit(h.economy),
  );
}

/** Section memoire : (re)rend la courbe RAM depuis le buffer local. */
function renderMemory(): void {
  const host = document.getElementById("memory");
  if (!host) return;
  host.replaceChildren(memoryChart(memBuffer));
}

let loading = false;

/** (Re)charge quota + historique depuis le backend. Defensif : une erreur n'ecrase pas la vue. */
async function refresh(): Promise<void> {
  if (loading) return;
  loading = true;
  const btn = document.getElementById("refresh") as HTMLButtonElement | null;
  if (btn) btn.disabled = true;
  try {
    // Defensif : une erreur de l'historique memoire (metrique orthogonale) ne doit pas ecraser
    // le quota ni l'historique tokens -> on la degrade en serie vide.
    const [snap, hist, mem] = await Promise.all([
      invoke<StateSnapshot>("get_reservoirs"),
      invoke<HistoryPayload>("get_history", { provider }),
      invoke<MemorySample[]>("get_memory_history").catch(() => [] as MemorySample[]),
    ]);
    renderQuota(snap);
    renderHistory(hist);
    memBuffer = mem;
    renderMemory();
    setText("updated", `mis a jour : ${new Date().toLocaleTimeString("fr-FR")}`);
  } catch (e) {
    console.error("chargement analytics echoue", e);
    setText("updated", "echec du chargement (voir la console)");
  } finally {
    loading = false;
    if (btn) btn.disabled = false;
  }
}

// En-tete statique (titre + bandeau de portee D4).
setText("title", `Historique — ${providerLabel(provider)}`);
setText(
  "scope",
  `Historique par provider : tous les comptes ${providerLabel(provider)} de ce poste ` +
    `(les logs ne portent pas l'ID de compte — limitation « account_ambiguous »). ` +
    `Le quota en tete est bien celui du compte « ${account} ».`,
);

document.getElementById("refresh")?.addEventListener("click", () => void refresh());

// Croissance live de la courbe memoire sans polling : on ecoute l'evenement pousse par le sampler
// backend a chaque nouvel echantillon (~60 s), on trim la fenetre 24 h et on re-rend. Dedup par `t`
// (evite un doublon a la frontiere chargement initial / premier evenement). Defensif : une erreur
// d'ecoute n'ecrase pas la vue.
let unlistenMem: UnlistenFn | null = null;
async function subscribeMemory(): Promise<void> {
  try {
    unlistenMem = await listen<MemorySample>("tray://memory", (ev) => {
      const lastT = memBuffer.length ? memBuffer[memBuffer.length - 1].t : Number.NEGATIVE_INFINITY;
      if (ev.payload.t <= lastT) return;
      const cutoff = Math.floor(Date.now() / 1000) - MEMORY_RETENTION_SECS;
      memBuffer = [...memBuffer, ev.payload].filter((s) => s.t >= cutoff);
      renderMemory();
    });
  } catch (e) {
    console.error("ecoute tray://memory echouee", e);
  }
}
window.addEventListener("beforeunload", () => {
  if (unlistenMem) unlistenMem();
});

void subscribeMemory();
void refresh();
