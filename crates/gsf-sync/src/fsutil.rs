//! Petits utilitaires de fichiers : écriture atomique, JSON persistant, dates.

use std::fs::{self, File, Metadata};
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Écrit `bytes` dans `path` via un fichier temporaire du même dossier puis un
/// renommage : un lecteur voit l'ancien contenu ou le nouveau, jamais un
/// fichier à moitié écrit. Crée le dossier parent si besoin.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "chemin sans dossier parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "chemin sans nom de fichier"))?;
    fs::create_dir_all(dir)?;
    let mut tmp_name = name.to_os_string();
    tmp_name.push(".gsf-tmp");
    let tmp = dir.join(tmp_name);
    let result = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        // `rename` remplace la cible existante (MoveFileExW REPLACE_EXISTING sous Windows).
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Lit un JSON ; `Ok(None)` si le fichier n'existe pas.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Écrit un JSON lisible, atomiquement.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    atomic_write(path, &bytes)
}

/// Secondes depuis l'époque Unix (0 si l'horloge est avant 1970).
pub fn epoch_secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Heure actuelle en secondes Unix.
pub fn now_secs() -> i64 {
    epoch_secs(SystemTime::now())
}

/// Date de modification en secondes Unix, si disponible.
pub fn mtime_secs(meta: &Metadata) -> Option<i64> {
    meta.modified().ok().map(epoch_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sous dossier é").join("Data.lua");
        atomic_write(&path, b"un").unwrap();
        atomic_write(&path, b"deux").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"deux");
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("Data.lua")]);
    }

    #[test]
    fn json_roundtrip_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        assert_eq!(read_json::<Vec<u32>>(&path).unwrap(), None);
        write_json(&path, &vec![1u32, 2]).unwrap();
        assert_eq!(read_json::<Vec<u32>>(&path).unwrap(), Some(vec![1, 2]));
        fs::write(&path, b"{corrompu").unwrap();
        assert!(read_json::<Vec<u32>>(&path).is_err());
    }
}
