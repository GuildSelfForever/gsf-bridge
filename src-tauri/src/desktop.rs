//! Vie en arrière-plan : icône près de l'horloge, fenêtre qui se cache au lieu
//! de se fermer, démarrage avec Windows, notifications et instance unique.
//! Les règles (couleur, info-bulle, quand notifier) sont dans `gsf-sync`
//! (`tray`, `alerts`, `prefs`) ; ce module ne fait que les brancher.

use std::path::PathBuf;
use std::sync::Mutex;

use gsf_sync::alerts::{Alert, AlertTracker};
use gsf_sync::engine::{Level, Status};
use gsf_sync::fsutil::now_secs;
use gsf_sync::prefs::DesktopPrefs;
use gsf_sync::tray::{self, TrayState};
use tauri::image::Image;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, Window, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt;

use crate::{AppState, Trigger};

/// Argument posé par le démarrage avec Windows : partir caché dans la barre.
pub const MINIMIZED_ARG: &str = "--minimized";
/// Demande à l'interface d'ouvrir la fenêtre de mise à jour.
pub const OPEN_UPDATE_EVENT: &str = "gsf://open-update";

const MAIN_WINDOW: &str = "main";
const TRAY_ID: &str = "gsf-tray";
const MENU_OPEN: &str = "open";
const MENU_SYNC: &str = "sync";
const MENU_QUIT: &str = "quit";

const ICON_OK: &[u8] = include_bytes!("../icons/tray/tray-ok.png");
const ICON_WARN: &[u8] = include_bytes!("../icons/tray/tray-warn.png");
const ICON_ERROR: &[u8] = include_bytes!("../icons/tray/tray-error.png");

/// État propre au bureau (à côté de [`AppState`]).
pub struct Desktop {
    data_dir: PathBuf,
    prefs: Mutex<DesktopPrefs>,
    alerts: Mutex<AlertTracker>,
    /// Dernière icône et info-bulle posées (évite de les reposer pour rien).
    shown: Mutex<Option<(TrayState, String)>>,
}

impl Desktop {
    pub fn new(data_dir: PathBuf) -> Self {
        let prefs = DesktopPrefs::load(&data_dir);
        Desktop {
            data_dir,
            prefs: Mutex::new(prefs),
            alerts: Mutex::new(AlertTracker::new()),
            shown: Mutex::new(None),
        }
    }

    /// Applique `f` aux préférences et les enregistre si elle rend `true`.
    fn with_prefs(&self, app: &AppHandle, f: impl FnOnce(&mut DesktopPrefs) -> bool) -> bool {
        let mut prefs = self.prefs.lock().unwrap_or_else(|p| p.into_inner());
        let changed = f(&mut prefs);
        if changed {
            if let Err(e) = prefs.save(&self.data_dir) {
                log(
                    app,
                    Level::Warn,
                    format!("Préférences non enregistrées : {e}"),
                );
            }
        }
        changed
    }
}

fn log(app: &AppHandle, level: Level, message: String) {
    app.state::<AppState>().engine.log(level, message);
}

fn icon(state: TrayState) -> tauri::Result<Image<'static>> {
    Image::from_bytes(match state {
        TrayState::Ok => ICON_OK,
        TrayState::Attention => ICON_WARN,
        TrayState::Error => ICON_ERROR,
    })
}

/// Lancé par le démarrage de Windows : rester caché.
pub fn started_minimized() -> bool {
    std::env::args().any(|a| a == MINIMIZED_ARG)
}

/// Montre la fenêtre principale au premier plan.
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Clic gauche sur l'icône : montre ou cache la fenêtre.
fn toggle_main(app: &AppHandle) {
    let Some(w) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let visible = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
    if visible {
        let _ = w.hide();
    } else {
        show_main(app);
    }
}

/// Montre la fenêtre et, si une mise à jour attend, sa fenêtre de mise à jour.
fn show_main_for_update(app: &AppHandle) {
    show_main(app);
    if app.state::<AppState>().updates.is_pending() {
        let _ = app.emit(OPEN_UPDATE_EVENT, ());
    }
}

/// Deuxième lancement (raccourci, clic sur une notification) : on réveille
/// l'instance déjà ouverte au lieu d'en démarrer une autre.
pub fn on_second_instance(app: &AppHandle, args: Vec<String>, _cwd: String) {
    if args.iter().any(|a| a == MINIMIZED_ARG) {
        return;
    }
    show_main_for_update(app);
}

/// Mise en place au démarrage : icône, démarrage avec Windows au premier
/// lancement, fenêtre montrée sauf lancement caché.
pub fn setup(app: &mut App) -> tauri::Result<()> {
    let handle = app.handle().clone();

    let open = MenuItem::with_id(app, MENU_OPEN, "Ouvrir GSF Bridge", true, None::<&str>)?;
    let sync = MenuItem::with_id(app, MENU_SYNC, "Envoyer maintenant", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quitter", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &sync, &separator, &quit])?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(TrayState::Ok)?)
        .tooltip(tray::APP_NAME)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(on_tray_event)
        .build(app)?;

    // Contrat étape 1 : plus d'activation silencieuse au premier lancement.
    // Le démarrage automatique n'est décidé que sur l'écran d'accueil
    // (`complete_welcome`), selon la case qui y est cochée.
    if !started_minimized() {
        show_main(&handle);
    }
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        MENU_OPEN => show_main(app),
        MENU_SYNC => app.state::<AppState>().trigger(Trigger::SyncNow),
        MENU_QUIT => app.exit(0),
        _ => {}
    }
}

fn on_tray_event(tray: &TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        toggle_main(tray.app_handle());
    }
}

/// Fermer la fenêtre la cache : seul « Quitter » arrête le bridge.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    if window.label() != MAIN_WINDOW {
        return;
    }
    api.prevent_close();
    let _ = window.hide();
    let app = window.app_handle();
    if app
        .state::<Desktop>()
        .with_prefs(app, DesktopPrefs::take_background_hint)
    {
        show_notification(
            app,
            "GSF Bridge continue en arrière-plan",
            "Il envoie tes données tout seul. Retrouve-le près de l'horloge ; « Quitter » dans son menu pour l'arrêter.",
        );
    }
}

/// Nouvel état : icône, info-bulle et notifications éventuelles.
pub fn on_status(app: &AppHandle, status: &Status) {
    let desktop = app.state::<Desktop>();
    let state = TrayState::from_tone(status.headline.tone);
    let tooltip = tray::tooltip(&status.headline);
    if let Some(tray_icon) = app.tray_by_id(TRAY_ID) {
        let mut shown = desktop.shown.lock().unwrap_or_else(|p| p.into_inner());
        let (old_state, old_tooltip) = match shown.as_ref() {
            Some((s, t)) => (Some(*s), Some(t.as_str())),
            None => (None, None),
        };
        if old_state != Some(state) {
            if let Ok(img) = icon(state) {
                let _ = tray_icon.set_icon(Some(img));
            }
        }
        if old_tooltip != Some(tooltip.as_str()) {
            let _ = tray_icon.set_tooltip(Some(&tooltip));
        }
        *shown = Some((state, tooltip));
    }
    let alerts = desktop
        .alerts
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .observe(status, now_secs());
    for alert in alerts {
        notify(app, &alert);
    }
}

/// Mise à jour trouvée par la vérification de fond.
pub fn on_update_found(app: &AppHandle, version: &str) {
    let alert = app
        .state::<Desktop>()
        .alerts
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .update_found(version);
    if let Some(alert) = alert {
        notify(app, &alert);
    }
}

/// Notifie, sauf si l'officier regarde déjà la fenêtre (la phrase d'état dit
/// la même chose).
fn notify(app: &AppHandle, alert: &Alert) {
    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let looking = w.is_visible().unwrap_or(false)
            && !w.is_minimized().unwrap_or(false)
            && w.is_focused().unwrap_or(false);
        if looking {
            return;
        }
    }
    show_notification(app, &alert.title(), &alert.body());
}

fn show_notification(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log(app, Level::Warn, format!("Notification impossible : {e}"));
    }
}

/// Case « Lancer au démarrage de Windows ».
#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|e| e.to_string())?;
    // Choix explicite : ne plus jamais l'activer d'office.
    app.state::<Desktop>()
        .with_prefs(&app, DesktopPrefs::take_first_autostart);
    log(
        &app,
        Level::Info,
        if enabled {
            "Lancement au démarrage de Windows activé".into()
        } else {
            "Lancement au démarrage de Windows désactivé".into()
        },
    );
    Ok(())
}

/// Contrat étape 1 : l'écran d'accueil (« Ce que fait le bridge ») doit
/// s'afficher tant qu'il n'a pas été conclu (toute première installation).
#[tauri::command]
pub fn needs_welcome(app: AppHandle) -> bool {
    !app.state::<Desktop>()
        .prefs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .welcome_completed
}

/// Conclut l'écran d'accueil : applique le démarrage automatique choisi sur
/// cet écran (case cochée par défaut) et ne le montre plus jamais, même si
/// l'activation du démarrage automatique échoue (l'erreur est renvoyée, mais
/// l'écran ne doit pas resurgir pour autant).
#[tauri::command]
pub fn complete_welcome(app: AppHandle, autostart: bool) -> Result<(), String> {
    app.state::<Desktop>()
        .with_prefs(&app, DesktopPrefs::complete_welcome);
    if !autostart {
        log(
            &app,
            Level::Info,
            "Écran d'accueil terminé : lancement au démarrage de Windows non activé".into(),
        );
        return Ok(());
    }
    match app.autolaunch().enable() {
        Ok(()) => {
            log(
                &app,
                Level::Info,
                "Écran d'accueil terminé : lancement au démarrage de Windows activé".into(),
            );
            Ok(())
        }
        Err(e) => {
            let msg = format!("Lancement au démarrage impossible : {e}");
            log(&app, Level::Warn, msg.clone());
            Err(msg)
        }
    }
}
