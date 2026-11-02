//! Orchestration du bridge : état affiché, synchronisation, file d'attente,
//! connexion, données poussées. L'application Tauri ne fait qu'appeler ces
//! méthodes (commandes, minuteries, surveillance) et afficher [`Status`].
//!
//! Aucun verrou n'est tenu pendant un appel réseau ou une attente disque.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Serialize;
use uuid::Uuid;

use crate::api::{ApiClient, ApiError, DeviceRegistration, SnapshotResponse, DEFAULT_BASE_URL};
use crate::auth::{self, DeviceSecretStore, TokenStore};
use crate::fsutil::{now_secs, read_json, write_json};
use crate::headline::{headline, Headline};
use crate::pushed::{
    addon_targets, data_files_present, write_pushed_data, PushedData, WriteOutcome,
};
use crate::queue::{QueuedSnapshot, RetryQueue, DEFAULT_CAPACITY};
use crate::snapshot::{build_snapshot, BuiltSnapshot, Snapshot, SnapshotError};
use crate::state::{AppPaths, Config, SendRecord, SyncState, TokenKind};
use crate::svread::{wait_all_stable, StabilityConfig};
use crate::wow::{
    accounts_dir, character_display_names, find_saved_variables, has_main_addon, list_flavors,
    GameDetector,
};

/// Nombre d'entrées gardées dans le journal.
pub const LOG_CAPACITY: usize = 50;

/// Réglages du moteur.
#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub data_dir: PathBuf,
    pub bridge_version: String,
    pub stability: StabilityConfig,
    pub parse_retry_delay: Duration,
    pub queue_capacity: usize,
    pub login_timeout: Duration,
}

impl EngineOptions {
    pub fn new(data_dir: &Path) -> Self {
        EngineOptions {
            data_dir: data_dir.to_path_buf(),
            bridge_version: crate::BRIDGE_VERSION.to_owned(),
            stability: StabilityConfig::default(),
            parse_retry_delay: Duration::from_secs(2),
            queue_capacity: DEFAULT_CAPACITY,
            login_timeout: Duration::from_secs(300),
        }
    }
}

/// État de la connexion au site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Connection {
    Disconnected,
    Connected {
        username: String,
        roles: Vec<String>,
        kind: TokenKind,
    },
    /// Token officier refusé (403/401) : effacé, reconnexion nécessaire.
    ReauthRequired {
        reason: String,
    },
    /// Appareil enregistré (contrat « vigies »), en attente de validation
    /// par un officier. Pas de token, aucune action possible.
    RegistrationPending,
    /// Appareil refusé ou coupé par les officiers : tout envoi reste
    /// bloqué tant qu'il n'est pas validé.
    RegistrationRefused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogEntry {
    pub at: i64,
    pub level: Level,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlavorInfo {
    pub name: String,
    pub path: String,
    pub accounts: usize,
    pub character_files: usize,
    pub addon_installed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WowInfo {
    pub root: Option<String>,
    /// Dossier choisi à la main (sinon détection automatique).
    pub manual: bool,
    pub flavors: Vec<FlavorInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueInfo {
    pub len: usize,
    pub oldest_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushedInfo {
    pub version: Option<String>,
    pub written_at: Option<i64>,
    /// Données reçues, en attente de la fermeture du jeu.
    pub waiting_game_closed: bool,
}

/// Tout ce qu'affiche l'interface.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub version: String,
    pub base_url: String,
    pub custom_base_url: bool,
    pub connection: Connection,
    /// Version minimale imposée par le site (réponse 426).
    pub update_required: Option<String>,
    pub wow: WowInfo,
    pub game_running: bool,
    /// Lecture ou envoi en cours.
    pub busy: bool,
    /// Le dernier échange avec le site a échoué faute de réponse (réseau,
    /// 5xx) ; repasse à `false` au premier échange réussi.
    pub site_unreachable: bool,
    pub last_send: Option<SendRecord>,
    pub queue: QueueInfo,
    pub pushed: PushedInfo,
    /// Du plus récent au plus ancien.
    pub log: Vec<LogEntry>,
    /// Phrase d'état principale, en mots simples.
    pub headline: Headline,
}

/// Bilan d'une synchronisation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub sent: usize,
    pub duplicates: usize,
    pub unchanged: usize,
    pub queued: usize,
    pub errors: usize,
}

/// Rappel « l'état a changé » (l'application relit [`Engine::status`]).
pub type Notifier = Arc<dyn Fn() + Send + Sync>;

enum SendOutcome {
    Sent,
    Duplicate,
    Queued,
    Failed,
}

struct Inner {
    config: Config,
    state: SyncState,
    queue: RetryQueue,
    log: VecDeque<LogEntry>,
    connection: Connection,
    update_required: Option<String>,
    api: ApiClient,
    auto_root: Option<PathBuf>,
    pending_pushed: Option<PushedData>,
    pushed_waiting: bool,
    site_unreachable: bool,
    /// État du jeu annoncé au dernier signal de vie réussi (`None` = aucun
    /// encore) : un changement déclenche un signal sans attendre 5 minutes.
    last_heartbeat_game: Option<bool>,
}

impl Inner {
    fn push_log(&mut self, level: Level, message: String) {
        self.log.push_back(LogEntry {
            at: now_secs(),
            level,
            message,
        });
        while self.log.len() > LOG_CAPACITY {
            self.log.pop_front();
        }
    }
}

/// Moteur du bridge.
pub struct Engine {
    opts: EngineOptions,
    paths: AppPaths,
    store: Arc<dyn TokenStore>,
    device_secret_store: Arc<dyn DeviceSecretStore>,
    detector: Arc<dyn GameDetector>,
    inner: Mutex<Inner>,
    /// Sérialise synchronisations et vidages de file.
    sync_lock: tokio::sync::Mutex<()>,
    /// Lecture ou envoi en cours (affichage).
    busy: AtomicBool,
    notifier: Notifier,
}

/// Marque le moteur occupé jusqu'à sa destruction.
struct BusyGuard<'a>(&'a Engine);

impl<'a> BusyGuard<'a> {
    fn new(engine: &'a Engine) -> Self {
        engine.busy.store(true, Ordering::SeqCst);
        engine.notify();
        BusyGuard(engine)
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::SeqCst);
        self.0.notify();
    }
}

fn api_for(config: &Config, version: &str) -> (ApiClient, Option<String>) {
    if let Some(url) = &config.base_url {
        match ApiClient::new(url, version) {
            Ok(c) => return (c, None),
            Err(e) => {
                let c =
                    ApiClient::new(DEFAULT_BASE_URL, version).expect("adresse par défaut valide");
                return (c, Some(format!("{e} : retour au site par défaut")));
            }
        }
    }
    (
        ApiClient::new(DEFAULT_BASE_URL, version).expect("adresse par défaut valide"),
        None,
    )
}

impl Engine {
    /// Charge réglages, état et file depuis `opts.data_dir`. `auto_root` est
    /// la racine WoW détectée automatiquement (registre), si connue.
    pub fn new(
        opts: EngineOptions,
        store: Arc<dyn TokenStore>,
        device_secret_store: Arc<dyn DeviceSecretStore>,
        detector: Arc<dyn GameDetector>,
        auto_root: Option<PathBuf>,
        notifier: Notifier,
    ) -> Self {
        let paths = AppPaths::new(&opts.data_dir);
        let mut startup_log: Vec<(Level, String)> = Vec::new();
        if let Err(e) = std::fs::create_dir_all(&paths.data_dir) {
            startup_log.push((
                Level::Error,
                format!("Dossier de données inaccessible : {e}"),
            ));
        }
        let config: Option<Config> = read_json(&paths.config()).unwrap_or_else(|e| {
            startup_log.push((
                Level::Warn,
                format!("Réglages illisibles, valeurs par défaut : {e}"),
            ));
            None
        });
        let mut config = config.unwrap_or_default();
        if config.device_id.is_none() {
            config.device_id = Some(Uuid::new_v4().to_string());
            if let Err(e) = write_json(&paths.config(), &config) {
                startup_log.push((
                    Level::Warn,
                    format!("Identifiant d'appareil non sauvegardé : {e}"),
                ));
            }
        }
        let state: Option<SyncState> = read_json(&paths.state()).unwrap_or_else(|e| {
            startup_log.push((Level::Warn, format!("État illisible, réinitialisé : {e}")));
            None
        });
        let mut state = state.unwrap_or_default();
        let (queue, qerr) = RetryQueue::load(&paths.queue(), opts.queue_capacity);
        if let Some(e) = qerr {
            startup_log.push((
                Level::Warn,
                format!("File d'attente illisible, vidée : {e}"),
            ));
        }
        let (api, api_warn) = api_for(&config, &opts.bridge_version);
        if let Some(w) = api_warn {
            startup_log.push((Level::Warn, w));
        }
        let token = match store.load() {
            Ok(t) => t,
            Err(e) => {
                startup_log.push((Level::Warn, e.to_string()));
                None
            }
        };
        let connection = match (token, &state.user, state.token_kind) {
            (Some(_), Some(user), _) => Connection::Connected {
                username: user.username.clone(),
                roles: user.roles.clone(),
                kind: TokenKind::Officer,
            },
            (Some(_), None, Some(TokenKind::Vigie)) => Connection::Connected {
                username: "Vigie".into(),
                roles: Vec::new(),
                kind: TokenKind::Vigie,
            },
            (Some(_), None, _) => Connection::Connected {
                username: "?".into(),
                roles: Vec::new(),
                kind: TokenKind::Officer,
            },
            (None, _, _) => {
                state.user = None;
                state.token_kind = None;
                Connection::Disconnected
            }
        };
        let mut inner = Inner {
            config,
            state,
            queue,
            log: VecDeque::new(),
            connection,
            update_required: None,
            api,
            auto_root,
            pending_pushed: None,
            pushed_waiting: false,
            site_unreachable: false,
            last_heartbeat_game: None,
        };
        for (level, msg) in startup_log {
            inner.push_log(level, msg);
        }
        Engine {
            opts,
            paths,
            store,
            device_secret_store,
            detector,
            inner: Mutex::new(inner),
            sync_lock: tokio::sync::Mutex::new(()),
            busy: AtomicBool::new(false),
            notifier,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn notify(&self) {
        (self.notifier)();
    }

    /// Ajoute une entrée au journal (ne pas appeler verrou tenu).
    pub fn log(&self, level: Level, message: impl Into<String>) {
        self.lock().push_log(level, message.into());
        self.notify();
    }

    fn save_state(inner: &mut Inner, paths: &AppPaths) {
        if let Err(e) = write_json(&paths.state(), &inner.state) {
            inner.push_log(
                Level::Error,
                format!("Sauvegarde de l'état impossible : {e}"),
            );
        }
    }

    // --- Réglages ----------------------------------------------------------

    /// Racine WoW utilisée : choix manuel, sinon détection automatique.
    pub fn install_root(&self) -> Option<PathBuf> {
        let inner = self.lock();
        inner
            .config
            .wow_dir
            .clone()
            .or_else(|| inner.auto_root.clone())
    }

    /// Dossiers des versions du client trouvées.
    pub fn flavor_dirs(&self) -> Vec<PathBuf> {
        self.install_root()
            .map(|r| list_flavors(&r).into_iter().map(|f| f.path).collect())
            .unwrap_or_default()
    }

    /// Dossiers à surveiller (`WTF/Account` de chaque version).
    pub fn watch_dirs(&self) -> Vec<PathBuf> {
        self.flavor_dirs().iter().map(|f| accounts_dir(f)).collect()
    }

    /// Choisit le dossier WoW (`None` = détection automatique).
    pub fn set_wow_dir(&self, dir: Option<PathBuf>) -> Result<(), String> {
        if let Some(d) = &dir {
            if list_flavors(d).is_empty() {
                return Err(format!(
                    "Aucune installation WoW (dossier WTF/Account) dans {}",
                    d.display()
                ));
            }
        }
        {
            let mut inner = self.lock();
            inner.config.wow_dir = dir.clone();
            if let Err(e) = write_json(&self.paths.config(), &inner.config) {
                return Err(format!("Sauvegarde des réglages impossible : {e}"));
            }
            let msg = match &dir {
                Some(d) => format!("Dossier WoW choisi : {}", d.display()),
                None => "Dossier WoW : détection automatique".to_owned(),
            };
            inner.push_log(Level::Info, msg);
        }
        self.notify();
        Ok(())
    }

    /// Change l'adresse du site (`None` = production). Le token appartient
    /// à l'ancien site : il est effacé.
    pub fn set_base_url(&self, url: Option<String>) -> Result<(), String> {
        let url = url.map(|u| u.trim().to_owned()).filter(|u| !u.is_empty());
        let api = ApiClient::new(
            url.as_deref().unwrap_or(DEFAULT_BASE_URL),
            &self.opts.bridge_version,
        )
        .map_err(|e| e.to_string())?;
        let changed = {
            let mut inner = self.lock();
            let changed = inner.api.base_url() != api.base_url();
            inner.config.base_url = url.map(|_| api.base_url().to_owned());
            inner.api = api;
            write_json(&self.paths.config(), &inner.config).map_err(|e| e.to_string())?;
            changed
        };
        if changed {
            self.logout();
            let base = self.api().base_url().to_owned();
            self.log(Level::Info, format!("Site : {base}"));
        }
        Ok(())
    }

    pub fn api(&self) -> ApiClient {
        self.lock().api.clone()
    }

    /// Token courant (gestionnaire d'identifiants).
    pub fn token(&self) -> Option<String> {
        self.store.load().ok().flatten()
    }

    pub fn update_required(&self) -> Option<String> {
        self.lock().update_required.clone()
    }

    // --- État --------------------------------------------------------------

    /// État complet pour l'interface (parcourt les dossiers WoW).
    pub fn status(&self) -> Status {
        let (root, manual) = {
            let inner = self.lock();
            (
                inner
                    .config
                    .wow_dir
                    .clone()
                    .or_else(|| inner.auto_root.clone()),
                inner.config.wow_dir.is_some(),
            )
        };
        let flavors = root
            .as_deref()
            .map(|r| {
                list_flavors(r)
                    .into_iter()
                    .map(|f| {
                        let accounts = find_saved_variables(&f.path);
                        FlavorInfo {
                            name: f.name.to_string_lossy().into_owned(),
                            path: f.path.display().to_string(),
                            accounts: accounts.iter().filter(|a| a.account_file.is_some()).count(),
                            character_files: accounts.iter().map(|a| a.characters.len()).sum(),
                            addon_installed: has_main_addon(&f.path),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let game_running = self.detector.is_game_running();
        let inner = self.lock();
        let mut status = Status {
            version: self.opts.bridge_version.clone(),
            base_url: inner.api.base_url().to_owned(),
            custom_base_url: inner.config.base_url.is_some(),
            connection: inner.connection.clone(),
            update_required: inner.update_required.clone(),
            wow: WowInfo {
                root: root.map(|r| r.display().to_string()),
                manual,
                flavors,
            },
            game_running,
            busy: self.busy.load(Ordering::SeqCst),
            site_unreachable: inner.site_unreachable,
            last_send: inner.state.last_send.clone(),
            queue: QueueInfo {
                len: inner.queue.len(),
                oldest_at: inner.queue.items().map(|i| i.queued_at).min(),
                last_error: inner.queue.front().and_then(|i| i.last_error.clone()),
            },
            pushed: PushedInfo {
                version: inner.state.pushed_version.clone(),
                written_at: inner.state.pushed_written_at,
                waiting_game_closed: inner.pending_pushed.is_some() && inner.pushed_waiting,
            },
            log: inner.log.iter().rev().cloned().collect(),
            headline: Headline::default(),
        };
        status.headline = headline(&status, now_secs());
        status
    }

    /// Note le résultat d'un échange avec le site : injoignable (réseau,
    /// 5xx…) ou non. Prévient l'interface seulement si ça change.
    fn set_site_unreachable(&self, unreachable: bool) {
        let changed = {
            let mut inner = self.lock();
            let changed = inner.site_unreachable != unreachable;
            inner.site_unreachable = unreachable;
            changed
        };
        if changed {
            self.notify();
        }
    }

    // --- Connexion ---------------------------------------------------------

    /// Connexion navigateur + loopback. `open_url` ouvre le navigateur.
    pub async fn login<F>(&self, open_url: F) -> Result<String, String>
    where
        F: FnOnce(&str) -> Result<(), String>,
    {
        let api = self.api();
        self.log(
            Level::Info,
            "Connexion : autorisez le bridge dans le navigateur",
        );
        match auth::login(
            &api,
            &*self.store,
            &auth::device_name(),
            self.opts.login_timeout,
            open_url,
        )
        .await
        {
            Ok(user) => {
                let username = user.username.clone();
                {
                    let mut inner = self.lock();
                    inner.connection = Connection::Connected {
                        username: user.username.clone(),
                        roles: user.roles.clone(),
                        kind: TokenKind::Officer,
                    };
                    inner.state.user = Some(user);
                    inner.state.token_kind = Some(TokenKind::Officer);
                    inner.state.pushed_collector = None;
                    Self::save_state(&mut inner, &self.paths);
                    inner.push_log(Level::Info, format!("Connecté en tant que {username}"));
                }
                self.notify();
                Ok(username)
            }
            Err(e) => {
                if let auth::AuthError::Api(api_err) = &e {
                    self.handle_api_error(api_err);
                }
                let msg = format!("Connexion échouée : {e}");
                self.log(Level::Error, msg.clone());
                Err(msg)
            }
        }
    }

    /// Déconnexion : efface le token.
    pub fn logout(&self) {
        let res = self.store.clear();
        {
            let mut inner = self.lock();
            if let Err(e) = res {
                inner.push_log(Level::Error, e.to_string());
            }
            inner.state.user = None;
            inner.state.token_kind = None;
            inner.state.pushed_collector = None;
            inner.connection = Connection::Disconnected;
            Self::save_state(&mut inner, &self.paths);
            inner.push_log(Level::Info, "Déconnecté".to_owned());
        }
        self.notify();
    }

    /// 401/403 → token effacé, reconnexion demandée ; 426 → mise à jour
    /// obligatoire. Renvoie `true` si l'erreur a été prise en charge.
    fn handle_api_error(&self, e: &ApiError) -> bool {
        match e {
            ApiError::AuthRequired { message, .. } => {
                let res = self.store.clear();
                {
                    let mut inner = self.lock();
                    if let Err(err) = res {
                        inner.push_log(Level::Error, err.to_string());
                    }
                    let was_vigie = inner.state.token_kind == Some(TokenKind::Vigie);
                    inner.state.user = None;
                    inner.state.pushed_collector = None;
                    inner.state.token_kind = None;
                    if was_vigie {
                        // Contrat « vigies » : le middleware révoque le token
                        // d'un appareil qui n'est plus validé ; une nouvelle
                        // demande d'enregistrement reprend au prochain cycle.
                        inner.connection = Connection::RegistrationPending;
                        inner.push_log(
                            Level::Error,
                            "Accès retiré par les officiers : nouvelle demande d'enregistrement"
                                .to_owned(),
                        );
                    } else {
                        inner.connection = Connection::ReauthRequired {
                            reason: message.clone(),
                        };
                        inner.push_log(
                            Level::Error,
                            "Accès refusé par le site : token effacé, reconnectez-vous".to_owned(),
                        );
                    }
                    Self::save_state(&mut inner, &self.paths);
                }
                self.notify();
                true
            }
            ApiError::Outdated { min_version } => {
                {
                    let mut inner = self.lock();
                    let min = min_version.clone().unwrap_or_else(|| "?".into());
                    if inner.update_required.as_deref() != Some(min.as_str()) {
                        inner.push_log(
                            Level::Error,
                            format!("Mise à jour obligatoire (version minimale {min})"),
                        );
                    }
                    inner.update_required = Some(min);
                }
                self.notify();
                true
            }
            _ => false,
        }
    }

    // --- Enregistrement d'appareil (contrat « vigies ») ---------------------

    fn set_registration_connection(&self, connection: Connection, log_message: String) {
        let changed = {
            let mut inner = self.lock();
            let changed = inner.connection != connection;
            if changed {
                inner.connection = connection;
                inner.push_log(Level::Info, log_message);
            }
            changed
        };
        if changed {
            self.notify();
        }
    }

    /// Nouveau `device_id` et nouveau secret : l'ancien identifiant reste
    /// définitivement `device_secret_mismatch` côté site.
    fn reset_device_identity(&self) -> Result<(), String> {
        let secret = auth::generate_device_secret();
        self.device_secret_store
            .save(&secret)
            .map_err(|e| e.to_string())?;
        let mut inner = self.lock();
        inner.config.device_id = Some(Uuid::new_v4().to_string());
        write_json(&self.paths.config(), &inner.config).map_err(|e| e.to_string())
    }

    /// Enregistrement sans connexion (appelé au démarrage, toutes les 5
    /// minutes et à chaque changement de SavedVariables) : sans effet si un
    /// token est déjà présent (officier, membre ou vigie approuvée), tant
    /// qu'une mise à jour obligatoire bloque les routes de données, ou —
    /// contrat étape 1 — pour un bridge qui n'a encore jamais commencé le
    /// flux « vigie » : plus d'enregistrement anonyme (`POST
    /// /bridge/devices`) pour un nouveau bridge, rien n'est envoyé avant la
    /// connexion. Seul un appareil qui a déjà un secret stocké (créé avant
    /// cette version, ou toujours en attente de validation) continue comme
    /// avant ; un nouveau secret n'est plus jamais créé ici.
    pub async fn register_if_needed(&self) {
        if self.update_required().is_some() || self.token().is_some() {
            return;
        }
        let secret = match self.device_secret_store.load() {
            Ok(Some(s)) => s,
            Ok(None) => return,
            Err(e) => {
                self.log(Level::Error, format!("Secret d'appareil : {e}"));
                return;
            }
        };
        let Some(device_id) = self.lock().config.device_id.clone() else {
            self.log(Level::Error, "Identifiant d'appareil manquant".to_owned());
            return;
        };
        let characters = character_display_names(&self.flavor_dirs());
        // Contrat « vigies » : `device_name` ≤ 64 caractères.
        let device_name: String = auth::device_name().chars().take(64).collect();
        let api = self.api();
        let result = api
            .register_device(
                &device_id,
                &secret,
                &device_name,
                &characters,
                &self.opts.bridge_version,
            )
            .await;
        match result {
            Ok(DeviceRegistration::Pending) => {
                self.set_site_unreachable(false);
                self.set_registration_connection(
                    Connection::RegistrationPending,
                    "Appareil enregistré : en attente de validation par un officier".to_owned(),
                );
            }
            Ok(DeviceRegistration::Refused) => {
                self.set_site_unreachable(false);
                self.set_registration_connection(
                    Connection::RegistrationRefused,
                    "Appareil refusé par les officiers : aucun envoi tant qu'il n'est pas validé"
                        .to_owned(),
                );
            }
            Ok(DeviceRegistration::SecretMismatch) => {
                self.set_site_unreachable(false);
                match self.reset_device_identity() {
                    Ok(()) => self.log(
                        Level::Warn,
                        "Identité d'appareil réinitialisée (secret invalide pour le site)"
                            .to_owned(),
                    ),
                    Err(e) => self.log(Level::Error, format!("Identité d'appareil : {e}")),
                }
            }
            Ok(DeviceRegistration::Approved { token }) => {
                self.set_site_unreachable(false);
                if let Err(e) = self.store.save(&token) {
                    self.log(Level::Error, format!("Enregistrement du token : {e}"));
                    return;
                }
                {
                    let mut inner = self.lock();
                    inner.state.user = None;
                    inner.state.pushed_collector = None;
                    inner.state.token_kind = Some(TokenKind::Vigie);
                    inner.connection = Connection::Connected {
                        username: "Vigie".into(),
                        roles: Vec::new(),
                        kind: TokenKind::Vigie,
                    };
                    Self::save_state(&mut inner, &self.paths);
                    inner.push_log(
                        Level::Info,
                        "Appareil validé par les officiers : vigie connectée".to_owned(),
                    );
                }
                self.notify();
            }
            Err(e) => {
                if e.is_retriable() {
                    self.set_site_unreachable(true);
                }
                self.log(Level::Warn, format!("Enregistrement de l'appareil : {e}"));
            }
        }
    }

    // --- Envoi des snapshots -----------------------------------------------

    /// Relit toutes les SavedVariables, envoie ce qui a changé (ou le met en
    /// file d'attente). Attend la stabilité des fichiers avant de les lire.
    pub async fn sync_all(&self, reason: &str) -> SyncReport {
        let _guard = self.sync_lock.lock().await;
        let _busy = BusyGuard::new(self);
        let mut report = SyncReport::default();
        let Some(root) = self.install_root() else {
            self.log(
                Level::Warn,
                "Dossier WoW introuvable : choisissez-le dans les réglages",
            );
            return report;
        };
        self.log(
            Level::Info,
            format!("Lecture des SavedVariables ({reason})"),
        );
        let flavors = list_flavors(&root);
        let game_running = self.detector.is_game_running();
        let stability = self.opts.stability;
        let retry = self.opts.parse_retry_delay;
        let version = self.opts.bridge_version.clone();
        let built = tokio::task::spawn_blocking(move || {
            let mut out: Vec<Result<BuiltSnapshot, String>> = Vec::new();
            for f in flavors {
                for acc in find_saved_variables(&f.path) {
                    let files: Vec<PathBuf> = acc.all_files().map(Path::to_path_buf).collect();
                    if let Err(e) = wait_all_stable(&files, stability) {
                        out.push(Err(format!("{} : {e}", acc.account_dir.display())));
                        continue;
                    }
                    match build_snapshot(&root, &f.path, &acc, game_running, retry, &version) {
                        Ok(b) => out.push(Ok(b)),
                        Err(SnapshotError::NoAccountFile(_)) => {}
                        Err(e) => out.push(Err(e.to_string())),
                    }
                }
            }
            out
        })
        .await
        .unwrap_or_else(|e| vec![Err(format!("lecture interrompue : {e}"))]);

        for item in built {
            let mut b = match item {
                Ok(b) => b,
                Err(e) => {
                    report.errors += 1;
                    self.log(Level::Error, e);
                    continue;
                }
            };
            // Membre simple (contrat 2026-11-02 b) : seulement les preuves de
            // triche ; le hash suit le contenu filtré.
            if self.lock().state.is_plain_member() {
                b.snapshot = b.snapshot.for_member();
            }
            for w in &b.warnings {
                self.log(Level::Warn, w.clone());
            }
            if b.snapshot.guild_count() == 0 {
                self.log(Level::Info, format!("{} : aucune guilde GSF", b.key));
                continue;
            }
            if let Some(key) = b.snapshot.active_guild_key() {
                let mut inner = self.lock();
                if inner.state.active_guild_key.as_deref() != Some(key.as_str()) {
                    inner.state.active_guild_key = Some(key);
                    Self::save_state(&mut inner, &self.paths);
                }
            }
            let (already_sent, already_queued) = {
                let inner = self.lock();
                let h = b.snapshot.snapshot_hash.as_str();
                (
                    inner.state.last_hashes.get(&b.key).map(String::as_str) == Some(h),
                    inner.queue.pending_hash(&b.key) == Some(h),
                )
            };
            if already_sent || already_queued {
                report.unchanged += 1;
                self.log(Level::Info, format!("{} : inchangé, rien à envoyer", b.key));
                continue;
            }
            match self.send_or_queue(&b.key, b.snapshot).await {
                SendOutcome::Sent => report.sent += 1,
                SendOutcome::Duplicate => report.duplicates += 1,
                SendOutcome::Queued => report.queued += 1,
                SendOutcome::Failed => report.errors += 1,
            }
        }
        report
    }

    fn record_success(
        &self,
        key: &str,
        snapshot: &Snapshot,
        resp: &SnapshotResponse,
    ) -> SendOutcome {
        let duplicate = resp.is_duplicate();
        let ignored_all = resp.is_ignored();
        {
            let mut inner = self.lock();
            inner
                .state
                .last_hashes
                .insert(key.to_owned(), snapshot.snapshot_hash.clone());
            let (status, message) = if duplicate {
                ("duplicate", format!("{key} : déjà reçu par le site"))
            } else if ignored_all {
                (
                    "ignored",
                    format!("{key} : aucune guilde suivie par le site dans cet envoi"),
                )
            } else {
                (
                    "accepted",
                    format!("{key} : {} guilde(s) envoyée(s)", snapshot.guild_count()),
                )
            };
            inner.state.last_send = Some(SendRecord {
                at: now_secs(),
                status: status.into(),
                message: message.clone(),
            });
            inner.update_required = None;
            inner.site_unreachable = false;
            Self::save_state(&mut inner, &self.paths);
            if let Err(e) = inner.queue.remove_key(key) {
                inner.push_log(Level::Error, format!("File d'attente : {e}"));
            }
            inner.push_log(Level::Info, message);
            let ignored = resp.ignored();
            if !ignored.is_empty() {
                inner.push_log(
                    Level::Warn,
                    format!(
                        "Guildes non suivies par le site, ignorées : {}",
                        ignored.join(", ")
                    ),
                );
            }
        }
        self.notify();
        if duplicate {
            SendOutcome::Duplicate
        } else {
            SendOutcome::Sent
        }
    }

    fn enqueue(&self, key: &str, snapshot: Snapshot, reason: &str) -> SendOutcome {
        {
            let mut inner = self.lock();
            let item = QueuedSnapshot {
                key: key.to_owned(),
                queued_at: now_secs(),
                attempts: 0,
                last_error: Some(reason.to_owned()),
                snapshot,
            };
            match inner.queue.push(item) {
                Ok(dropped) => {
                    for d in dropped {
                        inner.push_log(
                            Level::Warn,
                            format!("File pleine : envoi de {} abandonné", d.key),
                        );
                    }
                }
                Err(e) => inner.push_log(Level::Error, format!("File d'attente : {e}")),
            }
            inner.state.last_send = Some(SendRecord {
                at: now_secs(),
                status: "queued".into(),
                message: format!("{key} : en attente ({reason})"),
            });
            Self::save_state(&mut inner, &self.paths);
            inner.push_log(
                Level::Warn,
                format!("{key} : mis en file d'attente ({reason})"),
            );
        }
        self.notify();
        SendOutcome::Queued
    }

    async fn send_or_queue(&self, key: &str, snapshot: Snapshot) -> SendOutcome {
        if let Some(min) = self.update_required() {
            return self.enqueue(key, snapshot, &format!("mise à jour {min} obligatoire"));
        }
        let Some(token) = self.token() else {
            return self.enqueue(key, snapshot, "non connecté");
        };
        let api = self.api();
        match api.post_snapshot(&token, &snapshot).await {
            Ok(resp) => self.record_success(key, &snapshot, &resp),
            Err(e) if self.handle_api_error(&e) || e.is_retriable() => {
                if e.is_retriable() {
                    self.set_site_unreachable(true);
                }
                self.enqueue(key, snapshot, &e.to_string())
            }
            Err(e) => {
                // Refus définitif (422, 413…) : inutile de réessayer tel quel.
                {
                    let mut inner = self.lock();
                    inner.state.last_send = Some(SendRecord {
                        at: now_secs(),
                        status: "error".into(),
                        message: format!("{key} : {e}"),
                    });
                    Self::save_state(&mut inner, &self.paths);
                    inner.push_log(Level::Error, format!("{key} : envoi refusé : {e}"));
                }
                self.notify();
                SendOutcome::Failed
            }
        }
    }

    /// Renvoie les snapshots en attente, dans l'ordre, tant que ça passe.
    /// Renvoie le nombre d'éléments retirés de la file.
    pub async fn flush_queue(&self) -> usize {
        let _guard = self.sync_lock.lock().await;
        let mut busy_guard: Option<BusyGuard<'_>> = None;
        let mut done = 0;
        loop {
            let front = {
                let inner = self.lock();
                if inner.update_required.is_some() {
                    break;
                }
                match inner.queue.front() {
                    Some(f) => f.clone(),
                    None => break,
                }
            };
            let Some(token) = self.token() else { break };
            if busy_guard.is_none() {
                busy_guard = Some(BusyGuard::new(self));
            }
            let api = self.api();
            // Un envoi mis en file avant la connexion d'un membre simple est
            // filtré au moment de partir (filtre idempotent).
            let snapshot = if self.lock().state.is_plain_member() {
                front.snapshot.for_member()
            } else {
                front.snapshot.clone()
            };
            match api.post_snapshot(&token, &snapshot).await {
                Ok(resp) => {
                    self.record_success(&front.key, &snapshot, &resp);
                    done += 1;
                }
                Err(e) if self.handle_api_error(&e) || e.is_retriable() => {
                    if e.is_retriable() {
                        self.set_site_unreachable(true);
                    }
                    if let Err(err) = self.lock().queue.mark_front_failed(&e.to_string()) {
                        self.log(Level::Error, format!("File d'attente : {err}"));
                    }
                    self.notify();
                    break;
                }
                Err(e) => {
                    {
                        let mut inner = self.lock();
                        if let Err(err) = inner.queue.pop_front() {
                            inner.push_log(Level::Error, format!("File d'attente : {err}"));
                        }
                        inner.push_log(
                            Level::Error,
                            format!("{} : envoi en attente abandonné : {e}", front.key),
                        );
                    }
                    self.notify();
                    done += 1;
                }
            }
        }
        done
    }

    // --- Données poussées --------------------------------------------------

    /// Télécharge les données poussées et les écrit si le jeu est fermé
    /// (sinon elles attendent la fermeture, cf. [`Engine::tick`]).
    pub async fn refresh_pushed_data(&self) -> Option<WriteOutcome> {
        if self.update_required().is_some() {
            return None;
        }
        let token = self.token()?;
        let (active_guild_key, known) = {
            let inner = self.lock();
            // « Seulement si changé » (contrat 2026-11-02 b) : seulement si la
            // version écrite est connue, que rien n'attend la fermeture du jeu
            // et que le fichier de données existe partout (sinon requête
            // complète, pour le réécrire).
            let known = inner
                .state
                .pushed_version
                .clone()
                .filter(|_| inner.pending_pushed.is_none());
            (inner.state.active_guild_key.clone(), known)
        };
        let known = known.filter(|_| data_files_present(&addon_targets(&self.flavor_dirs())));
        match self
            .api()
            .pushed_data_if_changed(&token, active_guild_key.as_deref(), known.as_deref())
            .await
        {
            Ok(None) => {
                self.set_site_unreachable(false);
                Some(WriteOutcome::Unchanged)
            }
            Ok(Some(data)) => {
                self.set_site_unreachable(false);
                {
                    let mut inner = self.lock();
                    if inner.state.pushed_collector != Some(data.collector) {
                        inner.state.pushed_collector = Some(data.collector);
                        Self::save_state(&mut inner, &self.paths);
                    }
                    inner.pending_pushed = Some(data);
                }
                self.write_pending_pushed()
            }
            Err(ApiError::Http { status: 409, .. }) => {
                self.log(
                    Level::Info,
                    "Données poussées : aucune guilde suivie n'est réglée sur le site",
                );
                None
            }
            Err(ApiError::Http {
                status: 404,
                ref code,
                ..
            }) if code == "guild_not_tracked" => {
                self.set_site_unreachable(false);
                self.log(
                    Level::Warn,
                    format!(
                        "Données poussées : la guilde « {} » n'est pas suivie par le site, rien n'est écrit",
                        active_guild_key.as_deref().unwrap_or("?")
                    ),
                );
                None
            }
            Err(e) => {
                if e.is_retriable() {
                    self.set_site_unreachable(true);
                }
                if !self.handle_api_error(&e) {
                    self.log(Level::Warn, format!("Données poussées indisponibles : {e}"));
                }
                None
            }
        }
    }

    /// Écrit les données poussées en attente si le jeu est fermé.
    pub fn write_pending_pushed(&self) -> Option<WriteOutcome> {
        let data = self.lock().pending_pushed.clone()?;
        let targets = addon_targets(&self.flavor_dirs());
        let result = write_pushed_data(&targets, &data, &*self.detector, &self.opts.bridge_version);
        {
            let mut inner = self.lock();
            match &result {
                Ok((outcome, warnings)) => {
                    match outcome {
                        WriteOutcome::GameRunning => {
                            if !inner.pushed_waiting {
                                inner.push_log(
                                    Level::Info,
                                    "Jeu lancé : données poussées écrites à sa fermeture".to_owned(),
                                );
                            }
                            inner.pushed_waiting = true;
                            drop(inner);
                            self.notify();
                            return Some(WriteOutcome::GameRunning);
                        }
                        WriteOutcome::Written(dirs) => {
                            for w in warnings {
                                inner.push_log(Level::Warn, w.clone());
                            }
                            inner.state.pushed_version = data.version.clone();
                            inner.state.pushed_written_at = Some(now_secs());
                            Self::save_state(&mut inner, &self.paths);
                            let list = dirs
                                .iter()
                                .map(|d| d.display().to_string())
                                .collect::<Vec<_>>()
                                .join(", ");
                            inner.push_log(
                                Level::Info,
                                format!("Données poussées écrites ({} ban(s)) : {list}", data.bans.len()),
                            );
                        }
                        WriteOutcome::Unchanged => {
                            if inner.state.pushed_version != data.version {
                                inner.state.pushed_version = data.version.clone();
                                Self::save_state(&mut inner, &self.paths);
                            }
                        }
                        WriteOutcome::NoTarget => inner.push_log(
                            Level::Warn,
                            "Données poussées : addon GuildSelfForever introuvable dans Interface/AddOns"
                                .to_owned(),
                        ),
                    }
                }
                Err(e) => inner.push_log(Level::Error, format!("Données poussées : {e}")),
            }
            inner.pending_pushed = None;
            inner.pushed_waiting = false;
        }
        self.notify();
        result.ok().map(|(o, _)| o)
    }

    // --- Signal de vie (contrat 2026-11-02, bridge pour tous étape 2) -------

    /// Signal de vie léger : « ce bridge est lancé, le jeu est ouvert ou
    /// non ». Envoyé quel que soit le niveau de confiance du jeton (officier,
    /// vigie, membre) : le site en déduit lui-même le niveau. Sans effet sans
    /// jeton ou tant qu'une mise à jour obligatoire bloque les routes de
    /// données. Un échec n'est jamais mis en file : le suivant le remplace.
    pub async fn heartbeat(&self) {
        if self.update_required().is_some() {
            return;
        }
        let Some(token) = self.token() else { return };
        let game_running = self.detector.is_game_running();
        let guild_key = self.lock().state.active_guild_key.clone();
        match self
            .api()
            .heartbeat(&token, game_running, guild_key.as_deref())
            .await
        {
            Ok(()) => {
                self.set_site_unreachable(false);
                self.lock().last_heartbeat_game = Some(game_running);
            }
            Err(e) => {
                if e.is_retriable() {
                    self.set_site_unreachable(true);
                }
                if !self.handle_api_error(&e) {
                    self.log(Level::Warn, format!("Signal de vie : {e}"));
                }
            }
        }
    }

    /// Signal de vie immédiat si le jeu vient d'être ouvert ou fermé depuis
    /// le dernier signal réussi (appelé chaque minute par [`Engine::tick`]).
    pub async fn heartbeat_if_game_changed(&self) {
        let last = self.lock().last_heartbeat_game;
        if last.is_some_and(|g| g != self.detector.is_game_running()) {
            self.heartbeat().await;
        }
    }

    /// Tâche périodique : vide la file d'attente, écrit les données
    /// poussées en attente si le jeu a été fermé, et prévient le site si le
    /// jeu vient d'être ouvert ou fermé.
    pub async fn tick(&self) {
        self.flush_queue().await;
        self.write_pending_pushed();
        self.heartbeat_if_game_changed().await;
    }
}
