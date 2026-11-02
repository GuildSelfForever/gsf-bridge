//! Application Tauri de GSF Bridge : fenêtre, commandes et minuteries.
//! Toute la logique est dans `gsf-sync` ; ce crate ne fait que la brancher.

mod commands;
mod desktop;
mod updater;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gsf_sync::auth::{KeyringDeviceSecretStore, KeyringStore};
use gsf_sync::engine::{Engine, EngineOptions, Level, Notifier};
use gsf_sync::watch::{self, SvWatcher, DEFAULT_DEBOUNCE};
use gsf_sync::wow::{detect_install_root, SystemGameDetector};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

/// Événement de l'état complet poussé vers l'interface.
pub const STATUS_EVENT: &str = "gsf://status";

/// Demandes adressées à la boucle de fond.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Un fichier SavedVariables a changé.
    FilesChanged,
    /// Bouton « Synchroniser ».
    SyncNow,
    /// Dossier WoW changé : relancer la surveillance.
    Rewatch,
    /// Connexion réussie : vider la file, données poussées.
    LoggedIn,
}

/// État partagé de l'application.
pub struct AppState {
    pub engine: Arc<Engine>,
    pub wake: mpsc::UnboundedSender<Trigger>,
    watcher: Mutex<Option<SvWatcher>>,
    pub updates: updater::UpdateSlot,
}

impl AppState {
    pub fn trigger(&self, t: Trigger) {
        let _ = self.wake.send(t);
    }
}

/// (Re)lance la surveillance des dossiers `WTF/Account`.
fn rewatch(app: &AppHandle) {
    let state = app.state::<AppState>();
    let dirs = state.engine.watch_dirs();
    let mut slot = state.watcher.lock().unwrap_or_else(|p| p.into_inner());
    *slot = None;
    if dirs.is_empty() {
        return;
    }
    let wake = state.wake.clone();
    match watch::watch(&dirs, DEFAULT_DEBOUNCE, move |_| {
        let _ = wake.send(Trigger::FilesChanged);
    }) {
        Ok(w) => {
            let list: Vec<String> = w.watched.iter().map(|d| d.display().to_string()).collect();
            state
                .engine
                .log(Level::Info, format!("Surveillance : {}", list.join(", ")));
            *slot = Some(w);
        }
        Err(e) => state
            .engine
            .log(Level::Error, format!("Surveillance impossible : {e}")),
    }
}

/// Envoie l'état à l'interface et à l'icône près de l'horloge, en regroupant
/// les rafales de changements ; recalculé aussi chaque minute (« il y a
/// 2 min » vieillit, une coupure du site s'allonge).
async fn status_emitter(app: AppHandle, mut rx: mpsc::UnboundedReceiver<()>) {
    let mut minute = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            changed = rx.recv() => {
                if changed.is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(150)).await;
                while rx.try_recv().is_ok() {}
            }
            _ = minute.tick() => {}
        }
        let engine = app.state::<AppState>().engine.clone();
        // `status()` parcourt le disque et la liste des processus.
        if let Ok(status) = tauri::async_runtime::spawn_blocking(move || engine.status()).await {
            desktop::on_status(&app, &status);
            let _ = app.emit(STATUS_EVENT, status);
        }
    }
}

/// Boucle de fond : synchronisation au démarrage et à chaque changement,
/// file d'attente chaque minute, jeu fermé toutes les 5 s (données poussées en attente), données poussées toutes les
/// 15 minutes, signal de vie au démarrage puis toutes les 5 minutes, mises à jour au démarrage puis toutes les heures,
/// enregistrement d'appareil (contrat « vigies ») au démarrage, à chaque
/// changement de SavedVariables et toutes les 5 minutes tant qu'il n'y a
/// pas de token.
async fn background(app: AppHandle, mut rx: mpsc::UnboundedReceiver<Trigger>) {
    let engine = app.state::<AppState>().engine.clone();
    rewatch(&app);
    engine.register_if_needed().await;
    engine.sync_all("démarrage").await;
    engine.flush_queue().await;
    engine.refresh_pushed_data().await;
    engine.heartbeat().await;
    updater::check_in_background(&app).await;

    let mut tick = tokio::time::interval(Duration::from_secs(60));
    let mut pushed = tokio::time::interval(Duration::from_secs(15 * 60));
    let mut updates = tokio::time::interval(Duration::from_secs(60 * 60));
    let mut register = tokio::time::interval(Duration::from_secs(5 * 60));
    // Signal de vie (contrat 2026-11-02) : toutes les 5 minutes, et chaque minute si le jeu vient de s'ouvrir ou
    // de se fermer (`engine.tick`).
    let mut presence = tokio::time::interval(Duration::from_secs(5 * 60));
    // Données poussées en attente du jeu fermé : vérifié toutes les 5 s, pour les écrire avant qu'un joueur qui
    // ferme puis relance aussitôt le jeu ne le relance (la tâche d'une minute ratait ce créneau).
    let mut game_closed = tokio::time::interval(Duration::from_secs(5));
    // Le premier `tick()` d'un intervalle est immédiat : déjà fait ci-dessus.
    tick.tick().await;
    pushed.tick().await;
    updates.tick().await;
    register.tick().await;
    presence.tick().await;
    game_closed.tick().await;
    loop {
        tokio::select! {
            t = rx.recv() => {
                let Some(t) = t else { break };
                // Regroupe les demandes identiques déjà en attente.
                let mut pending = vec![t];
                while let Ok(next) = rx.try_recv() {
                    if !pending.contains(&next) {
                        pending.push(next);
                    }
                }
                for t in pending {
                    match t {
                        Trigger::FilesChanged => {
                            engine.register_if_needed().await;
                            engine.sync_all("fichier modifié").await;
                            // L'envoi peut créer un ban côté site : récupérer aussitôt
                            // l'accusé de réception pour le prochain lancement du jeu.
                            engine.refresh_pushed_data().await;
                        }
                        Trigger::SyncNow => {
                            engine.flush_queue().await;
                            engine.sync_all("demande manuelle").await;
                            engine.refresh_pushed_data().await;
                        }
                        Trigger::Rewatch => {
                            rewatch(&app);
                            engine.sync_all("dossier WoW changé").await;
                        }
                        Trigger::LoggedIn => {
                            engine.flush_queue().await;
                            engine.sync_all("connexion").await;
                            engine.refresh_pushed_data().await;
                            engine.heartbeat().await;
                            updater::check_in_background(&app).await;
                        }
                    }
                }
            }
            _ = tick.tick() => engine.tick().await,
            _ = game_closed.tick() => { engine.write_pending_pushed(); }
            _ = pushed.tick() => { engine.refresh_pushed_data().await; }
            _ = updates.tick() => updater::check_in_background(&app).await,
            _ = register.tick() => engine.register_if_needed().await,
            _ = presence.tick() => engine.heartbeat().await,
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // En premier : un deuxième lancement réveille l'instance ouverte.
        .plugin(tauri_plugin_single_instance::init(
            desktop::on_second_instance,
        ))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args([desktop::MINIMIZED_ARG])
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let (status_tx, status_rx) = mpsc::unbounded_channel::<()>();
            let notifier: Notifier = Arc::new(move || {
                let _ = status_tx.send(());
            });
            let engine = Arc::new(Engine::new(
                EngineOptions::new(&data_dir),
                Arc::new(KeyringStore::new()),
                Arc::new(KeyringDeviceSecretStore::new()),
                Arc::new(SystemGameDetector::new()),
                detect_install_root(),
                notifier,
            ));
            let (wake, wake_rx) = mpsc::unbounded_channel();
            app.manage(AppState {
                engine,
                wake,
                watcher: Mutex::new(None),
                updates: updater::UpdateSlot::default(),
            });
            app.manage(desktop::Desktop::new(data_dir));
            desktop::setup(app)?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(status_emitter(handle.clone(), status_rx));
            tauri::async_runtime::spawn(background(handle, wake_rx));
            Ok(())
        })
        .on_window_event(desktop::on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::login,
            commands::logout,
            commands::sync_now,
            commands::choose_wow_dir,
            commands::reset_wow_dir,
            commands::set_base_url,
            commands::open_repo,
            updater::check_update,
            updater::install_update,
            desktop::get_autostart,
            desktop::set_autostart,
            desktop::needs_welcome,
            desktop::complete_welcome,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de GSF Bridge");
}
