// Vue analytics (feature-app-analytics) : historique de consommation d'un provider + quota courant
// du compte double-clique en tete. La webview reste un pur consommateur du backend :
//  - quota courant (5h/7d) du compte via la commande `get_reservoirs` (etat MQTT retained),
//  - historique *all-time* par provider via la commande `get_history` (relecture disque iatc-core).
// SOURCE HONNETE (D4) : l'historique est par PROVIDER (les logs ne portent pas l'ID de compte) ; le
// quota en tete est bien celui du compte. Rafraichissement a l'ouverture + bouton (pas de polling).

import { invoke } from "@tauri-apps/api/core";
import { historySplit, historyTimeline, historyTreemap } from "./history";
import { FRESHNESS_5H, FRESHNESS_7D, gauge } from "./render";
import type { HistoryPayload, ReservoirCard, StateSnapshot } from "./types";

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

let loading = false;

/** (Re)charge quota + historique depuis le backend. Defensif : une erreur n'ecrase pas la vue. */
async function refresh(): Promise<void> {
  if (loading) return;
  loading = true;
  const btn = document.getElementById("refresh") as HTMLButtonElement | null;
  if (btn) btn.disabled = true;
  try {
    const [snap, hist] = await Promise.all([
      invoke<StateSnapshot>("get_reservoirs"),
      invoke<HistoryPayload>("get_history", { provider }),
    ]);
    renderQuota(snap);
    renderHistory(hist);
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

void refresh();
