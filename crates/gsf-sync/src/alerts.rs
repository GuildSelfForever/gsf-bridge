//! Notifications Windows : seulement quand l'officier doit agir, une seule
//! fois par changement d'état (jamais de répétition tant que rien ne change).
//!
//! [`AlertTracker`] compare chaque [`Status`] au précédent et ne rend que les
//! alertes qui viennent d'apparaître. Logique pure, testable sans fenêtre.

use crate::engine::{Connection, Status};

/// Délai avant de signaler un site injoignable (la file d'attente couvre les
/// coupures courtes sans déranger personne).
pub const SITE_UNREACHABLE_GRACE_SECS: i64 = 10 * 60;

/// Situation qui mérite une notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    /// Le site a refusé le token : il faut se reconnecter.
    Reconnect,
    /// Plus aucune réponse du site depuis [`SITE_UNREACHABLE_GRACE_SECS`].
    SiteUnreachable,
    /// Connecté, mais aucun dossier WoW lisible.
    WowNotFound,
    /// Le site n'accepte plus cette version (426).
    MandatoryUpdate,
    /// Une nouvelle version est proposée.
    UpdateAvailable { version: String },
}

impl Alert {
    pub fn title(&self) -> String {
        match self {
            Alert::Reconnect => "Reconnexion nécessaire".into(),
            Alert::SiteUnreachable => "Site injoignable".into(),
            Alert::WowNotFound => "WoW introuvable".into(),
            Alert::MandatoryUpdate => "Mise à jour obligatoire".into(),
            Alert::UpdateAvailable { .. } => "GSF Bridge".into(),
        }
    }

    pub fn body(&self) -> String {
        match self {
            Alert::Reconnect => {
                "Le site demande de te reconnecter. Clique pour ouvrir GSF Bridge.".into()
            }
            Alert::SiteUnreachable => {
                "Le site ne répond plus depuis 10 minutes. Rien n'est perdu : j'enverrai les données dès qu'il revient."
                    .into()
            }
            Alert::WowNotFound => {
                "Je ne trouve plus World of Warcraft. Clique pour choisir son dossier.".into()
            }
            Alert::MandatoryUpdate => {
                "Plus rien n'est envoyé tant que GSF Bridge n'est pas à jour. Clique pour mettre à jour."
                    .into()
            }
            Alert::UpdateAvailable { version } => {
                format!("Nouvelle version {version} disponible — clique pour mettre à jour")
            }
        }
    }

    /// L'alerte demande d'ouvrir la fenêtre de mise à jour.
    pub fn opens_update(&self) -> bool {
        matches!(self, Alert::MandatoryUpdate | Alert::UpdateAvailable { .. })
    }
}

/// Mémoire des alertes déjà signalées.
#[derive(Debug, Default)]
pub struct AlertTracker {
    reconnect: bool,
    site_unreachable: bool,
    wow_not_found: bool,
    mandatory: bool,
    /// Début de la coupure en cours (secondes Unix).
    unreachable_since: Option<i64>,
    /// Dernière version déjà annoncée.
    announced_version: Option<String>,
}

impl AlertTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compare `status` (à l'instant `now`, secondes Unix) au précédent et
    /// rend les alertes qui viennent d'apparaître. Une alerte disparue puis
    /// revenue est signalée de nouveau.
    pub fn observe(&mut self, status: &Status, now: i64) -> Vec<Alert> {
        let mut out = Vec::new();

        let mandatory = status.update_required.is_some();
        if rising(&mut self.mandatory, mandatory) {
            out.push(Alert::MandatoryUpdate);
        }

        let reconnect = matches!(status.connection, Connection::ReauthRequired { .. });
        if rising(&mut self.reconnect, reconnect) {
            out.push(Alert::Reconnect);
        }

        let connected = matches!(status.connection, Connection::Connected { .. });
        let wow_missing = status.wow.root.is_none() || status.wow.flavors.is_empty();
        if rising(&mut self.wow_not_found, connected && wow_missing) {
            out.push(Alert::WowNotFound);
        }

        let since = if status.site_unreachable {
            *self.unreachable_since.get_or_insert(now)
        } else {
            self.unreachable_since = None;
            now
        };
        let long_outage = status.site_unreachable
            && connected
            && !mandatory
            && now - since >= SITE_UNREACHABLE_GRACE_SECS;
        if rising(&mut self.site_unreachable, long_outage) {
            out.push(Alert::SiteUnreachable);
        }

        out
    }

    /// Une mise à jour vient d'être trouvée : alerte seulement pour une
    /// version pas encore annoncée, et pas si la mise à jour obligatoire
    /// a déjà été signalée (même action, inutile d'en remettre une couche).
    pub fn update_found(&mut self, version: &str) -> Option<Alert> {
        if self.announced_version.as_deref() == Some(version) {
            return None;
        }
        self.announced_version = Some(version.to_owned());
        if self.mandatory {
            return None;
        }
        Some(Alert::UpdateAvailable {
            version: version.to_owned(),
        })
    }
}

/// Met à jour `flag` et dit s'il vient de passer de faux à vrai.
fn rising(flag: &mut bool, now: bool) -> bool {
    let was = std::mem::replace(flag, now);
    now && !was
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headline::tests::{ok_status, NOW};

    #[test]
    fn nothing_when_all_is_fine() {
        let mut t = AlertTracker::new();
        assert!(t.observe(&ok_status(), NOW).is_empty());
        assert!(t.observe(&ok_status(), NOW + 60).is_empty());
    }

    #[test]
    fn reconnect_once_per_transition() {
        let mut t = AlertTracker::new();
        let mut s = ok_status();
        s.connection = Connection::ReauthRequired {
            reason: "403".into(),
        };
        assert_eq!(t.observe(&s, NOW), vec![Alert::Reconnect]);
        assert!(t.observe(&s, NOW + 60).is_empty());
        assert!(t.observe(&s, NOW + 3_600).is_empty());

        // Reconnecté puis de nouveau refusé : nouvelle alerte.
        assert!(t.observe(&ok_status(), NOW + 4_000).is_empty());
        assert_eq!(t.observe(&s, NOW + 5_000), vec![Alert::Reconnect]);
    }

    #[test]
    fn plain_logout_is_not_an_alert() {
        let mut t = AlertTracker::new();
        let mut s = ok_status();
        s.connection = Connection::Disconnected;
        s.wow.root = None;
        s.wow.flavors.clear();
        assert!(t.observe(&s, NOW).is_empty());
    }

    #[test]
    fn wow_not_found_when_connected() {
        let mut t = AlertTracker::new();
        let mut s = ok_status();
        s.wow.flavors.clear();
        assert_eq!(t.observe(&s, NOW), vec![Alert::WowNotFound]);
        assert!(t.observe(&s, NOW + 60).is_empty());
    }

    #[test]
    fn site_unreachable_only_after_ten_minutes() {
        let mut t = AlertTracker::new();
        let mut s = ok_status();
        s.site_unreachable = true;
        assert!(t.observe(&s, NOW).is_empty());
        assert!(t.observe(&s, NOW + 9 * 60).is_empty());
        assert_eq!(t.observe(&s, NOW + 10 * 60), vec![Alert::SiteUnreachable]);
        assert!(t.observe(&s, NOW + 60 * 60).is_empty());
    }

    #[test]
    fn short_outages_do_not_add_up() {
        let mut t = AlertTracker::new();
        let mut down = ok_status();
        down.site_unreachable = true;
        assert!(t.observe(&down, NOW).is_empty());
        assert!(t.observe(&ok_status(), NOW + 8 * 60).is_empty());
        // Nouvelle coupure : le compteur repart de zéro.
        assert!(t.observe(&down, NOW + 9 * 60).is_empty());
        assert!(t.observe(&down, NOW + 18 * 60).is_empty());
        assert_eq!(
            t.observe(&down, NOW + 19 * 60),
            vec![Alert::SiteUnreachable]
        );
    }

    #[test]
    fn mandatory_update_once_and_swallows_update_available() {
        let mut t = AlertTracker::new();
        let mut s = ok_status();
        s.update_required = Some("0.3.0".into());
        s.site_unreachable = true;
        assert_eq!(t.observe(&s, NOW), vec![Alert::MandatoryUpdate]);
        // Pendant le 426, un site « injoignable » n'est pas signalé en plus.
        assert!(t.observe(&s, NOW + 30 * 60).is_empty());
        assert_eq!(t.update_found("0.3.0"), None);
    }

    #[test]
    fn update_available_once_per_version() {
        let mut t = AlertTracker::new();
        let a = t.update_found("0.3.0").expect("première annonce");
        assert!(a.opens_update());
        assert_eq!(
            a.body(),
            "Nouvelle version 0.3.0 disponible — clique pour mettre à jour"
        );
        assert_eq!(t.update_found("0.3.0"), None);
        assert!(t.update_found("0.3.1").is_some());
    }

    #[test]
    fn several_alerts_at_once() {
        let mut t = AlertTracker::new();
        let mut s = ok_status();
        s.update_required = Some("0.3.0".into());
        s.connection = Connection::ReauthRequired {
            reason: "426".into(),
        };
        assert_eq!(
            t.observe(&s, NOW),
            vec![Alert::MandatoryUpdate, Alert::Reconnect]
        );
    }

    #[test]
    fn texts_are_plain_french() {
        let all = [
            Alert::Reconnect,
            Alert::SiteUnreachable,
            Alert::WowNotFound,
            Alert::MandatoryUpdate,
            Alert::UpdateAvailable {
                version: "1.0.0".into(),
            },
        ];
        for a in all {
            let text = format!("{} {}", a.title(), a.body()).to_lowercase();
            for word in ["token", "snapshot", "queue", "426", "403"] {
                assert!(!text.contains(word), "« {word} » dans « {text} »");
            }
        }
    }
}
