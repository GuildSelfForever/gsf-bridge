//! Découverte de l'installation WoW et des fichiers SavedVariables de l'addon.
//!
//! Tous les chemins restent des [`PathBuf`]/[`OsString`] de bout en bout : les
//! noms de royaumes et de personnages (`Dävin`, `Fumée-Ravine`…) ne sont
//! jamais supposés ni reconstruits, seulement lus sur le disque. La seule
//! conversion en texte (avec remplacement) est l'indice de chemin envoyé au
//! backend ([`relative_display`]).

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

/// Nom du fichier SavedVariables de l'addon.
pub const SV_FILE_NAME: &str = "GuildSelfForever.lua";
/// Dossier de l'addon principal (sert à repérer les versions où il est installé).
pub const MAIN_ADDON_DIR: &str = "GuildSelfForever";

/// Versions du client connues, par ordre de préférence. Les autres dossiers
/// `_xxx_` contenant `WTF/Account` sont aussi acceptés, après celles-ci.
pub const KNOWN_FLAVORS: &[&str] = &[
    "_classic_beta_",
    "_anniversary_",
    "_classic_era_",
    "_classic_",
    "_classic_ptr_",
    "_classic_era_ptr_",
    "_retail_",
    "_ptr_",
    "_beta_",
];

/// Une version installée du client (`_classic_beta_`, `_retail_`…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flavor {
    pub name: OsString,
    pub path: PathBuf,
}

/// Fichiers SavedVariables de l'addon pour un compte WoW.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFiles {
    /// Nom du dossier de compte (`WTF/Account/<ACCOUNT>`).
    pub account: OsString,
    pub account_dir: PathBuf,
    /// `WTF/Account/<ACCOUNT>/SavedVariables/GuildSelfForever.lua`, s'il existe.
    pub account_file: Option<PathBuf>,
    pub characters: Vec<CharacterFile>,
}

/// Fichier SavedVariables d'un personnage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterFile {
    pub realm_dir: OsString,
    pub character_dir: OsString,
    /// `WTF/Account/<ACCOUNT>/<royaume>/<perso>/SavedVariables/GuildSelfForever.lua`.
    pub path: PathBuf,
}

impl AccountFiles {
    /// Tous les fichiers (compte puis personnages).
    pub fn all_files(&self) -> impl Iterator<Item = &Path> + '_ {
        self.account_file
            .as_deref()
            .into_iter()
            .chain(self.characters.iter().map(|c| c.path.as_path()))
    }
}

/// `true` si `dir` est une version du client (contient `WTF/Account`).
pub fn is_flavor_dir(dir: &Path) -> bool {
    dir.join("WTF").join("Account").is_dir()
}

fn flavor_rank(name: &OsStr) -> usize {
    name.to_str()
        .and_then(|n| KNOWN_FLAVORS.iter().position(|k| k.eq_ignore_ascii_case(n)))
        .unwrap_or(KNOWN_FLAVORS.len())
}

fn looks_like_flavor_name(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    bytes.len() >= 3 && bytes.first() == Some(&b'_') && bytes.last() == Some(&b'_')
}

/// Ramène un chemin (valeur du registre ou choix manuel) à la racine
/// `World of Warcraft` : un dossier de version (`…\_classic_\`) donne son parent.
pub fn normalize_install_root(path: &Path) -> PathBuf {
    match (path.file_name(), path.parent()) {
        (Some(name), Some(parent)) if looks_like_flavor_name(name) || is_flavor_dir(path) => {
            parent.to_path_buf()
        }
        _ => path.to_path_buf(),
    }
}

/// Versions présentes sous `root`. `root` peut être la racine `World of
/// Warcraft` ou directement un dossier de version (choix manuel).
pub fn list_flavors(root: &Path) -> Vec<Flavor> {
    if is_flavor_dir(root) && root.file_name().is_some_and(looks_like_flavor_name) {
        return vec![Flavor {
            name: root.file_name().unwrap_or_default().to_os_string(),
            path: root.to_path_buf(),
        }];
    }
    let mut flavors: Vec<Flavor> = read_dirs(root)
        .into_iter()
        .filter(|(name, path)| looks_like_flavor_name(name) && is_flavor_dir(path))
        .map(|(name, path)| Flavor { name, path })
        .collect();
    flavors.sort_by(
        |a, b| match flavor_rank(&a.name).cmp(&flavor_rank(&b.name)) {
            Ordering::Equal => a.name.cmp(&b.name),
            o => o,
        },
    );
    if flavors.is_empty() && is_flavor_dir(root) {
        // Dossier choisi à la main qui contient WTF sans nom `_xxx_`.
        flavors.push(Flavor {
            name: root.file_name().unwrap_or_default().to_os_string(),
            path: root.to_path_buf(),
        });
    }
    flavors
}

/// Sous-dossiers (nom, chemin) d'un dossier, triés par nom ; vide si illisible.
fn read_dirs(dir: &Path) -> Vec<(OsString, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(OsString, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| (e.file_name(), e.path()))
        .collect();
    out.sort();
    out
}

fn sv_file_in(dir: &Path) -> Option<PathBuf> {
    let path = dir.join("SavedVariables").join(SV_FILE_NAME);
    path.is_file().then_some(path)
}

/// Fichiers de l'addon de chaque compte d'une version, triés par compte.
/// Les comptes sans aucun fichier sont omis. Aucun nom de royaume n'est
/// supposé : chaque `WTF/Account/<A>/<R>/<P>/SavedVariables/` est exploré.
pub fn find_saved_variables(flavor_dir: &Path) -> Vec<AccountFiles> {
    let accounts_dir = flavor_dir.join("WTF").join("Account");
    let mut out = Vec::new();
    for (account, account_dir) in read_dirs(&accounts_dir) {
        let account_file = sv_file_in(&account_dir);
        let mut characters = Vec::new();
        for (realm_dir, realm_path) in read_dirs(&account_dir) {
            if realm_dir == "SavedVariables" {
                continue;
            }
            for (character_dir, char_path) in read_dirs(&realm_path) {
                if let Some(path) = sv_file_in(&char_path) {
                    characters.push(CharacterFile {
                        realm_dir: realm_dir.clone(),
                        character_dir,
                        path,
                    });
                }
            }
        }
        if account_file.is_some() || !characters.is_empty() {
            out.push(AccountFiles {
                account,
                account_dir,
                account_file,
                characters,
            });
        }
    }
    out
}

/// Noms de personnages affichés à l'enregistrement d'un appareil (contrat
/// `POST /api/v1/bridge/devices`, décision « vigies ») : `perso-royaume`,
/// lus sur les dossiers `WTF/Account/<compte>/<royaume>/<perso>` — affichage
/// seulement, jamais une preuve. Triés, dédoublonnés, au plus 50 entrées de
/// 80 caractères.
pub fn character_display_names(flavor_dirs: &[PathBuf]) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for flavor in flavor_dirs {
        for acc in find_saved_variables(flavor) {
            for c in &acc.characters {
                let name = format!(
                    "{}-{}",
                    c.character_dir.to_string_lossy(),
                    c.realm_dir.to_string_lossy()
                );
                names.insert(name.chars().take(80).collect());
            }
        }
    }
    names.into_iter().take(50).collect()
}

/// Dossier `WTF/Account` d'une version (à surveiller récursivement).
pub fn accounts_dir(flavor_dir: &Path) -> PathBuf {
    flavor_dir.join("WTF").join("Account")
}

/// Dossier `Interface/AddOns` d'une version.
pub fn addons_dir(flavor_dir: &Path) -> PathBuf {
    flavor_dir.join("Interface").join("AddOns")
}

/// `true` si l'addon GuildSelfForever est installé dans cette version.
pub fn has_main_addon(flavor_dir: &Path) -> bool {
    addons_dir(flavor_dir).join(MAIN_ADDON_DIR).is_dir()
}

/// Chemin relatif à `base` avec des `/`, pour affichage et indice envoyé au
/// backend (`WTF/Account/…`). Remplacement des caractères non représentables.
pub fn relative_display(base: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(base).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Racine `World of Warcraft` lue dans le registre Windows
/// (`HKLM\SOFTWARE\WOW6432Node\Blizzard Entertainment\World of Warcraft`,
/// valeur `InstallPath`), sinon les emplacements par défaut.
#[cfg(windows)]
pub fn detect_install_root() -> Option<PathBuf> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for key in [
        r"SOFTWARE\WOW6432Node\Blizzard Entertainment\World of Warcraft",
        r"SOFTWARE\Blizzard Entertainment\World of Warcraft",
    ] {
        let value: Option<OsString> = hklm
            .open_subkey(key)
            .and_then(|k| k.get_value("InstallPath"))
            .ok();
        if let Some(value) = value {
            let root = normalize_install_root(Path::new(&value));
            if !list_flavors(&root).is_empty() {
                return Some(root);
            }
        }
    }
    [
        r"C:\Program Files (x86)\World of Warcraft",
        r"C:\Program Files\World of Warcraft",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| !list_flavors(p).is_empty())
}

/// macOS : appli Blizzard installée dans `/Applications/World of Warcraft`
/// (ou dans le dossier Applications de l'utilisateur).
#[cfg(target_os = "macos")]
pub fn detect_install_root() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("/Applications/World of Warcraft")];
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(Path::new(&home).join("Applications/World of Warcraft"));
    }
    first_install_root(candidates)
}

/// Linux : WoW tourne sous Wine (Lutris, Bottles, préfixe Wine par défaut) ;
/// on cherche le dossier `World of Warcraft` dans les préfixes les plus
/// courants, sinon choix manuel du dossier dans le bridge.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn detect_install_root() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let mut prefixes = vec![
        home.join(".wine"),
        home.join("Games/battlenet"),
        home.join("Games/battle-net"),
        home.join("Games/world-of-warcraft"),
    ];
    // Bottles : une bouteille par dossier, nom libre.
    for bottles in [
        home.join(".local/share/bottles/bottles"),
        home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"),
    ] {
        if let Ok(entries) = fs::read_dir(&bottles) {
            prefixes.extend(entries.flatten().map(|e| e.path()));
        }
    }
    first_install_root(linux_install_candidates(&prefixes))
}

/// Dossiers `World of Warcraft` possibles dans des préfixes Wine.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn linux_install_candidates(prefixes: &[PathBuf]) -> Vec<PathBuf> {
    prefixes
        .iter()
        .flat_map(|prefix| {
            [
                prefix.join("drive_c/Program Files (x86)/World of Warcraft"),
                prefix.join("drive_c/Program Files/World of Warcraft"),
            ]
        })
        .collect()
}

/// Le premier emplacement qui contient au moins une version du jeu.
#[cfg(any(test, not(windows)))]
fn first_install_root(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|p| !list_flavors(p).is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_game_in_a_wine_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty-prefix");
        let lutris = dir.path().join("Games/battlenet");
        let wow = lutris.join("drive_c/Program Files (x86)/World of Warcraft");
        fs::create_dir_all(wow.join("_classic_beta_/WTF/Account")).unwrap();
        fs::create_dir_all(empty.join("drive_c")).unwrap();

        let found = first_install_root(linux_install_candidates(&[empty, lutris]));
        assert_eq!(found, Some(wow));
        assert_eq!(first_install_root(linux_install_candidates(&[])), None);
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"GuildFoundForeverDB = {}\n").unwrap();
    }

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("World of Warcraft");
        let beta = root.join("_classic_beta_");
        let acc = beta.join("WTF/Account/DAVIN#1");
        touch(&acc.join("SavedVariables").join(SV_FILE_NAME));
        touch(&acc.join("SavedVariables/Other.lua"));
        touch(
            &acc.join("Dävin-Abysses/Dävin/SavedVariables")
                .join(SV_FILE_NAME),
        );
        touch(
            &acc.join("Fumée-Ravine/Fumée/SavedVariables")
                .join(SV_FILE_NAME),
        );
        // Perso sans fichier de l'addon : ignoré.
        touch(&acc.join("Fumée-Ravine/Autre/SavedVariables/Other.lua"));
        fs::create_dir_all(acc.join("Fumée-Ravine/Vide")).unwrap();
        // Compte sans rien : omis.
        fs::create_dir_all(beta.join("WTF/Account/VIDE/SavedVariables")).unwrap();
        // Versions : retail sans fichier, era, un dossier non-version.
        fs::create_dir_all(root.join("_retail_/WTF/Account")).unwrap();
        fs::create_dir_all(root.join("_classic_era_/WTF/Account")).unwrap();
        fs::create_dir_all(root.join("_xptr_/Interface")).unwrap();
        fs::create_dir_all(root.join("Data")).unwrap();
        fs::create_dir_all(beta.join("Interface/AddOns").join(MAIN_ADDON_DIR)).unwrap();
        dir
    }

    #[test]
    fn flavors_are_listed_in_preference_order() {
        let dir = tree();
        let root = dir.path().join("World of Warcraft");
        let names: Vec<_> = list_flavors(&root).into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["_classic_beta_", "_classic_era_", "_retail_"]);
        // Choix manuel d'une version précise.
        let only = list_flavors(&root.join("_classic_beta_"));
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].path, root.join("_classic_beta_"));
        assert!(list_flavors(&dir.path().join("absent")).is_empty());
    }

    #[test]
    fn install_root_from_registry_value() {
        let dir = tree();
        let root = dir.path().join("World of Warcraft");
        assert_eq!(normalize_install_root(&root.join("_classic_beta_")), root);
        assert_eq!(normalize_install_root(&root.join("_anniversary_")), root);
        assert_eq!(normalize_install_root(&root), root);
    }

    #[test]
    fn saved_variables_found_with_non_ascii_names() {
        let dir = tree();
        let beta = dir.path().join("World of Warcraft/_classic_beta_");
        let found = find_saved_variables(&beta);
        assert_eq!(found.len(), 1);
        let acc = &found[0];
        assert_eq!(acc.account, "DAVIN#1");
        assert_eq!(
            acc.account_file.as_deref(),
            Some(
                acc.account_dir
                    .join("SavedVariables")
                    .join(SV_FILE_NAME)
                    .as_path()
            )
        );
        let chars: Vec<_> = acc
            .characters
            .iter()
            .map(|c| (c.realm_dir.clone(), c.character_dir.clone()))
            .collect();
        assert_eq!(
            chars,
            [
                (OsString::from("Dävin-Abysses"), OsString::from("Dävin")),
                (OsString::from("Fumée-Ravine"), OsString::from("Fumée")),
            ]
        );
        assert!(acc.characters.iter().all(|c| c.path.is_file()));
        assert_eq!(acc.all_files().count(), 3);
        assert_eq!(
            relative_display(&beta, &acc.characters[0].path),
            "WTF/Account/DAVIN#1/Dävin-Abysses/Dävin/SavedVariables/GuildSelfForever.lua"
        );
        assert!(has_main_addon(&beta));
        assert!(!has_main_addon(
            &dir.path().join("World of Warcraft/_retail_")
        ));
    }

    #[test]
    fn character_display_names_are_perso_dash_royaume_sorted_and_deduped() {
        let dir = tree();
        let beta = dir.path().join("World of Warcraft/_classic_beta_");
        let names = character_display_names(&[beta.clone(), beta]);
        assert_eq!(names, ["Dävin-Dävin-Abysses", "Fumée-Fumée-Ravine"]);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_folder_names_are_kept_as_os_strings() {
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let realm = OsStr::from_bytes(b"Royaume-\xff");
        let file = dir
            .path()
            .join("WTF/Account/A")
            .join(realm)
            .join("Perso/SavedVariables")
            .join(SV_FILE_NAME);
        touch(&file);
        let found = find_saved_variables(dir.path());
        assert_eq!(found[0].characters[0].realm_dir, realm);
        assert_eq!(found[0].characters[0].path, file);
        assert!(relative_display(dir.path(), &file).contains('\u{fffd}'));
    }
}
