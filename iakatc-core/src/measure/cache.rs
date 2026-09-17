//! measure::cache — memo par fichier `(mtime, taille)` pour le scan mesure Claude (D5).
//!
//! Le daemon re-scanne l'integralite des logs Claude a CHAQUE tick (~60 s), en continu
//! (`iakatc-daemon/src/main.rs::tick`). Le lot L0 (`specs/instructions/feature-verite-des-chiffres.md`)
//! multiplie le volume scanne par ~5,7 (marche recursive sous `subagents/`, D1) : de 122,4 Mo a
//! 701,2 Mo par scan. Sans mitigation, ce serait de l'ordre de 40 Go d'I/O par heure en continu sur
//! le poste du decideur, pour une application de tray censee etre discrete.
//!
//! Ce module memoire le RESULTAT PARTIEL de chaque fichier, invalide sur `(mtime, taille)` : un
//! fichier inchange d'un tick a l'autre reutilise son resultat sans etre relu ni re-parse. Seule
//! la session en cours (et tout fichier effectivement modifie) est relue.
//!
//! ## Pourquoi c'est correct (et pas un bricolage)
//!
//! Cette memoisation n'est valide QUE parce que :
//! 1. les accumulateurs de mesure sont **purement additifs** (`(projet, agent) -> Tokens`) : le
//!    resultat global est la somme des resultats par fichier, quel que soit l'ordre ;
//! 2. la deduplication de messages (D2, `claude::fold_file_measure`) est **scopee au fichier** —
//!    aucun `message.id` n'est partage entre deux fichiers (recouvrement mesure nul sur les
//!    donnees reelles). Un resultat par fichier est donc calculable independamment et reste valide
//!    tant que le fichier ne change pas.
//!
//! Si la deduplication venait a changer de perimetre (ex. deduplication inter-fichiers), ce memo
//! cesserait d'etre valide — ce serait a signaler, pas a faire silencieusement.
//!
//! ## Ce que ce memo n'est PAS
//!
//! - **Pas un index persistant sur disque** : il vit EN MEMOIRE, le temps du process daemon. Il
//!   n'a pas besoin de survivre a un redemarrage (ecarte par D5 : plus cher a invalider/migrer/
//!   reparer, pour un cout qu'on veut juste eviter de REPETER, pas un cout initial).
//! - **Pas une approximation** : le daemon continue de recalculer depuis le disque a chaque tick
//!   (juste moins de fichiers relus). Les totaux publies restent EXACTS, jamais estimes.
//! - **Pas etendu a la GUI** : perimetre de ce lot = le daemon uniquement (`scan_claude_measurements`).
//!   La fenetre analytics (ouverture a la demande) reste sur la variante non memoisee.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::claude::{claude_transcript_files, finalize_measurements, fold_file_measure, MeasAcc};
use super::Measurement;

/// Signature d'invalidation d'un fichier : `(derniere modification, taille en octets)`. Lue
/// depuis les seules METADONNEES (aucun contenu lu) avant de decider si le fichier doit etre relu.
type FileSignature = (SystemTime, u64);

/// Resultat partiel memorise d'un fichier : sa signature au moment du calcul + son accumulateur
/// de mesure (deja deduplique par `message.id`, D2).
struct CachedFile {
    signature: FileSignature,
    partial: MeasAcc,
}

/// Memo par fichier (D5) : cle = chemin absolu du transcript, valeur = signature + resultat
/// partiel. Vit EN MEMOIRE le temps du process ; `Default`/`new()` = memo vide (etat au demarrage
/// du daemon, ou apres un redemarrage — le premier scan qui suit est alors un scan complet).
#[derive(Default)]
pub struct ScanCache {
    files: HashMap<PathBuf, CachedFile>,
}

impl ScanCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Nombre de fichiers actuellement memorises (diagnostic/tests).
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Variante MEMOISEE de `claude::scan_claude_measurements` (D5) : ne relit + re-parse que les
/// fichiers dont `(mtime, taille)` a change depuis le dernier appel avec ce `cache`. Les fichiers
/// disparus depuis le dernier scan sont purges du memo (pas de fuite memoire sur rotation/purge
/// des logs a 30 j). Defensif : fichier dont les metadonnees ou le contenu sont illisibles ->
/// aucune contribution (ni cache, ni total), jamais de panique. Le total reste la somme EXACTE
/// des resultats par fichier (cf. doc de module).
pub fn scan_claude_measurements_cached(
    projects_dir: &Path,
    cache: &mut ScanCache,
) -> Vec<Measurement> {
    let mut total: MeasAcc = HashMap::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for path in claude_transcript_files(projects_dir) {
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let signature: FileSignature = (
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        );
        seen.insert(path.clone());

        let up_to_date = cache
            .files
            .get(&path)
            .is_some_and(|c| c.signature == signature);

        if !up_to_date {
            let mut partial: MeasAcc = HashMap::new();
            if let Ok(content) = std::fs::read_to_string(&path) {
                fold_file_measure(&mut partial, &content);
            }
            cache.files.insert(
                path.clone(),
                CachedFile {
                    signature,
                    partial,
                },
            );
        }

        if let Some(entry) = cache.files.get(&path) {
            for (key, tokens) in &entry.partial {
                total.entry(key.clone()).or_default().add(tokens);
            }
        }
    }

    // Purge les fichiers disparus depuis le dernier scan (rotation/purge des logs) : le memo ne
    // grossit pas sans borne au fil des redemarrages de session Claude Code.
    cache.files.retain(|p, _| seen.contains(p));

    finalize_measurements(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::Agent;
    use std::io::Write;

    fn write_transcript(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        // Toujours terminer par un `\n` : un `append` ulterieur (test de modification) doit
        // demarrer sa propre ligne JSONL, pas se souder a la fin de la precedente.
        f.write_all(content.as_bytes()).unwrap();
        f.write_all(b"\n").unwrap();
        path
    }

    #[test]
    fn scan_cache_reutilise_un_fichier_inchange() {
        let tmp = tempdir();
        let sess = tmp.join("-w-p");
        std::fs::create_dir_all(&sess).unwrap();
        write_transcript(
            &sess,
            "s.jsonl",
            r#"{"type":"assistant","cwd":"/w/p","message":{"usage":{"input_tokens":10,"output_tokens":5}}}"#,
        );

        let mut cache = ScanCache::new();
        let first = scan_claude_measurements_cached(&tmp, &mut cache);
        assert_eq!(cache.len(), 1);
        let used_first = first
            .iter()
            .find(|m| m.project == "p" && m.agent == Agent::Coordinator)
            .map(|m| m.tokens.used());
        assert_eq!(used_first, Some(15));

        // Second scan SANS toucher au fichier : meme resultat, memo toujours a 1 entree (aucune
        // relecture necessaire, mais on ne peut observer ca depuis l'exterieur qu'au resultat).
        let second = scan_claude_measurements_cached(&tmp, &mut cache);
        assert_eq!(first, second);
        assert_eq!(cache.len(), 1);

        cleanup(&tmp);
    }

    #[test]
    fn scan_cache_detecte_un_fichier_modifie() {
        let tmp = tempdir();
        let sess = tmp.join("-w-q");
        std::fs::create_dir_all(&sess).unwrap();
        let path = write_transcript(
            &sess,
            "s.jsonl",
            r#"{"type":"assistant","cwd":"/w/q","message":{"usage":{"input_tokens":10,"output_tokens":5}}}"#,
        );

        let mut cache = ScanCache::new();
        let first = scan_claude_measurements_cached(&tmp, &mut cache);
        let used_first = first
            .iter()
            .find(|m| m.project == "q")
            .map(|m| m.tokens.used());
        assert_eq!(used_first, Some(15));

        // Le fichier GROSSIT (nouveau tour) : la signature (taille) change -> doit etre relu.
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","cwd":"/w/q","message":{{"usage":{{"input_tokens":100,"output_tokens":50}}}}}}"#
        )
        .unwrap();

        let second = scan_claude_measurements_cached(&tmp, &mut cache);
        let used_second = second
            .iter()
            .find(|m| m.project == "q")
            .map(|m| m.tokens.used());
        assert_eq!(used_second, Some(165)); // 15 + 150, PAS reste bloque a 15

        cleanup(&tmp);
    }

    #[test]
    fn scan_cache_equivaut_au_scan_non_memoise_sur_les_fixtures_reelles() {
        use crate::measure::claude::scan_claude_measurements;
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../specs/mock/claude_projects");
        let mut cache = ScanCache::new();
        let cached = scan_claude_measurements_cached(&dir, &mut cache);
        let uncached = scan_claude_measurements(&dir);
        assert_eq!(cached, uncached, "le memo ne doit JAMAIS changer le total publie");
    }

    #[test]
    fn scan_cache_purge_les_fichiers_disparus() {
        let tmp = tempdir();
        let sess = tmp.join("-w-r");
        std::fs::create_dir_all(&sess).unwrap();
        let path = write_transcript(
            &sess,
            "s.jsonl",
            r#"{"type":"assistant","cwd":"/w/r","message":{"usage":{"input_tokens":1,"output_tokens":1}}}"#,
        );
        let mut cache = ScanCache::new();
        scan_claude_measurements_cached(&tmp, &mut cache);
        assert_eq!(cache.len(), 1);

        std::fs::remove_file(&path).unwrap();
        let after = scan_claude_measurements_cached(&tmp, &mut cache);
        assert!(after.is_empty());
        assert!(cache.is_empty(), "le memo doit purger les fichiers disparus");

        cleanup(&tmp);
    }

    // ---- Petit dossier temporaire jetable (pas de dependance a un crate de test supplementaire) ----

    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        let unique = format!(
            "iakatc-scan-cache-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        p.push(unique);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn cleanup(p: &Path) {
        let _ = std::fs::remove_dir_all(p);
    }
}
