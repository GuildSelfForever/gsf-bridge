//! Lecture des SavedVariables au bon moment.
//!
//! WoW réécrit les fichiers à la déconnexion (et renomme l'ancien en `.bak`).
//! Après une notification, on attend que taille et date de modification
//! soient identiques sur deux lectures espacées ([`wait_stable`]), puis on
//! parse ; un échec de parsing (fichier tronqué) donne une relecture, puis un
//! repli sur `<fichier>.bak` ([`read_saved_variables`]).

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use gsf_core::sv::{parse, SavedVariables};

use crate::fsutil::epoch_secs;

/// Réglages de l'attente de stabilité.
#[derive(Debug, Clone, Copy)]
pub struct StabilityConfig {
    /// Écart entre deux lectures identiques (≥ 2 s en production).
    pub interval: Duration,
    /// Abandon si le fichier change encore après ce délai.
    pub max_wait: Duration,
}

impl Default for StabilityConfig {
    fn default() -> Self {
        StabilityConfig {
            interval: Duration::from_secs(2),
            max_wait: Duration::from_secs(60),
        }
    }
}

/// Empreinte d'un fichier : taille et date de modification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

/// Empreinte actuelle d'un fichier.
pub fn stamp(path: &Path) -> io::Result<Stamp> {
    let meta = fs::metadata(path)?;
    Ok(Stamp {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// Attend que `path` ait la même empreinte sur deux lectures espacées de
/// `cfg.interval`. Erreur `TimedOut` si le fichier change encore après
/// `cfg.max_wait`, ou erreur d'E/S (fichier disparu…).
pub fn wait_stable(path: &Path, cfg: StabilityConfig) -> io::Result<Stamp> {
    let started = Instant::now();
    let mut last = stamp(path)?;
    loop {
        thread::sleep(cfg.interval);
        let current = stamp(path)?;
        if current == last {
            return Ok(current);
        }
        if started.elapsed() >= cfg.max_wait {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{} change encore après {:?}", path.display(), cfg.max_wait),
            ));
        }
        last = current;
    }
}

/// Attend la stabilité de plusieurs fichiers ensemble : chacun doit être
/// stable, et aucun ne doit avoir bougé pendant l'attente des autres.
/// Les fichiers disparus sont ignorés.
pub fn wait_all_stable(paths: &[PathBuf], cfg: StabilityConfig) -> io::Result<()> {
    let started = Instant::now();
    loop {
        let before: Vec<Option<Stamp>> = paths.iter().map(|p| stamp(p).ok()).collect();
        for p in paths {
            match wait_stable(p, cfg) {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        let after: Vec<Option<Stamp>> = paths.iter().map(|p| stamp(p).ok()).collect();
        if before == after {
            return Ok(());
        }
        if started.elapsed() >= cfg.max_wait {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fichiers SavedVariables encore en cours d'écriture",
            ));
        }
    }
}

/// Fichier parsé.
#[derive(Debug, Clone)]
pub struct ReadFile {
    pub sv: SavedVariables,
    /// Date de modification (secondes Unix) du fichier effectivement lu.
    pub mtime: Option<i64>,
    /// `true` si le contenu vient de `<fichier>.bak`.
    pub from_backup: bool,
}

/// Échec de lecture d'un fichier et de sa sauvegarde.
#[derive(Debug, thiserror::Error)]
#[error("{path} illisible ({primary}) ; sauvegarde : {backup}")]
pub struct ReadError {
    pub path: String,
    pub primary: String,
    pub backup: String,
}

/// Chemin de la sauvegarde écrite par WoW : `<fichier>.bak`.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name: OsString = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

fn read_once(path: &Path) -> Result<(SavedVariables, Option<i64>), String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let sv = parse(&bytes).map_err(|e| format!("ligne {e}"))?;
    Ok((sv, meta.modified().ok().map(epoch_secs)))
}

/// Lit et parse `path` ; en cas d'échec, relit une fois après `retry_delay`,
/// puis se replie sur `<fichier>.bak`.
pub fn read_saved_variables(path: &Path, retry_delay: Duration) -> Result<ReadFile, ReadError> {
    let primary = match read_once(path) {
        Ok((sv, mtime)) => {
            return Ok(ReadFile {
                sv,
                mtime,
                from_backup: false,
            })
        }
        Err(_) => {
            thread::sleep(retry_delay);
            match read_once(path) {
                Ok((sv, mtime)) => {
                    return Ok(ReadFile {
                        sv,
                        mtime,
                        from_backup: false,
                    })
                }
                Err(e) => e,
            }
        }
    };
    match read_once(&backup_path(path)) {
        Ok((sv, mtime)) => Ok(ReadFile {
            sv,
            mtime,
            from_backup: true,
        }),
        Err(backup) => Err(ReadError {
            path: path.display().to_string(),
            primary,
            backup,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SHORT: StabilityConfig = StabilityConfig {
        interval: Duration::from_millis(150),
        max_wait: Duration::from_secs(5),
    };

    #[test]
    fn stable_file_returns_after_one_interval() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.lua");
        fs::write(&p, b"X = 1\n").unwrap();
        let t = Instant::now();
        let s = wait_stable(&p, SHORT).unwrap();
        assert_eq!(s.len, 6);
        assert!(t.elapsed() >= SHORT.interval);
        assert!(t.elapsed() < SHORT.interval * 3);
    }

    #[test]
    fn waits_while_file_keeps_growing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.lua");
        fs::write(&p, b"").unwrap();
        let writer_path = p.clone();
        let writer = thread::spawn(move || {
            for _ in 0..8 {
                let mut f = fs::OpenOptions::new()
                    .append(true)
                    .open(&writer_path)
                    .unwrap();
                f.write_all(b"-- ligne\n").unwrap();
                drop(f);
                thread::sleep(Duration::from_millis(60));
            }
        });
        let t = Instant::now();
        let s = wait_stable(&p, SHORT).unwrap();
        writer.join().unwrap();
        // 8 écritures toutes les 60 ms : au moins ~480 ms d'attente.
        assert!(
            t.elapsed() >= Duration::from_millis(400),
            "{:?}",
            t.elapsed()
        );
        assert_eq!(s.len, 8 * 9);
    }

    #[test]
    fn gives_up_when_file_never_settles() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.lua");
        fs::write(&p, b"").unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (wp, ws) = (p.clone(), stop.clone());
        let writer = thread::spawn(move || {
            while !ws.load(std::sync::atomic::Ordering::SeqCst) {
                let mut f = fs::OpenOptions::new().append(true).open(&wp).unwrap();
                f.write_all(b"x").unwrap();
                thread::sleep(Duration::from_millis(20));
            }
        });
        let cfg = StabilityConfig {
            interval: Duration::from_millis(100),
            max_wait: Duration::from_millis(400),
        };
        let err = wait_stable(&p, cfg).unwrap_err();
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        writer.join().unwrap();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = wait_stable(&dir.path().join("absent.lua"), SHORT).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        // Ensemble : les fichiers absents sont ignorés.
        wait_all_stable(&[dir.path().join("absent.lua")], SHORT).unwrap();
    }

    #[test]
    fn truncated_file_falls_back_to_bak() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("GuildSelfForever.lua");
        fs::write(&p, b"GuildFoundForeverDB = {\n[\"a\"] = ").unwrap();
        fs::write(backup_path(&p), b"GuildFoundForeverDB = { a = 1 }\n").unwrap();
        let r = read_saved_variables(&p, Duration::from_millis(10)).unwrap();
        assert!(r.from_backup);
        assert!(r.sv.get("GuildFoundForeverDB").is_some());
        assert_eq!(
            backup_path(&p).file_name().unwrap(),
            "GuildSelfForever.lua.bak"
        );
    }

    #[test]
    fn file_fixed_during_retry_is_read_without_backup() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("GuildSelfForever.lua");
        fs::write(&p, b"X = {").unwrap();
        let wp = p.clone();
        let fixer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            fs::write(&wp, b"X = { 1 }\n").unwrap();
        });
        let r = read_saved_variables(&p, Duration::from_millis(300)).unwrap();
        fixer.join().unwrap();
        assert!(!r.from_backup);
        assert!(r.mtime.is_some());
    }

    #[test]
    fn both_broken_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("GuildSelfForever.lua");
        fs::write(&p, b"X = {").unwrap();
        let err = read_saved_variables(&p, Duration::from_millis(1)).unwrap_err();
        assert!(err.to_string().contains("sauvegarde"));
    }
}
