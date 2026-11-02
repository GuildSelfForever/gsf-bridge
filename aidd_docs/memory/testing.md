# Testing

Comment le projet est testé : les couches, les outils et les conventions. Où vivent les tests et comment les lancer.

## Strategy

- `gsf-core` : tests unitaires dans les modules, plus intégration sur des copies de vraies SavedVariables du client Forever, anonymisées (personnages, guildes et BattleTags inventés : `tests/fixtures`, noms non ASCII et `.bak` compris), fichiers synthétiques par version d'addon, robustesse (entrées tronquées ou invalides).
- Oracle de parsing : chaque fixture est chargée par un vrai Lua 5.1 (`mlua`, vendored, sans stdlib) et doit donner le même résultat que `sv::parse`.
- `gsf-sync` : backend simulé (`wiremock`) pour le client HTTPS, flux de connexion loopback réel, moteur complet sur une arborescence WoW factice (`tests/common`).
- `src-tauri` et `ui` : pas de tests automatisés ; vérification manuelle sous Windows (registre, détection du jeu, keyring, fenêtre, mise à jour signée).

## Tools

- `cargo test`, `mlua` (oracle), `wiremock`, `tempfile`.

## Conventions

- Toute E/S système passe par un trait injectable pour rester testable sous Linux : `MemoryStore` pour le token, `FixedGameDetector` pour le jeu lancé.
- Une nouvelle forme de SavedVariables arrive avec une fixture (copie réelle anonymisée ou `synthetic_*`) et un test. Jamais de vrai nom de joueur, de BattleTag ni d'identifiant Discord dans une fixture : le dépôt est public.
- Un changement du modèle sérialisé se vérifie contre le contrat et avec `cargo run -p gsf-core --example dump`.

## Run

- `cargo test -p gsf-core -p gsf-sync` (Linux, WSL et CI Windows).
- Les tests de `src-tauri` ne tournent pas sous Linux (bibliothèques système Tauri absentes).
