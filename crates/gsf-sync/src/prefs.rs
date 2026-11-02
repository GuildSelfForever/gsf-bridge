//! Préférences de l'application de bureau (`desktop.json`), distinctes des
//! réglages du moteur : ce qui ne doit arriver qu'une fois par installation.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsutil::{read_json, write_json};

pub const PREFS_FILE: &str = "desktop.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopPrefs {
    /// Le lancement au démarrage de Windows a déjà été activé une fois. Après,
    /// c'est le choix de l'officier (case dans « Réglages ») qui compte : s'il
    /// l'a décoché, on ne le réactive jamais.
    pub autostart_initialized: bool,
    /// La notification « GSF Bridge continue en arrière-plan » a déjà été montrée.
    pub background_hint_shown: bool,
    /// L'écran d'accueil (contrat étape 1, « Ce que fait le bridge ») a déjà
    /// été vu et conclu : le démarrage automatique n'est décidé qu'à ce
    /// moment-là, plus jamais au tout premier lancement silencieusement.
    /// Absent d'un fichier qui existe déjà (mise à jour depuis une version
    /// plus ancienne, sans cet écran) : `true` par défaut, l'écran ne doit
    /// pas resurgir pour un bridge déjà installé. Absent parce que le
    /// fichier entier n'existe pas (toute première installation) : `false`
    /// (valeur par défaut de la structure), l'écran doit s'afficher.
    #[serde(default = "default_true")]
    pub welcome_completed: bool,
}

fn default_true() -> bool {
    true
}

impl DesktopPrefs {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join(PREFS_FILE)
    }

    /// Lit les préférences ; fichier absent ou illisible = valeurs par défaut.
    pub fn load(data_dir: &Path) -> Self {
        read_json(&Self::path(data_dir))
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> io::Result<()> {
        write_json(&Self::path(data_dir), self)
    }

    /// Premier lancement : rend `true` s'il faut activer le démarrage avec
    /// Windows, et le note pour ne plus jamais le refaire.
    pub fn take_first_autostart(&mut self) -> bool {
        !std::mem::replace(&mut self.autostart_initialized, true)
    }

    /// Première fermeture de la fenêtre : rend `true` s'il faut expliquer que
    /// le bridge continue en arrière-plan, et le note.
    pub fn take_background_hint(&mut self) -> bool {
        !std::mem::replace(&mut self.background_hint_shown, true)
    }

    /// Conclut l'écran d'accueil : rend `true` la première fois (il faut
    /// alors appliquer le choix de démarrage automatique coché sur cet
    /// écran), `false` ensuite.
    pub fn complete_welcome(&mut self) -> bool {
        !std::mem::replace(&mut self.welcome_completed, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_is_enabled_only_on_first_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = DesktopPrefs::load(dir.path());
        assert!(p.take_first_autostart());
        p.save(dir.path()).unwrap();

        // Lancements suivants : on respecte le choix de l'officier.
        let mut p = DesktopPrefs::load(dir.path());
        assert!(!p.take_first_autostart());
        assert!(!p.take_first_autostart());
    }

    #[test]
    fn background_hint_once() {
        let mut p = DesktopPrefs::default();
        assert!(p.take_background_hint());
        assert!(!p.take_background_hint());
    }

    #[test]
    fn unreadable_file_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(DesktopPrefs::path(dir.path()), b"{pas du json").unwrap();
        assert_eq!(DesktopPrefs::load(dir.path()), DesktopPrefs::default());

        // Champ inconnu ou manquant : toléré.
        std::fs::write(
            DesktopPrefs::path(dir.path()),
            br#"{"autostart_initialized": true, "futur": 1}"#,
        )
        .unwrap();
        let p = DesktopPrefs::load(dir.path());
        assert!(p.autostart_initialized);
        assert!(!p.background_hint_shown);
        // Champ apparu avec l'écran d'accueil, absent d'un fichier déjà
        // existant : considéré comme déjà conclu (pas de resurgissement pour
        // un bridge déjà installé avant cette version).
        assert!(p.welcome_completed);
    }

    #[test]
    fn welcome_screen_shows_once_for_a_brand_new_install_only() {
        let dir = tempfile::tempdir().unwrap();
        // Toute première installation : aucun fichier, l'écran doit s'afficher.
        let mut p = DesktopPrefs::load(dir.path());
        assert!(!p.welcome_completed);
        assert!(p.complete_welcome());
        assert!(!p.complete_welcome());
        p.save(dir.path()).unwrap();

        // Relecture : déjà conclu, ne redemande plus.
        let p = DesktopPrefs::load(dir.path());
        assert!(p.welcome_completed);
    }
}
