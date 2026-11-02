# Desktop

L'application de bureau : framework, accès natif et packaging.

## Framework

- Tauri v2 ; processus principal dans `src-tauri/src/lib.rs` (état partagé, boucle de fond `tokio` : sync au démarrage et à chaque changement, file et jeu fermé chaque minute, données poussées toutes les 15 min, signal de vie (`POST /bridge/heartbeat`, jeu ouvert ou non) au démarrage, à la connexion, toutes les 5 min et dès que le jeu s'ouvre ou se ferme (vu par la tâche d'une minute), mises à jour au démarrage puis toutes les heures, enregistrement d'appareil « vigie » au démarrage, à chaque changement de SavedVariables et toutes les 5 min tant qu'il n'y a pas de token).
- L'UI (`ui/`) appelle les commandes par `window.__TAURI__.core.invoke` et reçoit l'état par événements `gsf://status`, `gsf://update`, `gsf://update-progress`, `gsf://open-update`. Écran principal = une phrase d'état calculée par `gsf-sync` (`headline.rs`) ; « Plus tard » sur la fenêtre de mise à jour vaut pour le lancement en cours, sauf mise à jour obligatoire (426). CSP stricte, scripts locaux seulement.
- Vit en arrière-plan (`src-tauri/src/desktop.rs`) : icône près de l'horloge (pastille ok/attention/erreur, info-bulle = phrase d'état), fermer la fenêtre la cache (seul « Quitter » sort), démarrage avec Windows activé au premier lancement avec `--minimized` (fenêtre `visible: false` dans `tauri.conf.json`, montrée sauf lancement caché), instance unique, notifications sur changement d'état seulement. Règles pures dans `gsf-sync` : `tray.rs`, `alerts.rs`, `prefs.rs`. État recalculé à chaque changement et chaque minute.
- Windows est la cible principale (macOS et Linux construits mais pas encore validés sur de vraies machines) (identifiant `fr.gs4ever.bridge`, données dans `%APPDATA%\fr.gs4ever.bridge\` : `config.json`, `state.json`, `queue.json`, `desktop.json`).

## Native access

- Registre Windows (`winreg`) pour trouver WoW, détection de `Wow.exe`/`WowB.exe` (`sysinfo`), surveillance de `WTF/Account` (`notify`, anti-rebond 3 s).
- Lecture des SavedVariables de l'addon, jamais écrites ; écriture seulement de `Interface/AddOns/GuildSelfForever_Data/` dans chaque version où l'addon est installé, jeu fermé.
- Plugins : `single-instance` (enregistré en premier), `autostart` (clé `HKCU\…\Run` « GSF Bridge »), `notification`, `opener` (navigateur de connexion), `dialog` (choix du dossier WoW), `process` (relance), `updater`. Autostart et notifications sont pilotés côté Rust (commandes `get_autostart`/`set_autostart`), donc aucune permission de plugin en plus dans `src-tauri/capabilities/default.json`.
- Les notifications de bureau n'ont pas de clic programmable : cliquer relance l'exe, l'instance unique ramène la fenêtre (et émet `gsf://open-update` si une mise à jour attend).

## Build and release

- Installeur NSIS (`installMode: currentUser`, français) ; `createUpdaterArtifacts` jamais activé : `release.yml` signe lui-même l'installeur, l'AppImage et le `.app.tar.gz` (`cargo tauri signer sign --app-version`). Publication et clé de signature : voir `deployment.md`.
- Updater : endpoint posé à l'exécution (adresse du site réglable) avec le token bridge et `X-Bridge-Version` ; installation NSIS passive puis relance.
- Type-check Windows depuis Linux sans les bibliothèques Tauri : `cargo check`/`cargo clippy -p gsf-bridge --target x86_64-pc-windows-msvc` avec un faux `llvm-rc` et un faux `lib.exe` (pour `ring` ; scripts qui créent simplement le fichier de sortie demandé) placé dans un dossier temporaire ajouté au `PATH` pour cette seule commande, jamais dans le dépôt. Cela valide `tauri-build` et `generate_context!` mais pas l'édition de liens ni les ressources : seul le build CI fait foi.
