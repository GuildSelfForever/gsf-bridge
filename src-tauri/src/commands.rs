//! Commandes appelées par l'interface (`window.__TAURI__.core.invoke`).

use gsf_sync::engine::Status;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::{AppState, Trigger};

/// Dépôt du code source (écran d'accueil, contrat étape 1). Adresse encore
/// provisoire tant que le dépôt n'est pas passé public (étape 3 du plan).
const REPO_URL: &str = "https://github.com/GuildSelfForever/gsf-bridge";

/// Ouvre le dépôt du code dans le navigateur (écran d'accueil).
#[tauri::command]
pub fn open_repo(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(REPO_URL, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> Result<Status, String> {
    let engine = state.engine.clone();
    tauri::async_runtime::spawn_blocking(move || engine.status())
        .await
        .map_err(|e| e.to_string())
}

/// Connexion : ouvre le navigateur sur la page d'autorisation du site.
#[tauri::command]
pub async fn login(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let opener = app.clone();
    let username = state
        .engine
        .login(move |url| {
            opener
                .opener()
                .open_url(url, None::<&str>)
                .map_err(|e| e.to_string())
        })
        .await?;
    state.trigger(Trigger::LoggedIn);
    Ok(username)
}

#[tauri::command]
pub fn logout(state: State<'_, AppState>) {
    state.engine.logout();
}

#[tauri::command]
pub fn sync_now(state: State<'_, AppState>) {
    state.trigger(Trigger::SyncNow);
}

/// Choix manuel du dossier WoW (racine ou dossier de version).
#[tauri::command]
pub async fn choose_wow_dir(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Dossier World of Warcraft")
            .blocking_pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(picked) = picked else {
        return Ok(false);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    state.engine.set_wow_dir(Some(path))?;
    state.trigger(Trigger::Rewatch);
    Ok(true)
}

/// Retour à la détection automatique (registre Windows).
#[tauri::command]
pub fn reset_wow_dir(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.set_wow_dir(None)?;
    state.trigger(Trigger::Rewatch);
    Ok(())
}

/// Adresse du site (développement) ; vide = production.
#[tauri::command]
pub fn set_base_url(state: State<'_, AppState>, url: Option<String>) -> Result<(), String> {
    state.engine.set_base_url(url)
}

/// Case « Synchroniser mon addon entre mes ordinateurs » (contrat
/// 2026-11-03) : enregistrée tout de suite, puis envoi ou oubli des
/// inventaires et relecture des données poussées en arrière-plan.
#[tauri::command]
pub fn set_audit_sync(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    state.engine.set_audit_sync(enabled)?;
    state.trigger(Trigger::AuditSyncChanged);
    Ok(())
}
