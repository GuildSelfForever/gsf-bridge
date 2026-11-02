//! Accès aux fixtures réelles (copies des SavedVariables du client Forever,
//! addon 0.17.0 → 0.17.32), plus `synthetic_account_0.17.47.lua` écrit à la
//! main avec les formes exactes de `BridgeExport.lua` (addon 0.17.47).

#![allow(dead_code)]

use std::path::PathBuf;

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Toutes les fixtures `.lua` / `.lua.bak`, triées par nom.
pub fn fixtures() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<_> = std::fs::read_dir(fixtures_dir())
        .expect("dossier de fixtures")
        .map(|e| e.expect("entrée").path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy().into_owned();
            n.ends_with(".lua") || n.ends_with(".lua.bak")
        })
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&p).expect("lecture fixture"))
        })
        .collect();
    out.sort();
    out
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}
