//! Détection du jeu lancé : processus `Wow*.exe` (Windows, Linux sous Wine),
//! appli `World of Warcraft…` (macOS).

use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// Indique si le client WoW tourne. Injectable pour les tests.
pub trait GameDetector: Send + Sync {
    fn is_game_running(&self) -> bool;
}

/// `true` pour les exécutables du client : `Wow.exe`, `WowB.exe`, `WowT.exe`,
/// `WowClassic.exe`, `WowClassicB.exe`, `WowClassicT.exe`, `Wow-64.exe`…
/// Les outils tiers (`WowUp.exe`…) ne comptent pas. Insensible à la casse,
/// `.exe` facultatif (Wine, tests). Sur macOS, le client est l'appli
/// `World of Warcraft` (ou `World of Warcraft Classic`…), nom que macOS peut
/// couper à 15 caractères (`World of Warcra`).
pub fn is_wow_process_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("world of warcra") {
        return true;
    }
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    let Some(rest) = stem.strip_prefix("wow") else {
        return false;
    };
    let rest = rest.strip_suffix("-64").unwrap_or(rest);
    let rest = rest.strip_prefix("classic").unwrap_or(rest);
    matches!(rest, "" | "b" | "t" | "beta" | "ptr" | "x")
}

/// Détecteur réel basé sur la liste des processus (sysinfo).
pub struct SystemGameDetector {
    sys: Mutex<System>,
}

impl SystemGameDetector {
    pub fn new() -> Self {
        SystemGameDetector {
            sys: Mutex::new(System::new()),
        }
    }
}

impl Default for SystemGameDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl GameDetector for SystemGameDetector {
    fn is_game_running(&self) -> bool {
        let mut sys = match self.sys.lock() {
            Ok(s) => s,
            Err(poisoned) => poisoned.into_inner(),
        };
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        sys.processes()
            .values()
            .any(|p| is_wow_process_name(p.name()))
    }
}

/// Détecteur à valeur fixe, modifiable (tests, mode manuel).
#[derive(Debug, Default)]
pub struct FixedGameDetector(AtomicBool);

impl FixedGameDetector {
    pub fn new(running: bool) -> Self {
        FixedGameDetector(AtomicBool::new(running))
    }

    pub fn set(&self, running: bool) {
        self.0.store(running, Ordering::SeqCst);
    }
}

impl GameDetector for FixedGameDetector {
    fn is_game_running(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wow_executables_are_recognized() {
        for name in [
            "Wow.exe",
            "WowB.exe",
            "WowT.exe",
            "WowClassic.exe",
            "WowClassicB.exe",
            "WowClassicT.exe",
            "Wow-64.exe",
            "WOWB.EXE",
            "WowB",
            "World of Warcraft",
            "World of Warcraft Classic",
            "World of Warcra",
        ] {
            assert!(is_wow_process_name(OsStr::new(name)), "{name}");
        }
        for name in [
            "WowUp.exe",
            "WowUpCF.exe",
            "Battle.net.exe",
            "explorer.exe",
            "wowhead.exe",
            "Battle.net",
            "World",
            "",
        ] {
            assert!(!is_wow_process_name(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn fixed_detector_toggles() {
        let d = FixedGameDetector::new(false);
        assert!(!d.is_game_running());
        d.set(true);
        assert!(d.is_game_running());
    }

    #[test]
    fn system_detector_runs() {
        // Aucun WoW sur la machine de test : simple fumée, ne doit pas paniquer.
        let _ = SystemGameDetector::new().is_game_running();
    }
}
