# GSF Bridge

Petite application de bureau qui fait le lien entre l'addon World of Warcraft **GuildSelfForever** et le site de la
guilde, [gsforever.com](https://gsforever.com). Elle est **facultative** pour les membres, demandée aux officiers.
Code ouvert sous licence [MIT](LICENSE).

> **In English, in short.** GSF Bridge is the optional desktop companion of the *GuildSelfForever* WoW addon
> (guild "Guild Self Forever", Forever client). It reads the addon's SavedVariables (read-only), sends the guild
> data and the cheat reports recorded by the addon to `https://gsforever.com` once you have signed in (the site
> keeps only what your trust level allows), and writes the ban list and guild settings back into a separate data
> addon (`GuildSelfForever_Data`) while the game is closed. Before sign-in, its only network call is the update
> check. It only looks at process names to know whether WoW is running, reads no game memory, talks to no other
> server, and updates itself with signed releases. Everything it reads, writes and sends
> is listed below (section « Ce que fait le bridge sur ton PC »). Build it yourself with Rust and the Tauri CLI
> (section « Compiler soi-même »). Report security issues privately: see [SECURITY.md](SECURITY.md).
> Licensed under the MIT License.

## Pourquoi un bridge ?

L'addon voit des choses en jeu (triches, morts, liaisons Discord, banque de guilde…) mais un addon WoW ne peut pas
parler à Internet. Le bridge lit ce que l'addon a enregistré sur ton PC et l'envoie au site ; dans l'autre sens, il
rapporte dans le jeu la liste des bannis tenue par le site. Plus il y a de bridges, plus les triches remontent vite.

## Ce que fait le bridge sur ton PC

Liste exacte, tirée du code (les chemins entre parenthèses renvoient au fichier source).

### Ce qu'il lit (sans jamais le modifier)

- **Les fichiers de sauvegarde de l'addon GuildSelfForever** de chaque version de WoW installée (`_classic_`,
  `_classic_era_`…) :
  `WTF/Account/<compte>/SavedVariables/GuildSelfForever.lua` et
  `WTF/Account/<compte>/<royaume>/<perso>/SavedVariables/GuildSelfForever.lua`, plus leur copie `.bak` si le
  fichier est illisible. Aucun autre fichier de WoW n'est lu ; il parcourt seulement les noms des dossiers de
  `WTF/Account` pour trouver ces fichiers, et regarde si le dossier `Interface/AddOns/GuildSelfForever` existe
  (`crates/gsf-sync/src/wow/discover.rs`, `svread.rs`).
- **Les changements dans `WTF/Account`** (notification du système quand un fichier change), pour envoyer dès que le
  jeu a sauvegardé (`watch.rs`).
- **La liste des noms des programmes lancés**, pour savoir si WoW tourne (`Wow.exe`, `WowClassic.exe`…, ou
  « World of Warcraft » sur Mac). Seulement les noms : rien d'autre sur les programmes, jamais la mémoire du jeu
  (`wow/process.rs`).
- **L'emplacement de WoW** : sous Windows, la valeur `InstallPath` de la clé de registre
  `HKLM\SOFTWARE\WOW6432Node\Blizzard Entertainment\World of Warcraft` (ou sans `WOW6432Node`), sinon
  `C:\Program Files (x86)\World of Warcraft` ; sur Mac `/Applications/World of Warcraft` ou
  `~/Applications/World of Warcraft` ; sous Linux les préfixes Wine habituels (`~/.wine`, `~/Games/battlenet`,
  Bottles…). Tu peux aussi choisir le dossier à la main.
- **Le nom de ton ordinateur** (variable `COMPUTERNAME` ou `HOSTNAME`), seulement pour nommer la connexion sur le
  site, par exemple « GSF Bridge (PC-SALON) » (`auth.rs`).

### Ce qu'il écrit

- **L'addon de données `GuildSelfForever_Data`** : `Interface/AddOns/GuildSelfForever_Data/GuildSelfForever_Data.toc`
  et `Data.lua`, avec ce que le site envoie : liste des bannis, guildes alliées, réglage des demandes, tombola en
  cours du streamer (lots, et pour ton compte seulement : tes inscriptions et tes gains) et, pour un
  officier ou un collecteur, empreintes d'identité des personnes bannies, liste noire, nom et mot de passe du canal
  de la fédération, tombola, joueurs blanchis, preuves signées par le site que ses persos sont collecteurs,
  anciens membres autorisés à revenir par « gsf » (nom, empreintes d'identité, motif, date du départ) et retours
  validés par un officier. Il y
  écrit aussi la liste des triches déjà bien reçues par le site (pour un membre : seulement celles de ses persos),
  pour que l'addon sache qu'il peut arrêter de les renvoyer. Si tu as coché « Synchroniser mon addon entre mes
  ordinateurs », il y écrit aussi la dernière photo de tes persos prise sur tes autres ordinateurs (voir plus bas).
  Seulement dans les
  versions de WoW où l'addon GuildSelfForever est installé, **seulement quand le jeu est fermé**, et seulement si le
  contenu a changé (`pushed.rs`). Il ne touche jamais aux fichiers de sauvegarde de l'addon.
- **Ses propres fichiers**, dans son dossier de données (`state.rs`, `prefs.rs`, `queue.rs`) :
  - Windows : `%APPDATA%\fr.gs4ever.bridge\` ; Mac : `~/Library/Application Support/fr.gs4ever.bridge/` ;
    Linux : `~/.local/share/fr.gs4ever.bridge/` ;
  - `config.json` (dossier de WoW choisi à la main, identifiant aléatoire de l'appareil, case « Synchroniser mon
    addon entre mes ordinateurs »), `state.json` (nom et
    rôle du compte connecté, date du dernier envoi, empreintes des envois déjà acceptés), `queue.json` (au plus
    20 envois en attente, par exemple quand le site est injoignable ou avant la connexion), `desktop.json`
    (écran d'accueil déjà vu, démarrage automatique déjà réglé…).
  - Sous Windows, l'affichage de la fenêtre (WebView2) garde aussi son cache dans `%LOCALAPPDATA%\fr.gs4ever.bridge\`.
- **Le jeton de connexion** dans le coffre de mots de passe du système, jamais en clair sur le disque : Gestionnaire
  d'identification Windows, Trousseau macOS, ou GNOME Keyring / KWallet sous Linux. Nom : `fr.gs4ever.bridge`,
  entrée `bridge-token` (et `device-secret` pour les anciens bridges « vigie »).
- **Le démarrage automatique**, seulement si tu l'as choisi sur l'écran d'accueil ou dans « Réglages » : entrée
  « GSF Bridge » dans `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (Windows),
  `~/Library/LaunchAgents/GSF Bridge.plist` (Mac) ou `~/.config/autostart/GSF Bridge.desktop` (Linux).
- **Les mises à jour** : il télécharge la nouvelle version, vérifie sa signature, puis l'installe à ta demande.

### Ce qu'il envoie, et à qui

Uniquement à `https://gsforever.com` (le site de la guilde), en HTTPS. Aucun autre serveur, aucune mesure
d'audience, aucune publicité. Chaque requête porte la version du bridge (`User-Agent: gsf-bridge/<version>` et
`X-Bridge-Version`). Le détail des échanges est dans [docs/bridge-api-v1.md](docs/bridge-api-v1.md).

- **Avant toute connexion** : seulement la recherche de mise à jour, au démarrage puis toutes les heures (version
  actuelle, système et architecture, par exemple `windows/x86_64/0.6.9`), et le téléchargement de la mise à jour.
  Rien d'autre : les données de l'addon restent sur ton PC (dans `queue.json`) tant que tu ne t'es pas connecté.
- **À la connexion** (« Me connecter avec Discord ») : ton navigateur s'ouvre sur le site avec le nom de
  l'appareil ; tu te connectes sur le site (Discord ou identifiant officier) ; le bridge reçoit un code sur
  `http://127.0.0.1:<port>` (ton propre PC) et l'échange contre un jeton, en envoyant le nom de l'appareil.
- **Une fois connecté** :
  - à chaque sauvegarde de l'addon (en pratique à la déconnexion du jeu) : le contenu utile des fichiers de
    l'addon, remis en forme, selon ton niveau (voir plus bas). C'est ce que l'addon a vu et enregistré **sur ce PC**.
    Pour un officier ou un collecteur de confiance : nom et royaume des guildes,
    membres croisés par l'addon (nom, dernière fois vu, version de l'addon), liste de la guilde si l'addon l'a
    relevée (rang, classe, niveau, dernière connexion), recrues, demandes, audits d'équipement, de sacs et d'or
    (et, depuis l'addon 0.22.11, temps de jeu `/played` et niveau quand un perso a joué sans l'addon), triches détectées, morts, banque de guilde, métiers, guildes alliées, codes de liaison Discord (`/gsf link`),
    identités Battle.net des membres (empreinte du BattleTag et identifiant de compte aléatoire tiré par l'addon ;
    BattleTag en clair seulement s'il a été reçu par un officier). Quel que soit le niveau, s'y ajoutent les chemins des fichiers lus à
    partir de `WTF` (donc **le nom du dossier de ton compte WoW**, des royaumes et des persos), leurs dates de
    modification, et si le jeu était lancé. Un envoi identique à un envoi déjà accepté n'est pas refait.
  - toutes les 15 minutes : il demande au site la liste des bannis et les réglages à écrire dans
    `GuildSelfForever_Data`, en donnant la version qu'il a déjà écrite : si rien n'a changé, le site répond
    « rien de neuf » sans rien renvoyer.
  - toutes les 5 minutes, et dès que WoW s'ouvre ou se ferme : un signal de vie « bridge lancé, WoW ouvert ou
    non » avec la guilde active, pour que les officiers sachent à quelles heures la guilde est surveillée ;
    avec l'envoi des données, les sessions de jeu des persos « collecteurs » notées par l'addon.
  - toujours la recherche de mise à jour.
- **Seulement si tu coches « Synchroniser mon addon entre mes ordinateurs »** (décochée par défaut, section « Je
  joue sur plusieurs ordinateurs ? ») : l'addon garde sur chaque PC une photo de chaque perso (sacs, équipement, or,
  banque) pour repérer ce qui a changé pendant que l'addon ne regardait pas. Case cochée, le bridge envoie au site,
  avec ses envois habituels, la dernière photo des persos de ce PC (contenu des sacs, de l'équipement et de la
  banque, or, date, identifiant d'aventure, guilde, et depuis l'addon 0.22.11 temps de jeu `/played` et niveau) ; il demande au site, au démarrage, à chaque minute quand le jeu
  est fermé et avec les données habituelles, les photos prises sur tes autres PC, et les écrit dans
  `GuildSelfForever_Data`, jeu fermé. Le site ne garde que la plus récente par perso, seulement pour des persos liés
  à ton compte, et ne la renvoie qu'à tes propres bridges qui ont aussi coché la case. Décocher demande au site
  d'effacer toutes les photos de ton compte (`engine.rs`, `baseline.rs`). Pas disponible pour un ancien bridge
  « vigie » (pas de compte).
- **Ce qui part dépend de ton niveau.** Un officier ou un collecteur de confiance (désigné par un officier sur le
  site) envoie tout ce qui est listé plus haut, dont la liste de la guilde. Un **membre** n'envoie que les preuves
  de triche : les triches détectées sur ses persos et celles que son addon a reçues, ses audits (temps de jeu et niveau compris) et ceux qu'il porte
  pour d'autres joueurs, et les morts vues par l'addon, avec le nom et le royaume de la guilde. Il n'envoie ni la
  liste de la guilde, ni les membres croisés, ni les demandes, ni la banque, ni les métiers, ni les identités
  Battle.net, ni les codes de liaison (`snapshot.rs`, `MEMBER_GUILD_KEYS`). Le niveau vient du site : rôle du
  compte connecté, puis « collecteur » dans les données reçues ; tant que le bridge ne le sait pas encore, il
  envoie comme un membre. Sur le site, les triches et audits d'un membre restent marqués « à vérifier », et les
  données des guildes que le site ne suit pas sont ignorées. Chaque envoi reçu est gardé tel quel sur le site
  90 jours (14 jours pour un membre), puis effacé.
- **Anciens bridges « vigie »** (installés avant la connexion Discord) seulement : tant qu'ils ne sont pas
  connectés, ils s'annoncent toutes les 5 minutes avec un identifiant d'appareil, un secret, le nom du PC, les
  noms des persos trouvés (50 au plus) et la version. Un bridge neuf ne le fait jamais.

### Ce qu'il ne fait pas

Pas de lecture de la mémoire du jeu, pas d'injection, pas d'automatisation en jeu, aucun accès aux autres
programmes ou fichiers, aucun envoi ailleurs qu'au site de la guilde.

## L'arrêter, couper le démarrage automatique, le désinstaller

- **L'arrêter** : clic droit sur l'icône près de l'horloge, puis « Quitter ». Fermer la fenêtre ne fait que la
  cacher.
- **Couper le démarrage automatique** : décocher « Lancer au démarrage de l'ordinateur » dans « Réglages ». Ou :
  Windows, Gestionnaire des tâches, onglet Démarrage, « GSF Bridge », Désactiver ; Mac, Réglages Système,
  Général, Ouverture, retirer « GSF Bridge » ; Linux, supprimer `~/.config/autostart/GSF Bridge.desktop`.
- **Se déconnecter** (« Réglages », « Se déconnecter ») efface le jeton du PC. Le jeton expire de toute façon
  côté site (30 jours pour un membre) ; un officier peut aussi le couper tout de suite depuis la page Bridges.
- **Désinstaller** (penser à décocher le démarrage automatique et à se déconnecter avant) :
  - Windows : Paramètres, Applications, « GSF Bridge », Désinstaller. Cocher « Supprimer les données de
    l'application » pour effacer aussi `%APPDATA%\fr.gs4ever.bridge` et `%LOCALAPPDATA%\fr.gs4ever.bridge`.
    Le jeton reste dans le Gestionnaire d'identification (Informations d'identification Windows,
    `fr.gs4ever.bridge`) si tu ne t'es pas déconnecté : le supprimer à la main.
  - Mac : quitter, glisser « GSF Bridge » de Applications à la corbeille, supprimer
    `~/Library/Application Support/fr.gs4ever.bridge` et, s'il existe, `~/Library/LaunchAgents/GSF Bridge.plist` ;
    l'entrée `fr.gs4ever.bridge` du Trousseau si besoin.
  - Linux : supprimer le fichier `.AppImage` (ou `sudo apt remove gsf-bridge` pour le `.deb`),
    `~/.local/share/fr.gs4ever.bridge` et `~/.config/autostart/GSF Bridge.desktop`.
  - Partout : le dossier `Interface/AddOns/GuildSelfForever_Data` peut être supprimé à la main (l'addon
    GuildSelfForever fonctionne sans, sans liste de bannis à jour).

## Installer

Le fichier d'installation se télécharge sur le site de la guilde. Windows est la version recommandée ; Mac
(universelle Intel et Apple Silicon) et Linux (AppImage, WoW sous Wine, Lutris ou Bottles) sont construits mais pas
encore validés sur de vraies machines.

- Windows : l'installeur n'est pas signé par un certificat payant. Si Windows affiche « Windows a protégé votre
  ordinateur » : « Informations complémentaires », puis « Exécuter quand même ».
- Mac : ouvrir le `.dmg`, glisser « GSF Bridge » dans Applications ; au premier lancement, Réglages Système,
  Confidentialité et sécurité, « Ouvrir quand même ».
- Linux : rendre le `.AppImage` exécutable (`chmod +x`) puis le lancer ; il faut un trousseau (GNOME Keyring ou
  KWallet) pour retenir la connexion.

Vérifier le fichier : chaque version publiée sur GitHub contient `SHA256SUMS` (empreintes de tous les fichiers),
et une attestation de build signée par GitHub qui prouve que le fichier vient de ce dépôt et de sa CI :

```sh
sha256sum -c SHA256SUMS --ignore-missing
gh attestation verify gsf-bridge_X.Y.Z_x64-setup.exe --repo GuildSelfForever/gsf-bridge
```

## Compiler soi-même

Prérequis : [Rust](https://rustup.rs) (la version exacte est fixée par `rust-toolchain.toml`, installée toute seule
par `rustup`), la CLI Tauri et les [prérequis système de Tauri](https://v2.tauri.app/start/prerequisites/)
(Windows : WebView2 et les « Build Tools » MSVC ; Linux : `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`,
`librsvg2-dev`, `patchelf`). Aucun npm : l'interface est servie telle quelle depuis `ui/`.

```sh
cargo install tauri-cli --version "=2.12.4" --locked

# Tests (fonctionnent partout, y compris sans les bibliothèques de Tauri)
cargo fmt --all -- --check
cargo clippy --locked -p gsf-core -p gsf-sync --all-targets -- -D warnings
cargo test --locked -p gsf-core -p gsf-sync

# Application
cargo tauri dev                                   # lancer en développement
cargo tauri build --bundles nsis                  # Windows : target/release/bundle/nsis/
cargo tauri build --bundles appimage,deb          # Linux
cargo tauri build --target universal-apple-darwin --bundles app,dmg   # Mac

# Modèle normalisé de fichiers SavedVariables, en JSON
cargo run -p gsf-core --example dump -- <compte.lua> [<perso.lua>...]
```

Un bridge compilé soi-même fonctionne normalement, mais ne se mettra pas à jour tout seul vers les versions
officielles s'il n'a pas été signé avec la clé de la guilde (l'updater refuse toute version non signée).

## Organisation du code

```txt
crates/gsf-core/    parseur des SavedVariables (rien n'est exécuté) + modèle normalisé, sans Tauri
crates/gsf-sync/    moteur : découverte de WoW, surveillance, envoi, connexion, file, données poussées
src-tauri/          application Tauri v2 (fenêtre, icône, minuteries, mise à jour), fine
ui/                 interface HTML/JS/CSS en français, sans framework
docs/               contrat d'API avec le site (bridge-api-v1.md)
.github/workflows/  ci.yml (contrôles, build Windows), release.yml (publication signée)
aidd_docs/, CLAUDE.md, .claude/   notes de travail pour l'assistant de code (voir plus bas)
```

Toute la logique est dans `gsf-core` et `gsf-sync` : ils se compilent et se testent sous Linux sans les
bibliothèques système de Tauri. Les tests de `gsf-core` utilisent des copies anonymisées de vrais fichiers de
l'addon (personnages, guildes et BattleTags inventés) et un vrai Lua 5.1 comme référence ; ceux de `gsf-sync`
un faux site (`wiremock`) et une fausse installation de WoW.

**`aidd_docs/` et `CLAUDE.md`** : le projet est développé avec un assistant de code (Claude Code et le cadre AIDD).
Ces fichiers lui décrivent le projet (architecture, conventions, commandes de contrôle). Ils ne sont pas
nécessaires pour compiler ni pour utiliser le bridge, et peuvent être lus comme une documentation technique.

## Publication (mainteneurs)

`.github/workflows/release.yml` publie à chaque push sur `main` dont la version (`[workspace.package] version` de
`Cargo.toml`) n'a pas encore de tag `vX.Y.Z` :

1. builds sans secret ni cache : Windows (obligatoire), Mac universel et Linux (qui ne bloquent pas Windows
   s'ils échouent) ;
2. job `sign`, dans l'environnement GitHub protégé `release` : signature des fichiers de mise à jour par la CLI
   Tauri (version figée, installée depuis crates.io, aucune action tierce ni cache dans ce job), `SHA256SUMS`,
   attestation de build (dépôt public) ;
3. GitHub Release avec tous les fichiers ;
4. dépôt sur le serveur du site (`php artisan gsf:bridge-release`, une fois par plateforme), sauté si les
   secrets `DEPLOY_*` manquent.

Fichiers publiés (noms attendus par le site, ne pas les changer) : `gsf-bridge_X.Y.Z_x64-setup.exe` (+ `.sig`),
`gsf-bridge_X.Y.Z_universal.dmg`, `gsf-bridge_X.Y.Z_universal.app.tar.gz` (+ `.sig`),
`gsf-bridge_X.Y.Z_amd64.AppImage` (+ `.sig`), `gsf-bridge_X.Y.Z_amd64.deb`, `SHA256SUMS`. Le `.deb` n'a pas de
mise à jour automatique : l'updater ne sait mettre à jour que l'installeur Windows, l'appli Mac et l'AppImage.

Un lancement manuel (« Run workflow ») est par défaut un essai à blanc : il construit et signe, sans rien publier.

Secrets (Settings, Environments, `release`) :

| Secret | Rôle |
| ------ | ---- |
| `TAURI_SIGNING_PRIVATE_KEY` | clé privée de signature des mises à jour (obligatoire) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | son mot de passe |
| `DEPLOY_HOST`, `DEPLOY_USER`, `DEPLOY_SSH_KEY` | accès SSH au serveur du site ; absents = pas de dépôt sur le serveur |
| `DEPLOY_PORT`, `DEPLOY_KNOWN_HOSTS` | facultatifs (22 par défaut ; sinon `ssh-keyscan`) |

La clé publique correspondante est dans `plugins.updater.pubkey` de `src-tauri/tauri.conf.json`. La clé privée ne
doit jamais entrer dans le dépôt (`*.key` est ignoré) et doit être sauvegardée hors du poste : la perdre empêche
toute mise à jour automatique des bridges installés.

Création du dépôt public avec un historique propre : voir [PUBLISHING.md](PUBLISHING.md).

## Licence

[MIT](LICENSE). World of Warcraft et Battle.net sont des marques de Blizzard Entertainment ; ce projet n'est ni
affilié à Blizzard ni approuvé par Blizzard.
