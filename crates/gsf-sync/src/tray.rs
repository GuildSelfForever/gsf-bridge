//! Icône de la zone de notification (près de l'horloge) : couleur et info-bulle
//! déduites de la phrase d'état, par des fonctions pures testables.

use crate::headline::{Headline, Tone};

/// Nom affiché devant la phrase dans l'info-bulle.
pub const APP_NAME: &str = "GSF Bridge";

/// Windows tronque l'info-bulle d'une icône de notification à 127 caractères.
pub const TOOLTIP_MAX_CHARS: usize = 127;

/// Variante de l'icône : pastille verte, orange ou rouge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    /// Tout va bien, ou un envoi est en cours.
    Ok,
    /// Ça repartira tout seul, ou une petite action est attendue.
    Attention,
    /// Rien ne part tant que l'officier n'agit pas.
    Error,
}

impl TrayState {
    pub fn from_tone(tone: Tone) -> Self {
        match tone {
            Tone::Ok | Tone::Busy => TrayState::Ok,
            Tone::Warn => TrayState::Attention,
            Tone::Error => TrayState::Error,
        }
    }
}

/// Info-bulle : « GSF Bridge — <phrase d'état> », coupée proprement si trop longue.
pub fn tooltip(headline: &Headline) -> String {
    let full = format!("{APP_NAME} — {}", headline.text);
    if full.chars().count() <= TOOLTIP_MAX_CHARS {
        return full;
    }
    let mut cut: String = full.chars().take(TOOLTIP_MAX_CHARS - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_to_icon() {
        assert_eq!(TrayState::from_tone(Tone::Ok), TrayState::Ok);
        assert_eq!(TrayState::from_tone(Tone::Busy), TrayState::Ok);
        assert_eq!(TrayState::from_tone(Tone::Warn), TrayState::Attention);
        assert_eq!(TrayState::from_tone(Tone::Error), TrayState::Error);
    }

    #[test]
    fn tooltip_names_the_app() {
        let h = Headline {
            tone: Tone::Ok,
            text: "Tout fonctionne.".into(),
            action: None,
        };
        assert_eq!(tooltip(&h), "GSF Bridge — Tout fonctionne.");
    }

    #[test]
    fn long_tooltip_is_cut_on_a_char_boundary() {
        let h = Headline {
            tone: Tone::Warn,
            text: "é".repeat(200),
            action: None,
        };
        let t = tooltip(&h);
        assert_eq!(t.chars().count(), TOOLTIP_MAX_CHARS);
        assert!(t.ends_with('…'));
        assert!(t.starts_with("GSF Bridge — éé"));
    }

    #[test]
    fn every_real_headline_fits() {
        let s = crate::headline::tests::ok_status();
        let h = crate::headline::headline(&s, crate::headline::tests::NOW);
        assert!(!tooltip(&h).ends_with('…'));
        let mut s = s;
        s.last_send = None;
        let h = crate::headline::headline(&s, crate::headline::tests::NOW);
        assert!(!tooltip(&h).ends_with('…'), "{}", tooltip(&h));
    }
}
