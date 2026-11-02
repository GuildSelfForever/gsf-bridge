//! Affiche en JSON le modèle normalisé de fichiers SavedVariables.
//!
//! ```text
//! cargo run -p gsf-core --example dump -- <compte.lua> [<perso.lua>...]
//! ```
//!
//! Le fichier contenant `GuildFoundForeverDB` est le fichier compte ; les
//! autres sont traités comme fichiers de personnages. Sans fichier compte, le
//! premier argument est utilisé.

use std::io::Write;
use std::process::ExitCode;

use gsf_core::model::normalize;
use gsf_core::sv::{parse, SavedVariables};

fn main() -> ExitCode {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: dump <fichier.lua>...");
        return ExitCode::from(2);
    }
    let mut account: Option<SavedVariables> = None;
    let mut characters: Vec<(String, SavedVariables)> = Vec::new();
    for path in paths {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{path}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let sv = match parse(&bytes) {
            Ok(sv) => sv,
            Err(e) => {
                eprintln!("{path}:{e}");
                return ExitCode::FAILURE;
            }
        };
        if account.is_none() && sv.get("GuildFoundForeverDB").is_some() {
            account = Some(sv);
        } else {
            characters.push((path, sv));
        }
    }
    let account = account.unwrap_or_default();
    let guilds = normalize(&account, &characters);
    match serde_json::to_string_pretty(&guilds) {
        Ok(json) => {
            // Pas de panique si la sortie est fermée (`| head`).
            let _ = writeln!(std::io::stdout().lock(), "{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("sérialisation JSON : {e}");
            ExitCode::FAILURE
        }
    }
}
