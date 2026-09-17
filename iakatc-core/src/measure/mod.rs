//! measure — modele commun de mesure de conso + scanners Claude Code et Codex.
//!
//! Une [`Measurement`] = les tokens attribues a un `(project, provider, agent)`. Les scanners
//! [`claude::scan_claude_measurements`] et [`codex::scan_codex_measurements`] produisent des
//! `Vec<Measurement>` homogenes, que `aggregate` re-somme selon les deux axes du contrat.

pub mod cache;
pub mod claude;
pub mod codex;

/// Fournisseur d'IA (segment `{provider}` du contrat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provider {
    /// Claude Code (transcripts `~/.claude/projects/**/*.jsonl`).
    Claude,
    /// Codex CLI (rollouts `~/.codex/sessions/**/*.jsonl`).
    Codex,
}

impl Provider {
    /// Code stable du provider tel que publie dans les topics (`claude` / `codex`).
    pub fn code(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }
}

/// Agent mesurable (segment `{agent}` du contrat). Seule distinction disponible dans les logs :
/// coordinateur vs sous-agent delegue (`isSidechain` cote Claude ; Codex = coordinateur seul).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Agent {
    /// Tours principaux (non-sidechain).
    Coordinator,
    /// Tours de sous-agents delegues (`isSidechain:true`).
    Subagent,
}

impl Agent {
    /// Code stable de l'agent tel que publie dans les topics (`coordinator` / `subagent`).
    pub fn code(self) -> &'static str {
        match self {
            Agent::Coordinator => "coordinator",
            Agent::Subagent => "subagent",
        }
    }
}

/// Tokens d'une mesure, decomposes selon les codes de conso du contrat (§ 3.1).
///
/// - `input`  = `input + cache_creation + cache_read` (regle economy.rs).
/// - `output` = tokens de sortie.
/// - `cache`  = `cache_creation + cache_read` (diagnostic, sous-ensemble d'`input`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache: u64,
}

impl Tokens {
    /// `used_tokens` du contrat = `input_tokens + output_tokens`.
    pub fn used(&self) -> u64 {
        self.input + self.output
    }

    /// Accumulation en place (agregation).
    pub fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.output += other.output;
        self.cache += other.cache;
    }
}

/// Une mesure atomique : les tokens attribues a un `(project, provider, agent)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    pub project: String,
    pub provider: Provider,
    pub agent: Agent,
    pub tokens: Tokens,
}
