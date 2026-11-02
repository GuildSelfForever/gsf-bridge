# VCS

Les conventions de gestion de version du projet : branches, commits et plateforme.

## Setup

- Main branch: `main`
- Platform: `github` (`GuildSelfForever/gsf-bridge`, CLI `git` et `gh`) ; code ouvert sous licence MIT (voir `LICENSE`, `PUBLISHING.md` pour la création du dépôt public)

## Branches

- Format : `type/description-courte` (`docs/aidd-memory`)
- Types in use : `feat`, `fix`, `docs`, `chore`, `ci`

## Commits

- Convention: Conventional Commits
- Format: `type(scope): description`, scope = zone touchée (`core`, `sync`, `app`, `updater`) ; `ci:` et `docs:` sans scope
- Rules: en anglais, impératif, minuscules

## Commit Strategy

AI should auto commit: `never` (commit par l'IA autorisé uniquement sur demande explicite)
