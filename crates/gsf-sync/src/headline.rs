//! Phrase d'état affichée en grand dans l'interface, en mots simples.
//!
//! Les officiers ne sont pas développeurs : une seule phrase, une couleur et,
//! si besoin, une action à faire. Tout est déduit de [`Status`] par une
//! fonction pure, testable sans fenêtre.

use serde::Serialize;

use crate::engine::{Connection, Status};

/// Couleur du point d'état.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    /// Tout va bien.
    #[default]
    Ok,
    /// Travail en cours.
    Busy,
    /// Ça repartira tout seul, ou une petite action est attendue.
    Warn,
    /// Rien ne part tant que l'officier n'agit pas.
    Error,
}

/// Action proposée à côté de la phrase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadlineAction {
    /// « Se connecter avec le site ».
    Login,
    /// « Choisir le dossier de WoW ».
    ChooseWowDir,
    /// « Mettre à jour ».
    Update,
}

/// Phrase d'état principale.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Headline {
    pub tone: Tone,
    pub text: String,
    pub action: Option<HeadlineAction>,
}

impl Headline {
    fn new(tone: Tone, text: impl Into<String>, action: Option<HeadlineAction>) -> Self {
        Headline {
            tone,
            text: text.into(),
            action,
        }
    }
}

/// Durée écoulée en français courant : « à l'instant », « il y a 2 min »,
/// « il y a 3 h », « il y a 2 jours ».
pub fn relative_time(elapsed_secs: i64) -> String {
    let s = elapsed_secs.max(0);
    match s {
        0..=59 => "à l'instant".to_owned(),
        60..=3_599 => format!("il y a {} min", s / 60),
        3_600..=86_399 => format!("il y a {} h", s / 3_600),
        _ => {
            let days = s / 86_400;
            if days == 1 {
                "il y a 1 jour".to_owned()
            } else {
                format!("il y a {days} jours")
            }
        }
    }
}

/// Déduit la phrase d'état de `status` à l'instant `now` (secondes Unix).
/// Le champ `status.headline` est ignoré.
pub fn headline(status: &Status, now: i64) -> Headline {
    if status.update_required.is_some() {
        return Headline::new(
            Tone::Error,
            "Une mise à jour obligatoire est disponible.",
            Some(HeadlineAction::Update),
        );
    }
    match &status.connection {
        Connection::Disconnected => {
            return Headline::new(
                Tone::Warn,
                "Connecte-toi pour commencer.",
                Some(HeadlineAction::Login),
            )
        }
        Connection::ReauthRequired { .. } => {
            return Headline::new(
                Tone::Warn,
                "Le site demande de te reconnecter.",
                Some(HeadlineAction::Login),
            )
        }
        Connection::RegistrationPending => {
            return Headline::new(
                Tone::Warn,
                "En attente de validation par un officier.",
                None,
            )
        }
        Connection::RegistrationRefused => {
            return Headline::new(
                Tone::Error,
                "Ce PC n'a pas été validé par les officiers.",
                None,
            )
        }
        Connection::Connected { .. } => {}
    }
    if status.wow.root.is_none() || status.wow.flavors.is_empty() {
        return Headline::new(
            Tone::Error,
            "WoW introuvable.",
            Some(HeadlineAction::ChooseWowDir),
        );
    }
    if !status.wow.flavors.iter().any(|f| f.addon_installed) {
        return Headline::new(
            Tone::Warn,
            "L'addon GuildSelfForever n'est pas installé dans WoW.",
            None,
        );
    }
    if status.busy {
        return Headline::new(Tone::Busy, "Envoi en cours…", None);
    }
    if status.site_unreachable {
        return Headline::new(
            Tone::Warn,
            "Le site est injoignable, j'enverrai les données dès qu'il revient.",
            None,
        );
    }
    if status.queue.len > 0 {
        return Headline::new(
            Tone::Warn,
            "Des données attendent d'être envoyées, nouvel essai dans une minute.",
            None,
        );
    }
    match &status.last_send {
        Some(last) if last.status == "error" => Headline::new(
            Tone::Error,
            "Le site a refusé les dernières données. Détails dans le journal.",
            None,
        ),
        Some(last) => Headline::new(
            Tone::Ok,
            format!(
                "Tout fonctionne. Dernière mise à jour envoyée {}.",
                relative_time(now - last.at)
            ),
            None,
        ),
        None => Headline::new(
            Tone::Ok,
            "Tout fonctionne. Rien n'a encore été envoyé : joue puis déconnecte-toi, je m'occupe du reste.",
            None,
        ),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::{FlavorInfo, PushedInfo, QueueInfo, WowInfo};
    use crate::state::SendRecord;

    pub(crate) const NOW: i64 = 1_800_000_000;

    pub(crate) fn ok_status() -> Status {
        Status {
            version: "0.2.0".into(),
            base_url: "http://localhost:8000".into(),
            custom_base_url: false,
            connection: Connection::Connected {
                username: "Dävin".into(),
                roles: vec!["officer".into()],
                kind: crate::state::TokenKind::Officer,
            },
            update_required: None,
            wow: WowInfo {
                root: Some("C:/WoW".into()),
                manual: false,
                flavors: vec![FlavorInfo {
                    name: "_classic_".into(),
                    path: "C:/WoW/_classic_".into(),
                    accounts: 1,
                    character_files: 2,
                    addon_installed: true,
                }],
            },
            game_running: false,
            busy: false,
            site_unreachable: false,
            last_send: Some(SendRecord {
                at: NOW - 120,
                status: "accepted".into(),
                message: String::new(),
            }),
            queue: QueueInfo {
                len: 0,
                oldest_at: None,
                last_error: None,
            },
            pushed: PushedInfo {
                version: None,
                written_at: None,
                waiting_game_closed: false,
            },
            log: Vec::new(),
            headline: Headline::default(),
        }
    }

    #[test]
    fn relative_time_in_plain_french() {
        assert_eq!(relative_time(-5), "à l'instant");
        assert_eq!(relative_time(59), "à l'instant");
        assert_eq!(relative_time(60), "il y a 1 min");
        assert_eq!(relative_time(125), "il y a 2 min");
        assert_eq!(relative_time(3_600 * 3 + 10), "il y a 3 h");
        assert_eq!(relative_time(86_400), "il y a 1 jour");
        assert_eq!(relative_time(86_400 * 4), "il y a 4 jours");
    }

    #[test]
    fn everything_fine() {
        let h = headline(&ok_status(), NOW);
        assert_eq!(h.tone, Tone::Ok);
        assert_eq!(
            h.text,
            "Tout fonctionne. Dernière mise à jour envoyée il y a 2 min."
        );
        assert_eq!(h.action, None);

        let mut s = ok_status();
        s.last_send = None;
        let h = headline(&s, NOW);
        assert_eq!(h.tone, Tone::Ok);
        assert!(h.text.starts_with("Tout fonctionne."));
    }

    #[test]
    fn login_needed() {
        let mut s = ok_status();
        s.connection = Connection::Disconnected;
        let h = headline(&s, NOW);
        assert_eq!(h.text, "Connecte-toi pour commencer.");
        assert_eq!(h.action, Some(HeadlineAction::Login));

        s.connection = Connection::ReauthRequired {
            reason: "Rétrogradé".into(),
        };
        let h = headline(&s, NOW);
        assert_eq!(h.action, Some(HeadlineAction::Login));
        assert_eq!(h.tone, Tone::Warn);
    }

    #[test]
    fn device_registration_pending_and_refused() {
        let mut s = ok_status();
        s.connection = Connection::RegistrationPending;
        let h = headline(&s, NOW);
        assert_eq!(h.text, "En attente de validation par un officier.");
        assert_eq!(h.tone, Tone::Warn);
        assert_eq!(h.action, None);

        s.connection = Connection::RegistrationRefused;
        let h = headline(&s, NOW);
        assert_eq!(h.text, "Ce PC n'a pas été validé par les officiers.");
        assert_eq!(h.tone, Tone::Error);
        assert_eq!(h.action, None);
    }

    #[test]
    fn wow_missing() {
        let mut s = ok_status();
        s.wow.root = None;
        s.wow.flavors.clear();
        let h = headline(&s, NOW);
        assert_eq!(h.text, "WoW introuvable.");
        assert_eq!(h.tone, Tone::Error);
        assert_eq!(h.action, Some(HeadlineAction::ChooseWowDir));

        // Dossier choisi mais sans installation lisible.
        let mut s = ok_status();
        s.wow.flavors.clear();
        assert_eq!(headline(&s, NOW).action, Some(HeadlineAction::ChooseWowDir));
    }

    #[test]
    fn addon_missing() {
        let mut s = ok_status();
        s.wow.flavors[0].addon_installed = false;
        let h = headline(&s, NOW);
        assert_eq!(h.tone, Tone::Warn);
        assert!(h.text.contains("addon"));
    }

    #[test]
    fn site_unreachable_and_waiting() {
        let mut s = ok_status();
        s.site_unreachable = true;
        s.queue.len = 1;
        let h = headline(&s, NOW);
        assert_eq!(
            h.text,
            "Le site est injoignable, j'enverrai les données dès qu'il revient."
        );
        assert_eq!(h.tone, Tone::Warn);

        s.site_unreachable = false;
        let h = headline(&s, NOW);
        assert_eq!(h.tone, Tone::Warn);
        assert!(h.text.starts_with("Des données attendent"));
    }

    #[test]
    fn busy_and_refused() {
        let mut s = ok_status();
        s.busy = true;
        s.site_unreachable = true;
        assert_eq!(headline(&s, NOW).tone, Tone::Busy);

        let mut s = ok_status();
        s.last_send.as_mut().unwrap().status = "error".into();
        assert_eq!(headline(&s, NOW).tone, Tone::Error);
    }

    #[test]
    fn mandatory_update_comes_first() {
        let mut s = ok_status();
        s.update_required = Some("0.3.0".into());
        s.connection = Connection::Disconnected;
        s.wow.root = None;
        let h = headline(&s, NOW);
        assert_eq!(h.text, "Une mise à jour obligatoire est disponible.");
        assert_eq!(h.action, Some(HeadlineAction::Update));
        assert_eq!(h.tone, Tone::Error);
    }

    #[test]
    fn no_jargon() {
        let mut cases = vec![ok_status()];
        let mut s = ok_status();
        s.connection = Connection::Disconnected;
        cases.push(s);
        let mut s = ok_status();
        s.site_unreachable = true;
        cases.push(s);
        let mut s = ok_status();
        s.queue.len = 2;
        cases.push(s);
        for s in cases {
            let text = headline(&s, NOW).text.to_lowercase();
            for word in [
                "snapshot",
                "token",
                "upload",
                "hash",
                "queue",
                "file d'attente",
            ] {
                assert!(!text.contains(word), "« {word} » dans « {text} »");
            }
        }
    }
}
