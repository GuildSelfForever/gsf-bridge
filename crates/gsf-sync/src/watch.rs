//! Surveillance des dossiers `WTF/Account` (notify + anti-rebond).
//!
//! On surveille récursivement `WTF/Account` de chaque version : les dossiers
//! de personnages créés après le démarrage sont couverts. Seuls les
//! changements de `GuildSelfForever.lua` (et de sa `.bak`) sont remontés.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};

use crate::wow::SV_FILE_NAME;

/// Anti-rebond par défaut (WoW écrit tous les fichiers SV d'un coup).
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_secs(3);

/// Surveillance active ; s'arrête quand la valeur est libérée.
pub struct SvWatcher {
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
    pub watched: Vec<PathBuf>,
}

/// `true` pour `GuildSelfForever.lua` et `GuildSelfForever.lua.bak`.
pub fn is_relevant(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        n.eq_ignore_ascii_case(SV_FILE_NAME)
            || n.strip_suffix(".bak")
                .is_some_and(|b| b.eq_ignore_ascii_case(SV_FILE_NAME))
    })
}

/// Surveille `dirs` (récursivement) et appelle `on_change` avec les fichiers
/// de l'addon modifiés, après `debounce` sans nouvel événement. Les dossiers
/// absents sont ignorés ; erreur si aucun ne peut être surveillé.
pub fn watch<F>(dirs: &[PathBuf], debounce: Duration, on_change: F) -> Result<SvWatcher, String>
where
    F: Fn(Vec<PathBuf>) + Send + 'static,
{
    let mut debouncer = new_debouncer(debounce, None, move |res: DebounceEventResult| {
        if let Ok(events) = res {
            let mut paths: Vec<PathBuf> = events
                .iter()
                .flat_map(|e| e.event.paths.iter())
                .filter(|p| is_relevant(p))
                .cloned()
                .collect();
            paths.sort();
            paths.dedup();
            if !paths.is_empty() {
                on_change(paths);
            }
        }
    })
    .map_err(|e| e.to_string())?;
    let mut watched = Vec::new();
    let mut errors = Vec::new();
    for dir in dirs {
        match debouncer.watch(dir, RecursiveMode::Recursive) {
            Ok(()) => watched.push(dir.clone()),
            Err(e) => errors.push(format!("{} : {e}", dir.display())),
        }
    }
    if watched.is_empty() && !dirs.is_empty() {
        return Err(errors.join(" ; "));
    }
    Ok(SvWatcher {
        _debouncer: debouncer,
        watched,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn relevant_files() {
        assert!(is_relevant(Path::new(
            "a/SavedVariables/GuildSelfForever.lua"
        )));
        assert!(is_relevant(Path::new(
            "a/SavedVariables/GuildSelfForever.lua.bak"
        )));
        assert!(!is_relevant(Path::new("a/SavedVariables/Other.lua")));
        assert!(!is_relevant(Path::new(
            "a/SavedVariables/GuildSelfForever.lua.gsf-tmp"
        )));
    }

    #[test]
    fn reports_addon_file_changes_once_debounced() {
        let dir = tempfile::tempdir().unwrap();
        let accounts = dir.path().join("WTF/Account");
        std::fs::create_dir_all(&accounts).unwrap();
        let (tx, rx) = mpsc::channel();
        let w = watch(
            &[accounts.clone(), dir.path().join("absent")],
            Duration::from_millis(200),
            move |p| {
                let _ = tx.send(p);
            },
        )
        .unwrap();
        assert_eq!(w.watched, vec![accounts.clone()]);
        std::thread::sleep(Duration::from_millis(100));
        // Dossier de personnage créé après le démarrage, avec un nom non ASCII.
        let sv = accounts.join("A/Fumée-Ravine/Dävin/SavedVariables");
        std::fs::create_dir_all(&sv).unwrap();
        // Laisse au watcher récursif le temps de suivre les nouveaux dossiers.
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(sv.join("Other.lua"), b"x").unwrap();
        for i in 0..3 {
            std::fs::write(sv.join(SV_FILE_NAME), format!("X = {i}\n")).unwrap();
        }
        let got = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("événement attendu");
        assert_eq!(got, vec![sv.join(SV_FILE_NAME)]);
    }
}
