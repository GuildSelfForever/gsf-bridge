//! Mise à jour signée (contrat §4, étape 1) via tauri-plugin-updater.
//!
//! L'endpoint dépend de l'adresse du site (réglable) et, s'il existe, du
//! token bridge : posé à l'exécution avec `updater_builder()`, ainsi que
//! `X-Bridge-Version` (repris aussi pour le téléchargement). Avec un token,
//! la route authentifiée est utilisée (`Authorization` en plus) ; sans
//! token (bridge non connecté), la route publique du contrat étape 1
//! (`/bridge/public/update/…`) est utilisée à la place, sans en-tête
//! `Authorization`. Le site ne bloque jamais ces routes par la version
//! minimale (426), un bridge trop ancien peut donc toujours se mettre à
//! jour. La vérification de signature (clé minisign de `tauri.conf.json`)
//! s'applique dans les deux cas, inchangée.

use std::sync::Mutex;
use std::time::Duration;

use gsf_sync::api::VERSION_HEADER;
use gsf_sync::engine::Level;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, Url};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

use crate::AppState;

/// Événement « mise à jour disponible ».
pub const UPDATE_EVENT: &str = "gsf://update";
/// Progression du téléchargement.
pub const PROGRESS_EVENT: &str = "gsf://update-progress";

/// Dernière mise à jour trouvée, prête à installer.
#[derive(Default)]
pub struct UpdateSlot(Mutex<Option<Update>>);

impl UpdateSlot {
    /// Une mise à jour trouvée attend d'être installée.
    pub fn is_pending(&self) -> bool {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).is_some()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct Progress {
    downloaded: usize,
    total: Option<u64>,
}

fn build(app: &AppHandle) -> Result<Updater, String> {
    let engine = app.state::<AppState>().engine.clone();
    let token = engine.token();
    let api = engine.api();
    // Contrat étape 1 : route publique sans jeton, authentifiée sinon.
    let endpoint_url = match &token {
        Some(_) => api.update_endpoint(),
        None => api.public_update_endpoint(),
    };
    let endpoint = Url::parse(&endpoint_url).map_err(|e| e.to_string())?;
    let mut builder = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|e| e.to_string())?
        .header(VERSION_HEADER, gsf_sync::BRIDGE_VERSION)
        .map_err(|e| e.to_string())?
        .timeout(Duration::from_secs(120));
    if let Some(token) = token {
        builder = builder
            .header("Authorization", format!("Bearer {token}"))
            .map_err(|e| e.to_string())?;
    }
    builder.build().map_err(|e| e.to_string())
}

async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = build(app)?.check().await.map_err(|e| e.to_string())?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
        date: u.date.map(|d| d.to_string()),
    });
    let state = app.state::<AppState>();
    *state.updates.0.lock().unwrap_or_else(|p| p.into_inner()) = update;
    if let Some(info) = &info {
        let _ = app.emit(UPDATE_EVENT, info.clone());
        crate::desktop::on_update_found(app, &info.version);
    }
    Ok(info)
}

/// Vérification silencieuse (démarrage, toutes les heures) : contrat étape 1,
/// faite même sans jeton (route publique), pour qu'un bridge non connecté se
/// mette quand même à jour.
pub async fn check_in_background(app: &AppHandle) {
    let engine = app.state::<AppState>().engine.clone();
    match check(app).await {
        Ok(Some(info)) => engine.log(
            Level::Info,
            format!("Mise à jour {} disponible", info.version),
        ),
        Ok(None) => {}
        Err(e) => engine.log(
            Level::Warn,
            format!("Vérification des mises à jour impossible : {e}"),
        ),
    }
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    check(&app).await
}

/// Télécharge, vérifie la signature et installe. Sous Windows, l'installeur
/// NSIS ferme puis relance l'application ; ailleurs l'interface appelle
/// `process.relaunch()` ensuite.
#[tauri::command]
pub async fn install_update(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let update = state
        .updates
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take();
    let update = match update {
        Some(u) => u,
        None => {
            check(&app).await?;
            state
                .updates
                .0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
                .ok_or_else(|| "Aucune mise à jour disponible".to_owned())?
        }
    };
    state.engine.log(
        Level::Info,
        format!("Installation de la version {}", update.version),
    );
    let mut downloaded = 0usize;
    let progress_app = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk;
                let _ = progress_app.emit(PROGRESS_EVENT, Progress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|e| {
            let msg = format!("Mise à jour échouée : {e}");
            state.engine.log(Level::Error, msg.clone());
            msg
        })
}
