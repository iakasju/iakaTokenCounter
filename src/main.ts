// Point d'entree de la popover. La webview est un pur consommateur :
//  - snapshot initial via la commande `get_reservoirs`,
//  - mises a jour via l'evenement `tray://state`,
//  - double-clic sur une carte -> commande `open_analytics` (hook stub, D6).
// Un intervalle rafraichit les comptes a rebours / marquages « perime » sans nouveau message.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { render, renderAgents, setAnalyticsHandler } from "./render";
import type { AgentsSnapshot, StateSnapshot } from "./types";

let last: StateSnapshot | null = null;

async function refreshSnapshot(): Promise<void> {
  try {
    last = await invoke<StateSnapshot>("get_reservoirs");
    render(last);
  } catch (e) {
    console.error("get_reservoirs a echoue", e);
  }
}

// Agents Claude Code en cours (feature-agents-en-cours.md) : flux independant du quota (D8),
// snapshot initial + evenement dedie `tray://agents`.
async function refreshAgents(): Promise<void> {
  try {
    renderAgents(await invoke<AgentsSnapshot>("get_running_agents"));
  } catch (e) {
    console.error("get_running_agents a echoue", e);
  }
}

setAnalyticsHandler((provider, account) => {
  invoke("open_analytics", { provider, account }).catch((e) =>
    console.error("open_analytics a echoue", e),
  );
});

// Mises a jour poussees par le backend Rust.
listen<StateSnapshot>("tray://state", (event) => {
  last = event.payload;
  render(last);
}).catch((e) => console.error("listen tray://state a echoue", e));

listen<AgentsSnapshot>("tray://agents", (event) => {
  renderAgents(event.payload);
}).catch((e) => console.error("listen tray://agents a echoue", e));

// Re-rendu periodique pour les compte a rebours et l'expiration (fraicheur) sans nouveau message.
setInterval(() => {
  if (last) render(last);
}, 1000);

void refreshSnapshot();
void refreshAgents();
