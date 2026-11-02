# Project Brief

Ce qu'est le projet, le problème qu'il résout et son vocabulaire. Le « pourquoi » non dérivable, pas le « comment ».

## What it is

- GSF Bridge : application de bureau Windows réservée aux officiers de la guilde Guild Self Forever (WoW, client Forever). Elle relie l'addon `GuildSelfForever` au site de la guilde (backend `web-gsf`).
- Construite pour un streamer non développeur : elle doit tourner seule, sans réglage, avec des messages clairs en français.

## Why it exists

- L'addon ne peut pas parler au réseau : seul un programme local peut lire ses SavedVariables et les envoyer au site (audits, rangs, banques, demandes).
- À l'inverse, le site doit pouvoir pousser des données dans le jeu (liste de bans) sans toucher aux SavedVariables de l'addon.

## Domain language

| Term | Meaning |
| ---- | ------- |
| SavedVariables | fichiers Lua écrits par le client WoW à la déconnexion (`WTF/Account/.../SavedVariables/GuildSelfForever.lua`), compte et personnages |
| version (flavor) | dossier de version du client sous la racine WoW (`_classic_`, `_classic_beta_`…) contenant `WTF/Account` |
| guild key | `guild_key_normalized` : royaume sans espaces + `#` + guilde, minuscules ASCII ; identité d'une guilde partagée avec le backend |
| alias | nom de personnage « prénom seul » rattaché à sa clé complète quand il est unique (`CharacterRef.character` = `null` si ambigu) |
| snapshot | corps de `POST /api/v1/bridge/snapshots` pour un compte, identifié par `snapshot_hash` |
| données poussées | `GSF_PushedData` (bans, guildes alliées, canal de fédération, preuves de collecteur, violations reçues par le site) servies par le site et écrites dans l'addon séparé `GuildSelfForever_Data` |
| token bridge | token Sanctum (`bridge:read`, `bridge:push`) obtenu par connexion navigateur, rangé dans le gestionnaire d'identifiants Windows |
| ban de personne | ban en cascade de tous les persos connus d'une personne via son empreinte Battle.net/compte (`bt`/`acct`, contrat 2026-10-17), distinct du ban de perso par nom (`bans`) |

## Key features

- Découverte de l'installation WoW (registre, emplacements par défaut ou choix manuel) et de tous les fichiers SavedVariables, sans supposer royaume ni personnage.
- Surveillance de `WTF/Account`, parsing, normalisation par guilde et envoi idempotent au site, avec file d'attente persistée.
- Connexion officier par navigateur + loopback, token dans le gestionnaire d'identifiants.
- Écriture de la liste de bans dans `GuildSelfForever_Data`, jeu fermé uniquement.
- Installé une seule fois : le bridge propose ensuite ses mises à jour signées (fenêtre « Nouvelle version disponible »), servies par le site.
