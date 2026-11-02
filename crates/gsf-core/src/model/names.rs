//! Identités : noms de personnages et clés de guilde comme l'addon les
//! normalise (`string.lower` de Lua = minuscules ASCII seulement), plus la
//! résolution des alias « prénom seul » de WoW Forever.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

/// Espaces au sens de `%s` en Lua (`isspace` du C, locale « C »).
fn is_lua_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0B' | '\x0C' | '\r')
}

/// `GSF:Trim`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_lua_space)
}

/// `string.lower` de Lua : seules les lettres ASCII changent (`É` reste `É`).
pub fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// `GSF:CleanName` : sans suffixe `-Royaume`, minuscules ASCII. `None` si vide.
pub fn clean_name(name: &str) -> Option<String> {
    let t = trim(name);
    let t = t.split('-').next().unwrap_or("");
    (!t.is_empty()).then(|| lower(t))
}

/// Clé de guilde normalisée : royaume en minuscules **sans espaces**, `#`,
/// nom de guilde rogné en minuscules. Fusionne `classic beta pve#x` et
/// `classicbetapve#x`, que le client a écrits selon la source du royaume
/// (`GetRealmName` ou `GetNormalizedRealmName`).
pub fn normalize_guild_key(key: &str) -> String {
    match key.split_once('#') {
        Some((realm, guild)) => {
            let realm: String = realm.chars().filter(|c| !is_lua_space(*c)).collect();
            format!("{}#{}", lower(&realm), lower(trim(guild)))
        }
        None => lower(trim(key)),
    }
}

/// `GSF:MakeGuildStorageKey(guildName, realmName)` puis normalisation.
pub fn guild_key_from_parts(guild_name: &str, realm: &str) -> Option<String> {
    let guild = trim(guild_name);
    if guild.is_empty() {
        return None;
    }
    Some(normalize_guild_key(&format!("{}#{}", trim(realm), guild)))
}

/// Référence à un personnage telle qu'écrite dans le fichier, avec son
/// identité canonique si elle a pu être établie sans ambiguïté.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CharacterRef {
    /// Texte brut (clé de table ou champ).
    pub raw: String,
    /// Identité canonique (nom complet en minuscules ASCII), `None` si vide
    /// ou ambigu.
    pub character: Option<String>,
    /// Noms complets possibles quand l'alias est ambigu.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
}

impl CharacterRef {
    /// Clé de fusion : l'identité, sinon le nom brut nettoyé (préfixé `?`
    /// pour ne jamais se confondre avec une identité établie).
    pub fn merge_key(&self) -> String {
        match &self.character {
            Some(c) => c.clone(),
            None => format!("?{}", clean_name(&self.raw).unwrap_or_default()),
        }
    }
}

/// Alias « prénom seul » qu'aucune règle n'a permis de rattacher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnresolvedAlias {
    pub alias: String,
    pub candidates: Vec<String>,
    /// Où l'alias apparaît (`professions`, `requests.requester`…).
    pub contexts: Vec<String>,
}

/// Résolution des noms d'une guilde.
///
/// WoW Forever expose parfois un `UnitName` court (« Garou ») là où le roster
/// donne le nom affiché (« Garou Bel »). Un nom sans espace est rattaché au
/// seul nom complet connu qui commence par « nom␠ » ; s'il y en a plusieurs
/// (« Davin » → « Davin Test », « Davin Douze »), il n'est rattaché à rien.
/// S'il n'y en a aucun, c'est un nom complet d'un seul mot (« Astral »).
#[derive(Debug, Default)]
pub struct Resolver {
    known: BTreeSet<String>,
    unresolved: BTreeMap<String, (Vec<String>, BTreeSet<String>)>,
}

impl Resolver {
    /// Déclare un nom complet fiable (champ `name`, clé avec espace…).
    /// Les noms sans espace sont ignorés : ce sont peut-être des alias.
    pub fn learn(&mut self, name: &str) {
        if let Some(clean) = clean_name(name) {
            if clean.contains(' ') {
                self.known.insert(clean);
            }
        }
    }

    pub fn resolve(&mut self, raw: &str, context: &str) -> CharacterRef {
        let mut out = CharacterRef {
            raw: raw.to_string(),
            character: None,
            candidates: Vec::new(),
        };
        let Some(clean) = clean_name(raw) else {
            return out;
        };
        if clean.contains(' ') {
            out.character = Some(clean);
            return out;
        }
        let prefix = format!("{clean} ");
        let candidates: Vec<String> = self
            .known
            .range(prefix.clone()..)
            .take_while(|n| n.starts_with(&prefix))
            .cloned()
            .collect();
        match candidates.len() {
            0 => out.character = Some(clean),
            1 => out.character = candidates.into_iter().next(),
            _ => {
                let entry = self
                    .unresolved
                    .entry(clean)
                    .or_insert_with(|| (candidates.clone(), BTreeSet::new()));
                entry.1.insert(context.to_string());
                out.candidates = candidates;
            }
        }
        out
    }

    pub fn into_unresolved(self) -> Vec<UnresolvedAlias> {
        self.unresolved
            .into_iter()
            .map(|(alias, (candidates, contexts))| UnresolvedAlias {
                alias,
                candidates,
                contexts: contexts.into_iter().collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_is_ascii_only_like_lua() {
        assert_eq!(lower("Fumée ÉCLAIR"), "fumée Éclair");
        assert_eq!(
            clean_name("  Garou Bel-ClassicBetaPvE "),
            Some("garou bel".into())
        );
        assert_eq!(clean_name("  "), None);
        assert_eq!(clean_name("-Realm"), None);
    }

    #[test]
    fn guild_keys_merge_when_realm_differs_by_spaces() {
        assert_eq!(
            normalize_guild_key("classic beta pve#mirabel france"),
            normalize_guild_key("ClassicBetaPvE#Mirabel France ")
        );
        assert_eq!(
            normalize_guild_key("classic beta pve#mirabel france"),
            "classicbetapve#mirabel france"
        );
        assert_eq!(
            guild_key_from_parts("Mirabel France", "Classic Beta PvE").as_deref(),
            Some("classicbetapve#mirabel france")
        );
        assert_eq!(guild_key_from_parts(" ", "x"), None);
    }

    #[test]
    fn aliases_resolve_only_when_unique() {
        let mut r = Resolver::default();
        for n in [
            "Garou Bel",
            "Davin Test",
            "Davin Douze",
            "Fumée Ravine",
            "Garou",
        ] {
            r.learn(n);
        }
        assert_eq!(
            r.resolve("Garou", "t").character.as_deref(),
            Some("garou bel")
        );
        assert_eq!(
            r.resolve("garou bel", "t").character.as_deref(),
            Some("garou bel")
        );
        assert_eq!(
            r.resolve("fumée", "t").character.as_deref(),
            Some("fumée ravine")
        );
        assert_eq!(
            r.resolve("Astral", "t").character.as_deref(),
            Some("astral")
        );
        let d = r.resolve("Davin", "professions");
        assert_eq!(d.character, None);
        assert_eq!(d.candidates, ["davin douze", "davin test"]);
        assert_eq!(d.merge_key(), "?davin");
        assert_eq!(r.resolve("", "t").character, None);
        let u = r.into_unresolved();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].alias, "davin");
        assert_eq!(u[0].contexts, ["professions"]);
    }
}
