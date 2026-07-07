// Types du contrat de rendu entre le backend Rust (etat MQTT agrege) et la webview.
// Le backend serialise en camelCase (serde rename_all). La webview ne touche jamais MQTT :
// elle recoit un instantane via la commande `get_reservoirs` et les mises a jour via
// l'evenement `tray://state`.

/** Niveau de confiance d'une valeur, mappe sur les codes du contrat (§ 3.3). */
export type Confidence =
  | "official"
  | "official_stale"
  | "local_estimate"
  | "none";

/** Etat d'une fenetre de quota (5h ou 7d) pour un compte. */
export interface WindowState {
  usedPct: number | null;
  remainingPct: number | null;
  usedTokens: number | null;
  resetsAt: number | null;
  capturedAt: number | null;
  confidence: Confidence | null;
  source: string | null;
  /** Epoch s de la valeur la plus fraiche recue pour cette fenetre (fraicheur locale). */
  updatedAt: number | null;
}

/** Une carte de reservoir = un compte IA (provider + account) avec ses deux fenetres. */
export interface ReservoirCard {
  provider: string;
  account: string;
  fiveH: WindowState;
  sevenD: WindowState;
}

/** Le pire reservoir (plus petit remaining_pct) — sert au tooltip du tray. */
export interface Worst {
  label: string;
  remainingPct: number;
}

/** Instantane complet pousse a la webview. */
export interface StateSnapshot {
  reservoirs: ReservoirCard[];
  brokerConnected: boolean;
  daemonAvailable: boolean;
  worst: Worst | null;
}

// ---- Historique (vue analytics, commande `get_history`) ----
// Miroir des types Rust `iatc-core` : ProjectActivity / DayTokens / ProjectEconomy. Series
// *all-time* relues du disque, ventilees par projet et coord/sub, a l'echelle du PROVIDER (D4).

/** Tokens d'un jour pour un projet (miroir `DayTokens`). */
export interface DayTokens {
  date: string;
  tokens: number;
}

/** Serie d'activite d'un projet, jours tries croissants (miroir `ProjectActivity`). */
export interface ProjectActivity {
  project: string;
  days: DayTokens[];
}

/** Cout d'un projet + split coordinateur/sous-agent (miroir `ProjectEconomy`). */
export interface ProjectEconomy {
  project: string;
  input: number;
  output: number;
  /** Tokens de sortie du coordinateur (tours non-sidechain). */
  coord: number;
  /** Tokens de sortie des sous-agents delegues (sidechain ; Codex = 0). */
  sub: number;
}

/** Charge utile de `get_history(provider)`. */
export interface HistoryPayload {
  activity: ProjectActivity[];
  economy: ProjectEconomy[];
}
