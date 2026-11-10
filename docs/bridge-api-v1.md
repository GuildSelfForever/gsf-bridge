<!--
Copie publique du contrat d'API entre le bridge et le site gsforever.com.
L'original vit dans l'espace de travail privé du projet ; cette copie en retire les noms de joueurs réels
(remplacés par des personnages inventés) et les renvois vers des documents internes. Les « plans » cités
sont des notes de travail internes non publiées : le contrat ci-dessous se suffit à lui-même.
-->

# Contrat API bridge ↔ backend — v1

Source de vérité partagée par `gsf-bridge` (Rust) et `web-gsf` (Laravel). Toute évolution incompatible = `v2`.
Base : `https://<domaine>/api/v1`. Erreurs : `{"error":{"code":"…","message":"…","details"?:…}}`.
Auth : `Authorization: Bearer <token Sanctum bridge>` (abilities `bridge:read`, `bridge:push`), sauf l'échange de code.

## 1. Connexion du bridge (flux navigateur + loopback)

1. Le bridge écoute sur `http://127.0.0.1:<port aléatoire>/callback` et ouvre le navigateur sur
   `GET /bridge/connect?port=<port>&state=<aléa 32 car.>` (page web, session officier requise → login si besoin).
2. L'officier clique « Autoriser ce bridge » (POST web, CSRF) → le site crée un code à usage unique (60 s, haché en base,
   lié à l'utilisateur et au `state`) et redirige vers `http://127.0.0.1:<port>/callback?code=<code>&state=<state>`.
   `port` doit être un entier 1024–65535 ; aucune autre redirection n'est permise.
3. Le bridge vérifie `state`, puis `POST /api/v1/bridge/token` (sans auth, throttle 10/min/IP)
   `{"code":"…","state":"…","device_name":"PC de X"}` → `201 {"token":"<plaintext>","expires_at":"ISO8601","user":{"id","username","roles":[…]}}`.
   Code inconnu/expiré/déjà utilisé/state différent → `422 invalid_code`. Utilisateur non officier/admin ou inactif → `403 forbidden`.
4. Le token est stocké par le bridge dans le gestionnaire d'identifiants Windows (crate `keyring`). `403` sur une route bridge
   ⇒ le bridge efface le token et redemande la connexion.

## 2. Envoi des données : `POST /api/v1/bridge/snapshots` (`bridge:push`)

Corps JSON (gzip accepté avec `Content-Encoding: gzip`, taille max décompressée 8 Mo) :

```json
{
  "model_version": 1,
  "bridge_version": "0.1.0",
  "snapshot_hash": "<sha256 hex du JSON canonique de `guilds`>",
  "source": {
    "account_file": "WTF/Account/<ACCOUNT>/SavedVariables/GuildSelfForever.lua",
    "account_mtime": 1800000500,
    "character_files": [{"path": "WTF/Account/<ACCOUNT>/<dossier>/<Perso>/SavedVariables/GuildSelfForever.lua", "mtime": 1800000400}],
    "game_running": false
  },
  "guilds": [ "<GuildData>" ]
}
```

- `GuildData` = sérialisation serde de `gsf_core::model::GuildData` pour `MODEL_VERSION = 1`
  (`crates/gsf-core/src/model/mod.rs` dans gsf-bridge fait foi ; exemple : `cargo run -p gsf-core --example dump -- <fichiers>`).
  Champs ≥ 0.17.47 (`writer_version`, `bridge_roster`, `bridge_link_outbox`, `bridge_violations`, `bridge_local_violations`) peuvent être absents/`null`.
- Les noms de personnages sont des clés normalisées : minuscules ASCII (comme `string.lower` Lua), `CharacterRef.character`
  = clé résolue ou `null` si alias ambigu (alors ignorer pour l'identité). Clé de guilde = `guild_key_normalized`
  (royaume sans espaces + `#` + guilde, minuscules ASCII) — identique à `App\Support\WowName` côté Laravel.
- Idempotence : même `snapshot_hash` déjà reçu pour la même guilde → `200 {"status":"duplicate"}` sans retraitement.
  Sinon `202 {"status":"accepted","upload_id":…}` ; le traitement se fait en file (job), jamais dans la requête.
- Sécurité : le backend n'accepte que les guildes du registre (fédération multi-guildes, page admin « Guildes »,
  `guild_key_normalized` par guilde, une guilde peut être « suivie » ou non) ; **toutes** les guildes suivies présentes
  dans le snapshot sont traitées (une par guilde), les guildes non suivies sont ignorées et signalées dans la
  réponse (`ignored_guilds`).

## 3. Données poussées vers le jeu : `GET /api/v1/bridge/pushed-data` (`bridge:read`)

`200 {"schema":1,"guild_key":"<guildKey tel qu'attendu par l'addon : GetActiveGuildStorageKey()>","generated_at":1800000000,"version":"<hash>","allied_guilds":[{"key":"…","name":"…"}],"bans":{"<clé perso>":{"reason":"…","until":null|epoch,"by":"…"}}}`
→ le bridge écrit `Interface/AddOns/GuildSelfForever_Data/Data.lua` (`GSF_PushedData = {…}`) **jeu fermé uniquement**.
Format exact attendu par l'addon : voir `README.md` de guild-self-forever, section « Exports pour le bridge officier ».
Paramètre facultatif `?guild_key=<clé normalisée>` : la guilde active du PC qui interroge (fédération multi-guildes,
voir décisions phase 5 ci-dessous).

## 4. Mise à jour du bridge : `GET /api/v1/bridge/update/{target}/{arch}/{current_version}` (`bridge:read`)

- `204` si à jour. Sinon `200` au format Tauri updater : `{"version":"x.y.z","notes":"…","pub_date":"ISO8601","url":"https://…/bridge/download/<version>/<fichier>","signature":"<contenu .sig>"}`.
- `426 {"error":{"code":"bridge_outdated","message":…,"details":{"min_version":"…"}}}` sur **toutes** les routes bridge si
  `X-Bridge-Version` < version minimale réglée par l'admin. Le bridge envoie `X-Bridge-Version` sur chaque requête.
- Les binaires sont publiés par la CI dans le stockage du site (`storage/app/bridge-releases/<version>/`) avec un manifeste ;
  le téléchargement exige le token bridge.

## Décisions d'implémentation (backend web-gsf, branche `feat/bridge-ingestion`)

Points du contrat précisés ou ajustés lors de l'implémentation Laravel. Le bridge doit s'y conformer.

1. **Page de connexion** : `GET /bridge/connect` accepte en plus `device=<nom>` (optionnel, ≤ 60 car.), affiché seul sur la page
   de confirmation avec le navigateur et l'IP. `state` doit respecter `^[A-Za-z0-9_-]{32,128}$`. Paramètres invalides → page d'erreur
   `422`, **jamais** de redirection. Le callback reçoit `code` (48 car.) et `state` en query string.
2. **Échange du code** : `device_name` est optionnel (défaut « Bridge ») ; le token Sanctum s'appelle `bridge:<device_name>`.
   `user.roles` = noms des rôles du site (`["officer"]` ou `["admin"]`). Un `state` faux ne consomme pas le code.
3. **Snapshots — réponses** : `202 {"status":"accepted","upload_id","ignored_guilds"}`, `200 {"status":"duplicate","upload_id","ignored_guilds"}`
   (`upload_id` = envoi d'origine) et **nouveau** `200 {"status":"ignored","upload_id","ignored_guilds"}` quand aucune guilde du snapshot
   n'est la guilde suivie (ou qu'aucune n'est réglée) : l'envoi est journalisé pour la page admin, rien n'est traité.
   `ignored_guilds` est toujours présent (liste, éventuellement vide), clés normalisées.
4. **Snapshots — erreurs** : `422 unsupported_model_version` (`details.supported = [1]`), `422 validation_failed`, `413 payload_too_large`
   (corps brut ou décompressé > 8 Mo, décompression par blocs interrompue au dépassement), `400 invalid_gzip`, `400 invalid_json`,
   `415 unsupported_encoding` (seuls `gzip`/`identity`). Le bridge devrait **toujours compresser** (PHP limite aussi le corps brut).
5. **`snapshot_hash`** : traité comme clé d'idempotence opaque (64 hex minuscules), **non recalculé** côté serveur (la forme canonique du
   JSON n'est pas reproductible à l'identique en PHP). Doublon = même guilde suivie + même hash déjà reçu et pas en échec ; un envoi
   dont le traitement a échoué peut être renvoyé.
6. **Guildes suivies** (fédération multi-guildes, remplace l'ancien réglage `bridge_guild_key`) : registre admin page
   « Guildes », chaque guilde typée (Principale, Secondaire, Alt, Banque) avec sa clé `guild_key_normalized`. Le serveur
   re-normalise aussi chaque clé reçue (une par guilde du snapshot). Un snapshot produit une ligne d'envoi et un
   traitement par guilde suivie qu'il contient ; les données de chaque guilde non suivie restent ignorées (gzip, 90 jours).
7. **`X-Bridge-Version`** enregistré (`client_version`) en plus de `bridge_version` du corps.
8. **Identité des personnages** : un personnage = `CharacterRef.character` + royaume `GuildData.realm`. Les entrées dont `character`
   est `null` (alias ambigu) sont ignorées pour l'identité : pas de personnage, pas de métier, pas d'audit (les audits sont dédupliqués
   par joueur + identifiant) ; une violation est gardée avec `player_key = null` (dédupliquée par identifiant).
9. **Fraîcheur** : rang/classe/niveau ne changent que si `bridge_roster[].seen_at` (à défaut `bridge_roster_at`) est strictement plus récent
   que la dernière observation ; une valeur absente (`null`) ne remplace jamais une valeur connue. Version de l'addon : `members[].last_seen`.
   Aventure : `updated_at` (à défaut `enrolled_at`) ; `left_guild_at` = heure de passage à `OUT_OF_GUILD`/`RETIRED`.
   Changement de rang noté (`rank_changes`) seulement si un rang était déjà connu (la première observation n'est pas une promotion).
10. **Liaisons depuis `bridge_link_outbox`** : la validité de 15 min d'un code s'apprécie à l'heure `at` de réception en jeu (bornée à
    maintenant), car l'officier ne se déconnecte souvent que bien plus tard ; les codes expirés restent donc 8 jours en base (outbox = 7 j).
    Entrée `conflict = true` → refusée ; `sender.character = null` → refusée ; résultat noté par (code, expéditeur) ; une entrée liée
    n'est jamais retraitée. Le personnage lié est l'expéditeur résolu (`senderKey`), jamais le contenu d'un message de chat.
11. **Demandes** : par identifiant, `updated_at` le plus récent gagne, entre `requests` et `backup_requests` comme entre snapshots ; à
    égalité, l'enregistrement de la table `requests` l'emporte sur une ligne de sauvegarde. Banques : par propriétaire (perso) ou onglet,
    un relevé n'écrase que s'il a un `time` strictement plus récent.
12. **Données poussées (§3) — format corrigé** : le format du contrat initial (`bans` = objet indexé par clé avec `until`/`by`) **n'est pas**
    celui que valide l'addon (`BridgeExport.lua`, `validatePushedData`). Réponse réelle :
    `{"schema":1,"guild_key":"…","generated_at":<epoch>,"version":"<sha256>","allied_guilds":[…],"bans":[{"name":"Nom","reason":"…","expiresAt":<epoch>}]}`
    — `bans` est une **liste** ; `reason` et `expiresAt` sont **omis** quand absents/définitifs (≤ 500 bans, nom ≤ 64 octets sans
    caractère de contrôle ni `|`, raison ≤ 200 octets). Le bridge écrit `GSF_PushedData = { schema = 1, guildKey = <guild_key>,
    generatedAt = <generated_at>, alliedGuilds = { { key = …, name = … }, … }, bans = { { name = …, reason = …, expiresAt = … }, … } }`
    (`version` sert seulement à savoir s'il faut réécrire). `version` = hash du contenu sans `generated_at` : stable tant que la
    liste de bans et celle des guildes alliées ne changent pas.
13. **`guild_key` poussée** = clé normalisée de la guilde **active du PC qui interroge** (paramètre `?guild_key=` ci-dessus), qui
    vaut `GetNormalizedRealmName():lower().."#"..guilde:lower()`, la clé construite par `GSF:MakeGuildStorageKey` dans le cas
    normal — renvoyée **exactement** comme reçue (après normalisation), jamais remplacée par la clé stockée en base de la guilde
    (qui peut différer d'un royaume lié numéroté, cf. `WowName::sameGuild`). `guild_key` absent (anciens bridges, compatibilité) →
    repli sur la guilde Principale, comportement inchangé. `guild_key` présent mais ne résolvant à aucune guilde suivie →
    `404 guild_not_tracked`. Aucune guilde Principale suivie (et `guild_key` absent) → `409 guild_not_configured`. Limite connue :
    un client dont le profil a été créé avec `GetRealmName()` (royaume avec espaces, ex. `classic beta pve#…`) rejettera le
    fichier ; à corriger côté addon en comparant les clés normalisées si ce cas se présente.
14. **`allied_guilds`** (fédération multi-guildes, phase 5) : toutes les **autres** guildes suivies que celle demandée, tout type
    confondu (Secondaire, Alt et **Banque compris** — la liste sert de liste blanche d'échanges entre guildes côté addon, pas
    d'affichage), triées par nom ; `{"key": "<clé normalisée>", "name": "<nom affiché en jeu>"}`. Inclus dans le hash `version`.
15. **Mises à jour (§4)** : URL de téléchargement = `https://<domaine>/api/v1/bridge/download/<version>/<fichier>` (token bridge requis,
    même en-tête `Authorization` que la vérification). `target`/`arch` = valeurs Tauri (`windows`/`x86_64`…), plateforme manifeste
    `"<target>-<arch>"`. Version actuelle invalide → `422 invalid_version`. Manifeste `storage/app/bridge-releases/latest.json` =
    `{"version","notes","pub_date","platforms":{"windows-x86_64":{"file","signature"}}}`, écrit par `php artisan gsf:bridge-release
    <version> <fichier> <fichier.sig> --notes= [--target=windows --arch=x86_64]`.
16. **Version minimale (426)** : appliquée aux routes de **données** (`ping`, `link-codes/consume`, `snapshots`, `pushed-data`), **pas** à
    `update`, `download` ni `token` : sinon un bridge trop ancien ne pourrait plus jamais se mettre à jour. Comparaison semver (préversion
    < version finale). En-tête absent → accepté et journalisé ; en-tête invalide → 426 si un minimum est réglé.
17. **`federation`** (fédération multi-guildes, relais inter-guildes des audits) : objet ajouté à la réponse (et inclus dans le hash
    `version`), `{"channel":"gsf<10 car. hex>","password":"<16 car. hex>","guilds":[{"key","name","type","mode"},…]}`. `channel`/`password`
    dérivés par `hash_hmac('sha256', 'channel'|'password', <secret>)` d'un secret unique engendré une seule fois (`Setting`
    `federation_channel_secret`, chiffré, `Str::random(40)`) — seulement `[a-z0-9]`, comme l'exige un nom de salon personnalisé WoW.
    `guilds` = **toutes** les guildes suivies (celle demandée comprise), triées par nom. Le bridge écrit
    `GSF_PushedData.federation = { channel = …, password = …, guilds = { { key = …, name = …, type = …, mode = … }, … } }` ;
    l'addon rejoint ce salon caché (toute guilde fédérée) pour y recevoir/déposer un audit sur un joueur de n'importe quelle
    autre guilde fédérée (`App\Support\Bridge\FederationChannel`).
18. **`requests` et `cleared`** (contrat 2026-10-15, demandes/statuts/morts, §C) : deux clés ajoutées à la réponse, incluses dans le
    hash `version`. `requests = {"expiry_days": <1-60>}` : réglage site `request_expiry_days` (admin page Demandes, défaut 7),
    au-delà duquel une demande active non prise en charge est fermée côté site (`close_reason="expired"`) ; l'addon applique le
    même délai à `requestExpiryDays`. `cleared = [{"name": "Nom", "at": <epoch>}, …]` : joueurs dont un cas a été classé
    « Ce n'est rien » par le tribunal dans les 30 derniers jours (un seul enregistrement par joueur, la décision la plus
    récente), `at` = heure de la décision ; l'addon qui lit `cleared` repasse `CHEATER` à `ENROLLED` pour ces joueurs si
    `flaggedAt <= at`.

---

# Contrat API bot ↔ backend (ajout phase 10)

`GET /api/v1/bot/config` (token bot, ability `bot:read`) → `200`
```json
{
  "guild_id": "123…",
  "roles": {"visitor": "…|null", "member": "…|null", "bots": "…|null", "guild_member": "…|null"},
  "class_roles": {"warrior": "…", "mage": "…"},
  "channels": {"rules": "…|null", "welcome": "…|null", "classes": "…|null", "verify": "…|null"},
  "voice": {"create_room": "…|null", "create_group": "…|null", "category": "…|null"},
  "version": "<hash du contenu>"
}
```
- Valeurs = IDs Discord (chaînes) ou `null` si non configuré. Réglées dans l'admin (page Réglages Discord).
- Le bot lit cette config au démarrage puis toutes les 5 min (et applique sans redémarrage si `version` change).
  Les variables d'environnement restent un repli si le site est injoignable au démarrage ; `DISCORD_TOKEN`, `API_URL`, `API_TOKEN` restent en env.
- `POST /api/v1/bot/heartbeat` (ability `bot:read`) `{"version":"<version du bot>","guilds":1,"uptime_s":123}` → `204` ; le site affiche l'état du bot (en ligne si heartbeat < 3 min) sur la page Système.

## Décision sur un cas de triche depuis Discord : `POST /api/v1/bot/cases/{alert}/decision` (ability `bot:cases`, couverte par `bot:*`)

Le site poste chaque cas de triche dans le salon des alertes (avec le jeton du bot, jamais par le webhook qui ne peut pas porter de boutons) avec deux boutons : « Bannir » (`gsf_case:ban:{alert}`) et « Ce n'est rien » (`gsf_case:dismiss:{alert}`), plus un bouton lien « Ouvrir le dossier » si l'adresse du site est en https. Le récapitulatif de plusieurs cas n'a pas de boutons.

- « Bannir » : le bot demande d'abord une confirmation visible seulement par celui qui clique (« Confirmer le ban » `gsf_case:ban-confirm:{alert}` / « Annuler » `gsf_case:ban-cancel:{alert}`), puis appelle cette route avec `action: "ban"`. « Ce n'est rien » l'appelle directement avec `action: "dismiss"`.
- Requête : `{"discord_id": "<Discord de la personne qui a cliqué>", "action": "ban" | "dismiss"}`. Le bot n'envoie la requête qu'une fois (pas de nouvelle tentative).
- `200` : `{"data": {"status": "banned" | "dismissed", "message": "<phrase à afficher telle quelle>", "decided_by": "<identifiant site>"}}`.
- `403 not_allowed` : le Discord n'est lié à aucun compte actif qui juge les cas (officier ou admin) — message : « Tu dois être officier sur le site et avoir lié ton Discord. »
- `409 already_decided` : le cas a déjà été jugé ; `message` dit par qui et quoi, `details: {"decided_by", "outcome": "banned" | "confirmed" | "dismissed"}`.
- `409 not_possible` (ban impossible, personnage inconnu), `404 not_found` (cas inexistant), `422 validation_failed`.
- Le site applique la même décision que sur le dossier (« Classé depuis Discord par <identifiant> » pour « Ce n'est rien »), l'inscrit au journal avec la personne comme auteur, puis modifie le message Discord de l'alerte : boutons retirés, ligne « ✅ Classé par X » ou « ⛔ Banni par X » avec l'heure, couleur grise. Cette mise à jour a lieu aussi quand le cas est jugé sur le site.

---

# Ajout addon ≥ 0.17.53 : relais des audits et dernière connexion (champs SavedVariables → GuildData)

- `auditReports[i].relayedBy` (chaîne, optionnelle) : nom du membre GSF qui a porté l'audit jusqu'à l'officier quand le
  joueur audité n'a croisé aucun officier. Absent = reçu directement du joueur audité (authentifié par le jeu).
  gsf-core : `AuditReport.relayed_by: Option<String>` ; site : `audit_reports.relayed_by`, alerte marquée « relayé par X, non vérifié ».
- `bridgeRoster[clé].lastOnlineAt` (nombre, heure serveur, optionnel) : dernière connexion connue par le serveur
  (`GetGuildRosterLastOnline`, ou maintenant si en ligne), relevée par un officier. gsf-core : `BridgeRosterEntry.last_online_at: Option<i64>`.
  Avec `memberPresence[clé].lastSeen` (dernier contact de l'addon), le site signale « connecté sans que l'addon soit vu »
  quand `last_online_at` est plus récent que le dernier contact addon d'au moins 24 h (seuil réglable) : signal « À surveiller », jamais une preuve.

---

# Ajout bridge 0.5.0 : bridges « vigie » (plan « vigies » du 2026-10-07, hors dépôt)

Voir ce plan pour le contrat complet de `POST /api/v1/bridge/devices` (enregistrement sans connexion,
statuts `pending`/`approved`/`device_refused`/`device_secret_mismatch`) et `GET /api/v1/bridge/pushed-data`
(`collector: true`). Ajouts côté modèle `GuildData` (gsf-core), additifs, `MODEL_VERSION` inchangé :

- `bridgeCarriedAudits[player:id]` (addon ≥ 0.18.0, store-and-forward par un non-autorité, voir `Audit.lua` de
  `guild-self-forever`) : audit en attente porté pour un autre joueur, pas encore remis à une autorité. Chaque
  entrée : `{player, id, payload, carriedAt}`, `payload` = forme sérialisée opaque de l'addon (`SerializeAudit`),
  jamais décodée côté bridge. gsf-core : `GuildData.bridge_carried_audits: Option<Vec<CarriedAudit>>`
  (`CarriedAudit { player: String, id: String, payload: String, carried_at: Option<i64> }`), `None` si aucun
  profil n'en a, dédoublonné par `(player, id)` (le `carriedAt` le plus récent gagne). Côté site : ingéré comme
  des audits relayés (`relayed_by` = porteur), jamais en double d'un audit déjà reçu directement d'un officier.

## Décisions d'implémentation (site web-gsf, branche `feat/vigie-bridges`, phase 1)

Précisions apportées en implémentant le site ; le bridge (phase 2) et l'addon (phase 3) doivent s'y conformer.

1. **`POST /api/v1/bridge/devices`** : `device_id` validé comme UUID (`uuid` rule Laravel) ; `secret` haché en
   SHA-256 (`hash_equals`, pas de coût bcrypt — ce n'est pas un mot de passe humain) ; `device_name` et chaque
   entrée de `characters` nettoyés des caractères de contrôle et tronqués (64 / 80 car.) avant stockage. Les
   champs (nom, personnages, version, dernière visite) sont mis à jour à **chaque** appel réussi (secret valide),
   quel que soit le statut (`pending`/`approved`/`refused`), pas seulement au premier.
2. **Token vigie** : `vigies` (compte de service, rôle `member`, `is_active = false`) créé à la demande
   (`ApiTokens::vigiesUser()`), jamais promu par les promotions en jeu (rôle `member` fixe, aucun personnage ne
   lui est lié). Un appel `approved` révoque systématiquement le token précédent de l'appareil (`BridgeDevice.token_id`)
   avant d'en émettre un nouveau, même si le précédent était déjà révoqué.
3. **Middleware `bridge`** : un token dont le propriétaire n'est ni officier/admin actif est cherché dans
   `bridge_devices` par `token_id` (égal à l'id du token Sanctum courant) ; approuvé → passe (et met à jour
   `last_seen_at`) ; sinon → token supprimé, `403 bridge_access_revoked`. Le compte `vigies` échoue toujours
   `canUseBridge()`/`canAccessAdmin()` : un token vigie ne passe jamais un contrôle réservé au staff ailleurs.
4. **Résolution de l'appareil à l'ingestion** : `BridgeSnapshotController` retrouve l'appareil par le **nom du
   token courant** (`bridge:vigie:<device_id>`), pas par une relecture de `BridgeDevice.token_id` (qui peut avoir
   changé depuis un précédent envoi) ; l'id trouvé est mémorisé sur `bridge_uploads.bridge_device_id` (nouvelle
   colonne, nulle pour un envoi d'officier) pour un lien stable même après réémission du token.
5. **Ingestion restreinte** : `CharacterSync` reçoit un drapeau `restricted` qui saute uniquement le bloc
   rang/`rank_name`/détection de changement (classe, niveau, dernière connexion, aventure, version d'addon
   restent à jour) ; `BankMirror` et `RequestMirror` (demandes **et** métiers) ne sont simplement pas instanciés.
   `AuditEvidence` (audits, tombstones, violations) tourne normalement, vigie ou officier.
6. **`bridge_carried_audits`** : `payload` string opaque → seuls `report_id`/`player_key`/`relayed_by`/
   `reported_at` (= `carried_at`) sont renseignés, le reste des champs structurés (or, objets…) reste `null` ;
   la chaîne brute est gardée dans `audit_reports.carried_payload` (nouvelle colonne, texte, nulle sinon) pour
   une relecture manuelle future. Par robustesse, si `payload` est déjà un objet décodé (format alternatif), ses
   champs sont repris comme pour une entrée normale de `audit_reports`. Porteur (`relayed_by`) déterminé depuis
   le dernier dossier personnage de `source.character_files` de l'envoi, à défaut le nom du PC de la vigie.
7. **Liaison vers un rang staff (`LinkOutboxEntry.outcome = 'awaiting_confirmation'`)** : évaluée sur le rang
   **déjà connu en base** du personnage expéditeur (`guild_ranks.site_role` du `guild_rank_index` actuel), jamais
   mis à jour par l'envoi vigie courant (garde-fou n°5). Le code de liaison n'est ni consommé ni vérifié
   (expiration incluse) tant qu'aucune décision admin n'est prise. Confirmation (`POST
   /admin/bridges/vigies/links/{entry}/confirm`) : consomme le code en ignorant son expiration, applique la
   liaison exactement comme `ConsumeLinkCode` pour un officier (même déclenchement de `ApplyRankChanges`).
   Refus (`.../refuse`) : code laissé intact (`LinkOutboxEntry.outcome = 'confirmation_refused'`, non terminal :
   un nouvel envoi du même code sera réévalué comme les autres issues non « linked »). Limite connue : si
   `LinkCode` est purgé avant la décision (`model:prune`, 8 jours), la confirmation échoue (`link_code_unknown`) ;
   la liaison vigie→staff n'est pas exemptée de cette purge.
8. **Permission `vigies.manage`** (admin et officier) : ouvre aussi la page Bridges (désormais gatée sur
   `vigies.manage` plutôt que `users.manage`, un officier y voit donc les mêmes informations qu'un admin sur les
   bridges officiers, en plus de la section Vigies).
9. **`collector`** sur `GET /bridge/pushed-data` : champ top-niveau `"collector": true` (toujours vrai, tout
   bridge autorisé) en plus d'être inclus dans le contenu haché pour `version`.

---

# Ajout bridge (branche `feat/requests-deaths-statuses`, plan
contrat du 2026-10-15, hors dépôt) : demandes, statuts Tricheur/À bannir, courrier, morts

Additifs côté `GuildData` (gsf-core) et `PushedData` (gsf-sync), `MODEL_VERSION` inchangé. Les noms de champs
sont normatifs (addon, bridge, site identiques) ; voir le plan pour les sections A (SavedVariables addon) et D
(règles métier) qui ne touchent pas le bridge directement.

## Snapshot bridge → site (`GuildData`, additifs)

- `requests[]` : `released_by: Option<String>`, `released_at: Option<i64>` (`releasedBy`/`releasedAt`, déjà
  décodés depuis les lignes de sauvegarde à 17 champs par `wire::decode_request_line` mais jusqu'ici non
  reportés — désormais lus aussi depuis la table `requests` du profil de guilde) ; `close_reason:
  Option<String>` (`closeReason` = `"expired"` | `"death"` | `"purge"` | absent, `status` reste `"cancelled"`).
  `close_reason` n'existe **que** dans la table `requests` du compte : le format des lignes de sauvegarde
  (13/15/17 champs) ne le porte pas, donc toujours `null` sur un `backup_requests[]`.
- `request_tombstones[]` (nouveau) : `{id, status, updated_at}` depuis `requestTombstones[id] = {status,
  updatedAt}`, fusionné entre profils de guilde, le plus récent `updated_at` gagnant par identifiant.
- `request_purge_before: Option<i64>` (nouveau) : depuis `requestPurgeBefore`, le plus récent (max) de tous
  les profils.
- `deaths[]` (nouveau) : `{id, name, realm, guild_key, guild_name, class_file, race, level, zone, killer,
  killer_kind, at, reporter}` depuis `deathLog[id]`, dédupliqué par identifiant (le `at` le plus récent gagne,
  à égalité le profil actif prime comme partout ailleurs dans gsf-core).
- `adventure_roster[]` : `flagged_at: Option<i64>`, `flagged_by: Option<String>`, `flag_source: Option<String>`
  (`flaggedAt`/`flaggedBy`/`flagSource` = `"audit"` | `"officer"` | `"site-ban"`) ; `status` peut désormais
  valoir `CHEATER` ou `TO_BAN` (valeur passée telle quelle, pas de validation côté bridge).

Exemple (`cargo run -p gsf-core --example dump`) :

```json
{
  "requests": [{ "id": "r1", "status": "cancelled", "released_by": "Ann", "released_at": 9,
                 "close_reason": "expired", "…": "…" }],
  "request_tombstones": [{ "id": "r9", "status": "expired", "updated_at": 50 }],
  "request_purge_before": 1700000000,
  "deaths": [{ "id": "realm-100", "name": "Ann Bee", "realm": "Realm", "guild_key": "realm#g",
               "guild_name": "G", "class_file": "MAGE", "race": "Human", "level": 60,
               "zone": "Orgrimmar", "killer": "Falling", "killer_kind": "environment",
               "at": 1700000100, "reporter": "Ann Bee" }],
  "adventure_roster": [{ "status": "CHEATER", "flagged_at": 1700000000, "flagged_by": "Off Icer",
                         "flag_source": "audit", "…": "…" }]
}
```

## Données poussées (§3, `PushedData`, additifs)

Réponse `GET /api/v1/bridge/pushed-data` : `requests: {expiry_days}` (objet, optionnel) et `cleared:
[{name, at}]` (liste, défaut vide), en snake_case comme `allied_guilds`/`federation`.

- `requests.expiry_days` (entier, 1 à 60, défaut 7 côté site) : réglage partagé des officiers pour
  l'expiration automatique des demandes (`requestExpiryDays` côté addon, lu puis écrit localement — **pas**
  resynchronisé dans le snapshot). Hors bornes ou absent → le bridge écarte toute la clé `requests` (journalisé,
  pas d'erreur).
- `cleared` : joueurs dont un signalement a été classé « Ce n'est rien » par le site dans les 30 derniers
  jours ; un officier qui lit `flaggedAt <= at` pour un joueur `CHEATER` le repasse `ENROLLED` (logique addon,
  hors scope bridge). Mêmes règles de nom que `bans` (≤ 64 octets, sans caractère de contrôle ni `|`) ; `at`
  négatif rejeté ; `MAX_CLEARED = 500` entrées (défensif, le site filtre déjà sur 30 jours).
- Les deux clés font partie du hash `version` côté site (comme `allied_guilds`/`federation`).

Le bridge écrit `GSF_PushedData = { …, requests = { expiryDays = <expiry_days> }, cleared = { { name = "Nom",
at = <epoch> }, … } }` (`requests` omise si absente/invalide, `cleared` toujours présente, même vide).

Exemple (`cargo run -p gsf-sync`, voir `to_addon_value`) :

```lua
GSF_PushedData = {
    schema = 1,
    guildKey = "realm#home",
    bans = {},
    alliedGuilds = {},
    requests = {
        expiryDays = 7,
    },
    cleared = {
        [1] = { name = "Ann Bee", at = 1700000000 },
    },
}
```

## Tombola du streamer (§3, `PushedData`, additif, contrat 2026-10-16)

Réponse `GET /api/v1/bridge/pushed-data` : clé `tombola` **optionnelle**, présente seulement quand une tombola a ses
inscriptions ouvertes ou qu'un lot gagné reste à envoyer (absente sinon, et alors hors du hash `version`, pour ne
pas faire réécrire le fichier).

```json
"tombola": {
  "id": 12,
  "announce": { "text": "Tombola de Brindille en cours : inscris-toi sur le Discord (salon tombola). Tirage le 20/10 à 21h00.", "ends_at": 1800000000 },
  "to_send": [
    { "lot_id": 34, "kind": "item", "label": "Cape de factionnaire", "item_id": 2059, "count": 1, "guild_key": "realm#guilde",
      "location": "Mirabel France · Banque de guilde — onglet 2", "winner": "Brindefer", "drawn_at": 1800000100 },
    { "lot_id": 35, "kind": "free", "label": "500 pièces d'or", "item_id": null, "count": 1, "guild_key": null,
      "location": "Lot du streamer", "winner": "Brumelle", "drawn_at": 1800000200 }
  ]
}
```

- `id` : identifiant de la tombola aux inscriptions ouvertes (null sinon) ; l'addon s'en sert pour ne diffuser
  l'annonce qu'une fois par tombola.
- `announce` (null hors inscriptions) : `text` ≤ 140 **octets** (message du jour GSF de l'addon, `MOTD_MAX`), sans
  `|` ; `ends_at` = date du tirage, ou publication + 7 jours sans date. Texte invalide → annonce écartée.
- `to_send` : lots tirés avec un gagnant et pas encore marqués « Envoyé » sur le site (50 au plus), `kind` `item`
  (objet de la banque, `item_id` renseigné) ou `free` (lot du streamer, sans objet). `winner` suit les règles de nom
  des bans (≤ 64 octets, sans caractère de contrôle ni `|`) ; `label` ≤ 160 octets, `location` ≤ 200 octets.
  Entrée invalide → écartée seule.

Le bridge écrit (clés camelCase, `guild_key` non recopiée) :

```lua
tombola = {
    id = 12,
    announce = { text = "Tombola de Brindille en cours…", endsAt = 1800000000 },
    toSend = {
        { lotId = 34, kind = "item", label = "Cape de factionnaire", itemId = 2059, count = 1,
          location = "Mirabel France · Banque de guilde — onglet 2", winner = "Brindefer", drawnAt = 1800000100 },
    },
},
```

`tombola` est omise s'il ne reste ni annonce ni lot valide. Un ancien bridge ignore la clé ; un ancien addon aussi.

---

# Ajout 2026-10-17 : identité Battle.net et ban « personne » (site, bridge, addon)

Objectif : deux niveaux de ban. Ban de perso (inchangé, `bans[]` par nom). Ban de la personne : ban du serveur
Discord, BattleTag et compte WoW sur liste noire, ban en cascade de tous ses persos connus ; tout perso de cette
personne, même neuf, est refusé à l'invitation. Tout est additif : un ancien bridge/addon ignore les nouvelles clés.

## Empreinte d'identité (identique dans les trois dépôts)

- BattleTag lu par l'addon du joueur lui-même : `BNGetInfo()` (2e valeur), jamais celui d'un autre joueur.
- Normalisation : espaces retirés en tête et en fin, puis minuscules **ASCII seulement** (octets `A`-`Z` → `a`-`z`,
  les octets UTF-8 non ASCII restent tels quels, comme `string.lower` Lua 5.1 et `strtolower` PHP 8).
- `bt` = 32 premiers caractères hexadécimaux minuscules de `sha256("GSF-ID-v1|" .. btag_normalisé)` (UTF-8).
- Vecteur de test : `"  Davin#54321 "` → normalisé `"davin#54321"` → `bt = "fa2ce2459aec6c5c0d72ca5991cd4cfd"`.
  SHA-256 de `"abc"` = `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.
- `acct` = identifiant de compte WoW : 16 caractères hexadécimaux minuscules tirés au hasard une seule fois par
  l'addon et gardés dans les SavedVariables de compte (partagées par tous les persos du compte). Pas un secret.
- Le BattleTag en clair n'est jamais stocké ni envoyé par l'addon. Côté site, un BattleTag saisi par un admin est
  stocké chiffré (`encrypted`) et son `bt` calculé côté serveur avec la même normalisation.

## SavedVariables (addon → bridge)

Dans le profil de guilde (même niveau que `memberPresence`) :
`memberIdentity[<clé perso, comme memberPresence>] = { bt = "<32 hex>"|nil, acct = "<16 hex>"|nil, at = <epoch> }`.
Rempli par l'officier à réception du message d'identité d'un membre, et pour ses propres persos. Plus récent `at` gagne.

## Snapshot (bridge → site), `GuildData`

`member_identities: [{ "character": <même référence de perso que les autres listes>, "bt": "<32 hex>"|null,
"acct": "<16 hex>"|null, "at": <epoch> }]` (défaut liste vide). Le bridge valide le format (hex strict, longueurs
exactes) et écarte une valeur invalide (champ à null), jamais toute l'entrée. Plafond 2000 entrées.
Le site : une observation plus ancienne (`at`) n'écrase jamais une plus récente.
Précision : `at` absent côté SavedVariables (ne devrait pas arriver, l'addon l'écrit toujours) → le bridge envoie
`0`, jamais d'erreur ni d'entrée écartée (même règle de tolérance que les autres champs du modèle).

## Données poussées (site → bridge → addon)

Réponse `GET /api/v1/bridge/pushed-data` : `banned_identities: [{ "bt": "<32 hex>"?, "acct": "<16 hex>"?,
"reason": "…"?, "expiresAt": <epoch>? }]` (liste, défaut vide ; au moins un de `bt`/`acct` ; mêmes règles de
`reason` que `bans`), dans le hash `version`. Plafond 500. Le bridge écrit
`GSF_PushedData.bannedIdentities = { { bt = "…", acct = "…", reason = "…", expiresAt = … }, … }` (toujours présente, même vide).

## Messages addon (nouveaux, commandes inconnues ignorées par les anciens addons)

- `IDN|<bt ou ->|<acct>` : identité d'un membre, envoyée en **chuchotement aux officiers en ligne** uniquement
  (jamais sur `GUILD`), à la connexion et en réponse au `HELLO` d'un officier. `-` = BattleTag indisponible.
- `INVID|<nonce>|<bt ou ->|<acct>` : envoyé par le candidat à côté de `INVSTAT` (message séparé : la regex
  d'`INVSTAT` est fermée par `$`, un champ en plus casserait les anciens recruteurs).
- Recruteur : identité bannie (`bt` ou `acct` dans `bannedIdentities`) → pas de popup, refus neutre au candidat,
  alerte à l'officier. **Identité absente (`bt` = `-` ou pas d'`INVID`) → refus** (décision 2026-10-17), avec un
  message au candidat : connecter Battle.net / mettre l'addon à jour.

## Complément 2026-10-17 (b) : BattleTag en clair visible des admins (décision utilisateur)

Le BattleTag en clair remonte désormais jusqu'au site, affiché aux **admins seulement** (stocké chiffré).

- Addon : nouveau message `IDB|<battletag brut>` envoyé juste après chaque `IDN`, aux mêmes destinataires
  (chuchotement aux officiers en ligne uniquement, jamais `GUILD`). Message séparé : `IDN` reste inchangé pour
  ne pas casser les officiers en 0.21.0 (commandes inconnues ignorées). BattleTag brut tel que `BNGetInfo()`
  le donne, rogné ; rejeté s'il dépasse 64 octets, contient un caractère de contrôle ou `|`, ou n'a pas de `#`.
- Officier : `memberIdentity[clé].btag = "<battletag brut>"`, gardé seulement si
  `bt == hash(btag)` (même normalisation et hash que plus haut) ; sinon ignoré. Le propre perso de l'officier
  aussi. Un `IDB` sans `IDN` déjà enregistré pour ce perso est ignoré.
- Snapshot : `member_identities[].btag` (chaîne, optionnelle, null si absente ou invalide : ≤ 64 octets,
  pas de contrôle, contient `#`).
- Site : garde le BattleTag seulement si `BattleTag::hash(btag) === bt` de la même entrée ; stocké chiffré sur
  le personnage (`characters.battletag_encrypted`), même règle « plus récent `at` gagne ». Affiché aux admins
  (fiche joueur, écran « Bannir la personne » pré-rempli, ban de la personne) ; jamais aux officiers, jamais
  dans les données poussées.

---

# Ajout 2026-10-31 : bridge « membre » (étape 1 du plan « Le bridge pour tous »)

Résumé de ce qui concerne le bridge (le reste est côté site) :

- **Connexion** : même flux loopback (§1) sur `/bridge/connect?port=…&state=…&device=…`. Le site y propose
  « Se connecter avec Discord » à tout membre lié à un personnage d'une guilde suivie et non banni, ou
  l'identifiant du site pour un officier. La réponse de `POST /api/v1/bridge/token` est inchangée ; un membre
  reçoit un jeton de 30 jours (`bridge:member:<appareil>`), un officier un jeton de 60 jours. `user.roles`
  contient `officer` ou `admin` pour un officier, sinon le bridge affiche « Membre ».
- **Accès** : un jeton membre est révoqué dès que le compte ne peut plus utiliser le bridge (ban, perso délié) :
  `403 bridge_access_revoked`, le bridge efface le jeton et redemande la connexion.
- **Données poussées à un membre** : `schema`, `guild_key`, `generated_at`, `collector: false`,
  `allied_guilds`, `requests`, `bans`, `version` seulement (jamais `federation`, `banned_identities`,
  `game_blacklist`, `blacklist_receipts`, `cleared`, `tombola`).
- **Ingestion d'un envoi de membre** : le site ignore les identités (`member_identities`) et les liaisons
  (`bridge_link_outbox`) ; les preuves sont toujours marquées « non vérifiées ».
- **Mises à jour sans jeton** : `GET /api/v1/bridge/public/update/{target}/{arch}/{current}` et
  `GET /api/v1/bridge/public/download/{version}/{file}`, sans authentification, 30 requêtes/min/IP, mêmes
  réponses que les routes authentifiées (§4).
- **Plus d'enregistrement anonyme** : un bridge neuf n'appelle plus `POST /api/v1/bridge/devices` ; seul un
  bridge qui possède déjà un secret d'appareil « vigie » continue de l'utiliser.

---

# Ajout 2026-11-02 : présence et couverture (bridge pour tous, étape 2)

Plan « Le bridge pour tous » (hors dépôt), étape 2. Tout est additif : un ancien bridge
n'envoie simplement pas de signal de vie, un ancien addon n'écrit pas de sessions, `MODEL_VERSION` inchangé.

## Signal de vie : `POST /api/v1/bridge/heartbeat` (`bridge:read`)

- Corps JSON : `{"game_running": true|false, "guild_key": "<clé de la guilde active du PC>"}` ; `guild_key` facultative
  (absente tant que le bridge ne connaît pas encore de guilde active), normalisée comme `guild_key_normalized`.
- Réponse : `204` sans corps. Mêmes garde-fous que les routes de données : middleware `bridge` (`403
  bridge_access_revoked` => le bridge efface le jeton), `X-Bridge-Version` sous la version minimale => `426
  bridge_outdated`, `422 validation_failed` (`game_running` manquant ou non booléen, `guild_key` sans `#` ou > 160
  car.), limite `bridge` (30/min par jeton).
- Envoyé par **tout** bridge connecté, quel que soit son niveau de confiance (officier, vigie, collecteur de confiance,
  membre) : le site déduit le niveau du jeton (`BridgeTrust`), jamais du corps.
- Fréquence (bridge) : au démarrage, à la connexion, toutes les 5 minutes, et à la minuterie d'une minute dès que le jeu
  s'ouvre ou se ferme. Jamais mis en file : un signal raté est remplacé par le suivant.
- Site : un intervalle par bridge (`bridge_presences`, clé = jeton) prolongé par chaque signal ; un trou de plus de
  12 min, un changement de `game_running`, de guilde (résolue parmi les guildes suivies, sinon nulle) ou de niveau
  ouvre un nouvel intervalle. Chaque signal vaut pour les 5 minutes qui suivent (statistiques). Lignes purgées à 90 jours.

## Sessions des collecteurs (addon ≥ 0.22.8 → `GuildData.collector_sessions`)

SavedVariables, dans le profil de guilde, écrites seulement pour un personnage collecteur (`GSF:IsBridgeCollector()` :
autorité de la guilde configurée, ou données poussées `collector = true`) :

```lua
collectorSessions["<clé perso>:<startedAt>"] = {
    character = "<clé perso, comme memberPresence>", name = "Nom affiché",
    startedAt = <epoch serveur>, seenAt = <epoch serveur>, endedAt = <epoch serveur> | nil,
}
```

- `seenAt` avance chaque minute tant que la session est ouverte ; `endedAt` posé à `PLAYER_LOGOUT` (aussi au
  `/reload`, la session est rouverte au chargement suivant si elle s'est fermée il y a moins de 3 min) ; `endedAt` nil
  après un plantage. 30 jours, 200 sessions au plus.
- gsf-core : `collector_sessions: [{"character": <CharacterRef>, "name": "…"|null, "started_at": <epoch>,
  "seen_at": <epoch>|null, "ended_at": <epoch>|null}]` (liste, défaut vide), fusionné entre profils par (personnage,
  `started_at`), le plus récent `seen_at`/`ended_at` gagne ; entrée sans `character` ou sans `startedAt` > 0 écartée.
- Site : seulement depuis un envoi de niveau collecteur (`officer`, `vigie`, `trusted_member`) ; ignoré pour un
  `member`. Une ligne par (personnage résolu, `started_at`) ; fin = `ended_at`, à défaut `seen_at`, à défaut
  `started_at` ; heures bornées par le plafond d'ingestion (jamais dans le futur), session bornée à 48 h ; une fin plus
  ancienne n'écrase jamais une fin plus récente. Les sessions n'arrivent qu'après la déconnexion : elles servent aux
  statistiques (bilan horaire recalculé), jamais à l'alerte en direct.

## Couverture (site)

- Collecteur en jeu, minute par minute = le plus grand de : bridges de collecteurs avec le jeu ouvert, sessions de
  l'addon en cours (jamais additionnés, pour ne pas compter deux fois la même personne). Minute sans personne = 0.
- Alerte Discord (salon officiers) : 15 min après le dernier signal d'un bridge de collecteur jeu ouvert, 24 h/24, un
  seul message « Personne ne surveille la guilde depuis HH:MM : … », modifié en « Couverture revenue à HH:MM (trou de
  X min, depuis HH:MM). » au premier signal suivant d'un collecteur jeu ouvert. Aucune alerte tant qu'aucun collecteur
  n'a jamais envoyé de signal jeu ouvert.

---

# Ajout 2026-11-02 (b) : montée en charge et accusé de réception (bridge pour tous, étape 5)

Plan « Le bridge pour tous » (hors dépôt), étape 5. Tout est additif : un ancien bridge
n'envoie pas `If-None-Match` et ignore les nouvelles clés, un ancien addon ignore les nouvelles commandes et clés,
`MODEL_VERSION` et `schema` inchangés.

## Données poussées : ETag / 304 (`GET /api/v1/bridge/pushed-data`)

- Toute réponse `200` porte l'en-tête `ETag: "<version>"` (même valeur que le champ `version`).
- Requête avec `If-None-Match: "<version>"` (guillemets et préfixe `W/` tolérés, liste séparée par des virgules
  acceptée) égal à la version courante → `304` sans corps (en-tête `ETag` répété). Mêmes garde-fous qu'avant (403, 426,
  404 `guild_not_tracked`, 409) : ils passent avant la comparaison.
- Bridge : envoie `If-None-Match` avec la `version` des données qu'il a **déjà écrites** (`state.pushed_version`)
  seulement si le fichier `GuildSelfForever_Data/Data.lua` existe dans chaque dossier d'addon cible et qu'aucune donnée
  reçue n'attend la fermeture du jeu ; sinon requête complète. `304` = rien à écrire, site joignable, pas de ligne de
  journal à chaque fois.

## Données poussées : contenu par niveau

**Membre simple (`collector: false`)**, allégé : `schema`, `guild_key`, `generated_at`, `collector`, `allied_guilds`,
`requests`, `bans` (**noms seulement** : `{"name", "expiresAt"?}`, jamais `reason`), `received_violations` (seulement
les violations de ses propres persos, voir plus bas), `version`. Le site ne calcule plus le reste pour un membre.

**Collecteur (`collector: true`)** : inchangé + `collector_proofs` + `received_violations`.

### `received_violations` (accusé de réception du site)

- Liste de chaînes : identifiants (`violation_id`, forme `kind-at-hash` de l'addon) des violations **que le site a
  reçues**, dont l'heure (`occurred_at`, à défaut la réception) date de moins de 30 jours, les plus récentes d'abord.
  Collecteur : toutes (2000 au plus). Membre : seulement celles dont le joueur est un de ses persos liés (200 au plus).
- Dans le hash `version`. Bridge : garde chaque identifiant de 1 à 128 octets fait de `[A-Za-z0-9_.:-]`, écarte les
  autres un par un, 2000 au plus ; écrit `receivedViolations = { "id", … }` (toujours présente, même vide).

### `collector_proofs` (collecteur prouvé)

- Liste `[{"name": "<clé perso>", "expires_at": <epoch>, "sig": "<512 hex>"}]`, une entrée par perso du collecteur
  (20 au plus) : persos liés du compte (guilde suivie, sans ban actif) pour un jeton officier ou membre de confiance ;
  persos déclarés de l'appareil (`bridge_devices.characters`, normalisés) pour une vigie. `name` = clé du perso comme
  `characters.name_key` / `GSF:CleanName` (minuscules ASCII de `string.lower`).
- `expires_at` = début du jour UTC courant + 8 jours (change une fois par jour : la `version` ne bouge qu'une fois par
  jour pour un collecteur ; une preuve vaut au moins 7 jours après la dernière écriture du bridge).
- `sig` = signature RSA 2048 bits, exposant 65537, PKCS#1 v1.5 type 1 **sans** DigestInfo :
  `EM = 00 01 FF…FF 00 || SHA-256(message)` (256 octets), en hexadécimal minuscule (512 caractères), avec
  `message = "GSF-COL-v1|" .. name .. "|" .. expires_at` (UTF-8, `expires_at` en décimal). Site :
  `openssl_private_encrypt(hash('sha256', $message, true), $sig, $key, OPENSSL_PKCS1_PADDING)`.
- Clé privée : réglage chiffré du site `collector_signing_key` (PEM), créée une fois par
  `php artisan gsf:collector-key`, qui affiche aussi la clé publique (module hexadécimal) à recopier dans l'addon. Sans
  clé réglée, `collector_proofs` est vide.
- Dans le hash `version`. Bridge : garde une entrée si `name` fait 1 à 64 octets sans caractère de contrôle ni `|` ni `~`,
  `expires_at` > 0, `sig` exactement 512 caractères `[0-9a-f]` ; écarte les autres une par une, 20 au plus ; écrit
  `collectorProofs = { { name = "…", expiresAt = …, sig = "…" }, … }` (toujours présente, même vide).

## Snapshot d'un membre simple (bridge)

Quand le niveau connu du bridge est « membre simple » (jeton obtenu par la connexion, `user.roles` sans `officer` ni
`admin`, et dernières données poussées reçues avec `collector` différent de `true`, y compris tant qu'aucune n'a encore
été reçue), le bridge n'envoie de chaque guilde que : `guild_key_normalized`, `guild_name`, `realm`, `source_keys`,
`active`, `writer_version`, `writer_saved_at`, `audit_reports`, `audit_pending`, `bridge_violations`,
`bridge_local_violations`, `bridge_carried_audits`, `deaths`, `presence_gaps`, `crafted_sightings`, `self_identity`
(contrat 2026-11-10, bridge 0.8.0). Les autres clés sont absentes (le site les ignore déjà pour un membre),
notamment `guild_bank_log` : collecteurs seulement, jamais envoyé par un membre simple. `snapshot_hash` est calculé sur
ce contenu filtré. Un officier, une vigie ou un membre de confiance (`collector: true`) envoie tout, comme avant.

## Ingestion (site)

- Roster (`members`, `adventure_roster`, `bridge_roster`) : seulement depuis un envoi d'officier, de vigie ou de membre
  de confiance (inchangé) ; jamais d'un membre simple.
- Regroupé par guilde : une seule ingestion à la fois par guilde (verrou par `guild_key`, les autres attendent) ; un
  roster identique (même empreinte de `bridge_roster`, `bridge_roster_at`, `members`, `adventure_roster`) à celui du
  dernier envoi déjà traité de la même guilde, venant d'une source au moins aussi fiable (un officier pour un officier),
  n'est pas réappliqué (`bridge_uploads.roster_hash`).
- Fichier brut d'un envoi de membre simple : supprimé après 14 jours (la ligne d'envoi reste 90 jours) ; 90 jours pour
  les autres.
- Files : `ingest` (ingestion, tribunal, bilan de présence) et `discord` (appels Discord) quand `QUEUE_SPLIT=true`
  (sinon tout reste sur `default`).

## Addon (≥ 0.22.9) : accusé de réception, tirage, collecteur prouvé

### Collecteur prouvé

- Clé publique du site dans le code de l'addon (`CollectorProof.lua`, `local SITE_KEY_MODULUS = "<module hex>"`, ligne
  affichée par `php artisan gsf:collector-key`). **Vide = ancien comportement** (`BRCOL|1` déclaratif cru). Clé présente : un pair n'est collecteur (`IsCollectorPeer`) que s'il a
  envoyé `BRCOL|1` récemment **et** une preuve valide pour **son propre nom** (`CleanName(sender)`), non expirée.
- `BRPRF|<jeton>|i/n|<partie>` (découpé comme les audits, `SendChunked`/`AcceptChunk`) ; contenu reconstitué =
  `<name>~<expiresAt>~<sig>`. Envoyé par un collecteur qui a une preuve non expirée pour le perso connecté : sur `GUILD`
  après le chargement du roster puis au plus toutes les 30 min, et en chuchotement en réponse à `BRPRQ`.
- `BRPRQ|1` : chuchoté à un pair vu avec `BRCOL|1` sans preuve vérifiée (au plus une fois par pair et par 5 min).
  Réponse limitée à une par demandeur et par minute.
- Vérification : `name == CleanName(sender)`, `now < expiresAt <= now + 30 j`, signature valide (calcul RSA en Lua pur,
  ~20 ms, une fois par pair et par preuve, résultat gardé en mémoire ; une vérification au plus par pair et par minute).
- `FEDCFG` accepté seulement d'une autorité ou d'un collecteur prouvé (clé vide : ancien comportement).

### Répartition des signalements

`GetBridgeRelayTargets(salt, exclude)` : autorités et collecteurs (prouvés) en ligne, mélangés par tirage
(`StableHash(tostring(StableHash(salt .. "|" .. clé)) .. "|" .. clé)` : deux passes, car `StableHash` seul garde presque
le même ordre quel que soit le sel ; `salt` = identifiant du signalement et numéro d'essai, le code pour un lien), 3 au
plus, ceux de `exclude` (déjà essayés) en dernier. Renvoie les noms, puis les clés. Remplace « autorités puis collecteurs, 5 premiers par ordre alphabétique ».

### Accusé de réception de bout en bout (violations `BRVIOL`)

- Signaleur (journal `bridgeLocalViolations[id]`, inchangé : 50 entrées, 30 jours) : garde chaque violation jusqu'à
  l'accusé du site. Nouveaux champs : `sentTo = { ["<clé>"] = <epoch> }` (collecteurs ou autorités à qui elle a été
  remise), `siteAt` (accusé du site reçu), `attempts`, `nextTryAt`. Sans accusé 24 h après le dernier envoi, renvoi à
  d'autres collecteurs (tirage en évitant `sentTo`), 6 envois au plus. `relayed` garde son sens (remise au moins une fois). Une violation déjà relayée par un addon ≤ 0.22.8 attend 24 h à partir de sa propre
  heure avant un renvoi (pas de rafale à la mise à jour).
- Accusé : `BRVOK|<id>` chuchoté par un collecteur ou une autorité dont les données poussées listent `id` dans
  `receivedViolations`, en réponse à un `BRVIOL` du joueur concerné ; accepté seulement en chuchotement, d'une autorité
  ou d'un collecteur (prouvé quand la clé est réglée). Un
  signaleur qui a lui-même un bridge lit aussi `receivedViolations` de ses propres données poussées au chargement.
- Historique : `BRVH|<id>|<clé>@<epoch>;<clé>@<epoch>…` (4 au plus, ≤ 255 octets, clés `Encode`), chuchoté juste après le
  `BRVIOL` d'un renvoi : à qui la violation avait déjà été remise. Le collecteur ne le garde que s'il a la violation et
  que l'expéditeur en est le joueur ; `bridgeViolations[id].handedTo = { { name = "<clé>", at = <epoch> }, … }` (8 au plus).
- Un signaleur qui a un bridge l'envoie aussi directement : `bridge_local_violations` (déjà exporté, ingéré pour tout
  niveau, membre compris).

### Snapshot (`GuildData`, gsf-core)

`Violation.handed_to: [{"name": "<clé perso>", "at": <epoch>|null}]` (défaut liste vide, omis si vide) : depuis
`bridgeViolations[id].handedTo` (liste) ou `bridgeLocalViolations[id].sentTo` (table clé → epoch, `name` = clé), 8 au
plus, entrée sans nom écartée.

### Site : collecteur qui ne transmet pas

- `violation_handoffs` (violation, clé du perso collecteur, heure de remise) depuis `handed_to` ; `violations.carrier_keys`
  = clés des persos des bridges qui ont transmis la violation (persos liés du compte, ou persos déclarés de la vigie).
- Une remise est « transmise » dès qu'un envoi du bridge de ce collecteur contient la violation. Page Bridges : « Ces
  joueurs ont reçu en jeu des signalements qui ne sont jamais arrivés par leur bridge » (remises de plus de 3 jours,
  seulement pour des persos de collecteurs connus). Signal à vérifier, jamais une preuve : l'historique vient du joueur
  qui a envoyé le signalement.

---

# Ajout 2026-11-03 : « Je joue sur plusieurs ordinateurs » (site, bridge 0.7.1, addon 0.22.9)

Problème : l'addon garde sur chaque PC une photo A de chaque perso (sacs, équipement, or, banque :
`GuildSelfForeverAuditBaseline` + copie de secours `auditBaselineBackups`) et compare l'état à la connexion à A. Sur un
deuxième PC, A est vieille : tout ce qui s'est passé sur l'autre PC ressort en audit. Tout est additif et **optionnel** :
seuls les bridges dont la case « Synchroniser mon addon entre mes ordinateurs » est cochée (décochée par défaut)
envoient et reçoivent des photos ; un ancien bridge, un ancien site ou un ancien addon ne voient rien changer.

## Photo (format commun, JSON)

```json
{
  "name": "tester",
  "character": "Tester",
  "saved_at": 1800000000,
  "checksum": "1830179085",
  "adventure_id": "A1929936150",
  "guild_key": "classic beta pve#ma guilde",
  "snapshot": {
    "time": 1800000000, "gold": 562, "bag_slots": 20, "legacy": true,
    "items": [[159, 2], [6948, 1]],
    "equipment": [[4, "154:0"], [16, "36:0"]],
    "bank": { "time": 1799990000, "legacy": true, "items": [[242709, 1]] }
  }
}
```

- `name` = clé du perso (`GSF:CleanName`), 1 à 64 octets sans caractère de contrôle, `|` ni `~` ; `character` (facultatif)
  = nom affiché, ≤ 64 octets sans contrôle ni `|` ; `saved_at` > 0 (site : au plus 5 min dans le futur) ;
  `checksum` = empreinte de l'addon (`snapshotFingerprint`), 1 à 32 `[0-9A-Za-z]` ; `adventure_id` 1 à 64
  `[A-Za-z0-9_-]` ; `guild_key` = clé **telle qu'écrite par l'addon** (`guildKey` du record, pas normalisée), 1 à 160
  octets avec `#`.
- `snapshot` : `time` ≥ 0, `gold` 0 à 2147483647, `bag_slots` 0 à 1000 (défaut 0), `legacy` seulement si vrai ;
  `items` = `[identifiant 1-9999999, nombre 0-1000000]` (1000 au plus, identifiants triés et uniques) ; `equipment` =
  `[emplacement 1-23, "objet:enchantement"]` (texte ≤ 64 octets, emplacements triés et uniques) ; `bank` facultative,
  même règle pour `items`. Rien d'autre (seul ce qui entre dans `checksum` est transporté).
- Une photo dont un champ est invalide est écartée seule, jamais tout l'envoi ni toute la réponse.

## Lecture (bridge, `gsf-core` `model::baseline`)

- Sources : `GuildSelfForeverAuditBaseline` de chaque fichier de perso (nom = `character` du record, sinon nom du
  dossier du perso) et `GuildFoundForeverDB.guilds[*].auditBaselineBackups[*]` du fichier compte (nom = `character`).
- Écartée : `snapshot.captureComplete == false`, clé d'objet non entière, valeur non entière, sans `adventureID`,
  `guildKey` ou `checksum`, hors limites. Emplacements d'équipement hors 1-23 ignorés (hors empreinte). Banque sans
  `time` : heure de la photo (comme `cloneSnapshot`).
- La plus récente (`savedAt`) par perso, tous comptes WoW du PC confondus, 50 au plus (les plus récentes).

## Envoi : `POST /api/v1/bridge/audit-baselines` (`bridge:push`, routes de données : 403/426 comme ailleurs)

- Corps `{"baselines": [<photo>, …]}`, gzip accepté (comme les snapshots), 4 Mo décompressé, 50 photos au plus
  (sinon `422 validation_failed`).
- Réponse `200 {"stored": n, "kept": n, "ignored": [{"name": "…", "reason": "invalid"|"guild_not_tracked"|"not_linked"|"ambiguous"}]}`.
  `kept` = photo pas plus récente que celle déjà gardée (rien n'est écrit).
- Bridge : envoyé avec ses envois habituels (après la lecture des SavedVariables : démarrage, fichier modifié,
  connexion, « Envoyer maintenant ») et quand la case vient d'être cochée ; seulement si la case est cochée et que
  la liste a changé depuis le dernier envoi accepté (empreinte SHA-256 dans `state.json`, `audit_baselines_hash`) ;
  réseau ou 5xx : gardé en mémoire et renvoyé à la tâche d'une minute.
- Vigie (appareil sans compte) : `409 audit_sync_needs_account` (jamais 403, qui ferait effacer le jeton) ; le bridge
  le note une fois dans son journal.

## Site : garde-fous (`App\Support\Bridge\AuditBaselines`, table `audit_baseline_syncs`)

- Seulement un perso **lié au compte du jeton** (`characters.user_id`), sans ban actif, dans une guilde suivie
  (`guild_key` résolue) ; `name` comparé à `name_key` ; plusieurs persos du compte avec ce nom : départagés par le
  royaume de `guild_key`, sinon `ambiguous`.
- Une ligne par perso (`character_id` unique) : `user_id`, `saved_at`, `payload` (photo réécrite champ par champ, 64 Ko
  au plus). Une photo n'est remplacée que par une **plus récente** (`saved_at` strictement plus grand), sauf si la
  ligne venait d'un autre compte (perso relié depuis). Ligne effacée 90 jours après le dernier envoi (`model:prune`).
- Ouvert aux jetons officier, membre de confiance et membre simple (la photo passe aussi pour un membre simple, dont
  le snapshot ne porte que ses preuves : elle voyage par cette route à part, jamais dans le snapshot).

## Oubli : `DELETE /api/v1/bridge/audit-baselines` (`bridge:push`)

- Réponse `200 {"deleted": n}` : **toutes** les photos du compte du jeton sont effacées (décision : le site oublie, il
  ne se contente pas de ne plus les renvoyer). Les autres PC du compte qui ont encore la case cochée renverront les
  leurs à leur prochain envoi. Vigie : `409 audit_sync_needs_account`.
- Bridge : case décochée → `audit_forget_pending` dans `state.json`, demandé tout de suite puis à chaque minute
  jusqu'à une réponse (`200` ou `409`) ; un refus définitif (autre 4xx) abandonne.

## Données poussées : `GET /api/v1/bridge/pushed-data?audit_baselines=1`

- Paramètre envoyé seulement quand la case est cochée. Clé `audit_baselines` = liste des photos du compte du jeton
  (persos encore liés, sans ban), 50 au plus, les plus récentes d'abord ; présente (même vide) seulement si le
  paramètre est là et que le jeton a un compte (jamais pour une vigie). Pour un membre comme pour un collecteur.
  Incluse dans le hash `version` : cocher/décocher change la version, donc réécrit `Data.lua`.
- Bridge : relit les données poussées au démarrage (déjà le cas), à la connexion, quand la case change, et, case
  cochée, **à chaque minute quand le jeu est fermé** (`If-None-Match`, un `304` ne coûte presque rien) : bridge
  ouvert avant le jeu, une photo prise sur l'autre PC arrive ici en une minute environ. Écrit (jeu fermé, comme le
  reste), seulement si la clé est présente, chaque photo validée seule, 50 au plus :

```lua
GSF_PushedData.auditBaselines = {
    { name = "tester", character = "Tester", savedAt = 1800000000, checksum = "1830179085",
      adventureID = "A1929936150", guildKey = "classic beta pve#ma guilde",
      snapshot = { time = 1800000000, gold = 562, bagSlots = 20, legacy = true,
                   items = { [159] = 2, [6948] = 1 }, equipment = { [4] = "154:0", [16] = "36:0" },
                   bank = { time = 1799990000, items = { [242709] = 1 } } } },
}
```

## Addon (0.22.9, `Audit.lua`, `selectBaseline`)

- Troisième candidate « site » à côté de la copie dédiée et de la copie de secours, lue directement dans
  `GSF_PushedData.auditBaselines` (indépendamment de la validation du reste du fichier et de son `guildKey`, pour être
  prête dès la comparaison de connexion).
- Retenue seulement si : `name == CleanName(UnitName("player"))`, `character` (si présent) du même perso, `checksum`
  **présent** et égal à l'empreinte recalculée, `adventureID` **présent** et égal à l'aventure du perso (aventure
  inconnue sur ce PC = refusée), `guildKey` de la même guilde que la guilde active (comparaison sans espaces ni numéro
  de royaume relié), puis les validations habituelles (`baselineRecordValid` : forme, date ≤ maintenant + 5 min).
- La plus récente gagne (`savedAt`) ; à égalité la copie dédiée garde la place. Gagnante : recopiée dans la copie
  dédiée (`source = "site"`, `recovered`), visible dans `/gsf auditstatus`.

---

# Ajout 2026-11-03 (b) : onglet Tombola ouvert à tous (site, bridge 0.7.2, addon 0.22.10)

Décision streamer : l'onglet Tombola de l'addon n'est plus réservé aux officiers. Chaque joueur y voit la tombola en
cours, les lots où il participe et s'il a gagné, avec une pastille « +1 » pour l'amener sur la chaîne du streamer
pendant le tirage. Tout est additif : un ancien bridge ignore la clé, un ancien addon aussi ; `schema` inchangé ; la clé
`tombola` des collecteurs (annonce, lots à envoyer) ne change pas.

## Données poussées : `tombola_public` (`GET /api/v1/bridge/pushed-data`, **tout** bridge, membre simple compris)

Clé **optionnelle**, présente seulement s'il y a une tombola aux inscriptions ouvertes ou en tirage (`open`,
`drawing`) ou un lot gagné par le compte du jeton (absente sinon, et alors hors du hash `version`). Incluse dans le
hash : une inscription, un tirage, un envoi changent la version.

```json
"tombola_public": {
  "id": 12, "status": "open", "streamer": "Brindille", "twitch": "brindille", "draw_at": 1800000000,
  "lots": [
    { "lot_id": 34, "kind": "item", "label": "Cape de factionnaire", "item_id": 2059, "count": 1, "details": null,
      "min_level": 25, "drawn": false, "entered": true, "won": false },
    { "lot_id": 35, "kind": "free", "label": "500 pièces d'or", "item_id": null, "count": 1,
      "details": "Remis en jeu par Brindille", "min_level": null, "drawn": false, "entered": false, "won": false }
  ],
  "won": [
    { "lot_id": 30, "kind": "item", "label": "Bottes de marche", "item_id": 1234, "count": 1, "details": null,
      "character": "Brindefer", "drawn_at": 1799990000, "sent": false }
  ]
}
```

- `id`, `status` (`open` ou `drawing`), `streamer` (nom du camp, ≤ 64 octets), `twitch` (identifiant de chaîne mis
  en minuscules, `[a-z0-9_]`, 25 au plus, null si inconnu ou invalide), `draw_at` : la tombola en cours ; tous null sans tombola en cours.
- `lots` (50 au plus, ordre du streamer) : objet (`kind` `item`, `item_id` pour l'infobulle du jeu) ou lot libre
  (`free`, `details` ≤ 200 octets), `label` ≤ 160 octets sans `|` ni caractère de contrôle, `count` 1-1000,
  `min_level` = niveau minimum pour s'inscrire (niveau requis de l'objet − 5, null pour un lot libre), `drawn` = lot
  déjà tiré (sans dire par qui), `entered` / `won` = **pour le compte du jeton seulement**.
- `won` (20 au plus, plus récents d'abord) : lots gagnés par le compte du jeton, toutes tombolas non annulées, pas
  encore envoyés ou envoyés depuis moins de 7 jours (`sent`) ; `character` = son perso qui reçoit le lot.
- **Jamais** un autre joueur : ni nom d'inscrit, ni gagnant, ni nombre d'inscrits par compte. Une vigie (compte de
  service) reçoit la tombola en cours sans `entered`/`won` ni `won[]`.

## Bridge (0.7.2) : `tombolaPublic`

Chaque lot validé seul (écarté s'il est invalide : `lot_id` < 1, `kind` autre que `item`/`free`, libellé invalide,
`count` hors 1-1000), `details`/`character` gardés seulement s'ils sont valides, `min_level` seulement entre 1 et 100,
`status` seulement `open`/`drawing`, `twitch` seulement `[a-z0-9_]{1,25}`, booléens écrits seulement s'ils sont vrais ;
clé omise s'il ne reste ni `id` ni lot :

```lua
tombolaPublic = {
    id = 12, status = "open", streamer = "Brindille", twitch = "brindille", drawAt = 1800000000,
    lots = {
        { lotId = 34, kind = "item", label = "Cape de factionnaire", itemId = 2059, count = 1, minLevel = 25, entered = true },
        { lotId = 35, kind = "free", label = "500 pièces d'or", count = 1, details = "Remis en jeu par Brindille" },
    },
    won = {
        { lotId = 30, kind = "item", label = "Bottes de marche", itemId = 1234, count = 1, character = "Brindefer", drawnAt = 1799990000 },
    },
},
```

## Addon (0.22.10, `Tombola.lua`, `GuildUI.lua`)

- Validation additive (`BridgeExport.lua`) : entrée invalide écartée seule, jamais le reste du fichier.
- Onglet « Tombola » dans la colonne de tous : page « En cours » (streamer, date du tirage, chaîne Twitch à copier,
  lots avec « Tu participes », « Tiré », « Tu as gagné », et « Tes gains » : « un officier t'envoie ton lot par
  courrier » / « Envoyé ») ; les officiers gardent leur page inchangée dans l'onglet « À envoyer » de la même entrée.
- Pastille rouge « +N » sur le bouton de la minicarte et sur l'entrée Tombola tant qu'il y a du nouveau (tombola
  ouverte ou tirage pas encore vus, lot gagné pas encore vu) ; effacée à l'ouverture de l'onglet (vu gardé par guilde sur
  le compte WoW : tombola + statut, lots gagnés). Un message dans le chat une fois par session (connexion ou /reload) tant qu'une
  tombola est en cours ou qu'un lot gagné attend.
- Données périmées (bridge qui ne synchronise plus) : une tombola dont le tirage date de plus de 24 h, ou des données
  de plus de 14 jours (`generatedAt`), n'est plus montrée « en cours » (ni pastille ni message) ; les gains restent.
- Les données ne changent qu'après une synchronisation du bridge jeu fermé : un tirage fait pendant la partie n'arrive
  qu'au prochain lancement du jeu (le gagnant reste prévenu par MP Discord, comme avant).

---

# Ajout 2026-11-03 (c) : `/played` dans l'audit (inactivité et absences : addon 0.22.11, bridge 0.7.3)

L'addon note le temps de jeu serveur
(`/played`) et le niveau dans chaque relevé A, et produit à la connexion un rapport « joué sans l'addon : X min » quand
la hausse du `/played` dépasse de plus de 10 minutes le temps où l'addon a vu le perso en ligne. **Cas à examiner,
jamais Tricheur automatique** (ni chez le joueur, ni chez l'officier qui le reçoit, ni sur le site). Tout est additif
et optionnel : un ancien bridge ignore les nouveaux champs, un ancien site les ignore (il ne les garde pas, sans
refuser l'envoi).

## SavedVariables (addon ≥ 0.22.11 → bridge)

- Relevé A, `GuildSelfForeverAuditBaseline.snapshot` (fichier du perso) et
  `GuildFoundForeverDB.guilds[*].auditBaselineBackups[*].snapshot` (fichier compte), champs facultatifs :
  - `played` (entier, 0 à 2147483647) : dernier `/played` total connu du serveur, en secondes ;
  - `playedObserved` (entier, 0 à 315360000) : secondes où l'addon a vu le perso en ligne depuis cette valeur ;
    temps de jeu attendu au moment du relevé = `played + playedObserved` ;
  - `level` (entier, 1 à 255) : niveau du perso au moment du relevé.
  `played` et `playedObserved` vont **toujours ensemble** (l'un sans l'autre = ignorés tous les deux). Ils
  n'entrent **pas** dans `checksum` (`snapshotFingerprint` inchangé) : un ancien bridge ou un ancien addon qui ne les
  transporte pas garde des empreintes valides.
- Rapport d'audit, `auditReports[i]` (officier) et `GuildSelfForeverAuditPending.audits[i]` (joueur), champs
  facultatifs, présents seulement dans un rapport « joué sans l'addon » :
  - `playedBefore` (entier) : temps de jeu attendu d'après l'addon ; `playedAfter` (entier, > `playedBefore`) :
    `/played` donné par le serveur à la connexion ; écart = `playedAfter − playedBefore` (> 600 s) ;
  - `levelBefore`, `levelAfter` (entiers 1 à 255, facultatifs) : niveau du relevé de référence et niveau actuel.
  Un tel rapport n'a ni objet, ni banque, ni équipement, ni variation d'or : `changeCount = 1`,
  `missingBaseline` absent ; `from` = heure de la référence, `to` = heure du contrôle.

## Forme sérialisée (`SerializeAudit` : `AUD`, `AUDO`, `XRLY`/`XRLF`, `bridgeCarriedAudits[*].payload`, `#audit`)

Dernier jeton facultatif du corps : `P,<playedBefore>,<playedAfter>,<levelBefore>,<levelAfter>` (niveaux 0 = inconnu),
un seul, `playedAfter > playedBefore`, valeurs 0 à 2147483647, niveaux 0 à 255 ; sinon tout le rapport est rejeté
(jamais de rapport partiel). Exemple : `1800000000,1800090000,562,562,,0|P,51210,53910,20,22`.
Les addons ≤ 0.22.10 et `App\Domain\Bridge\SerializedAudit` actuel rejettent ce jeton : ils n'accusent pas réception
et ne classent personne. Tous les autres rapports gardent exactement l'ancien format.

## Bridge (0.7.3, `gsf-core` `model`)

- `AuditReport` : `played_before`, `played_after`, `level_before`, `level_after` (`Option<i64>`, omis du JSON
  du snapshot si absents : un ancien rapport garde exactement le même JSON), lus depuis `playedBefore`,
  `playedAfter`, `levelBefore`, `levelAfter` (entiers ; `played_*` de 0 à 2147483647, niveaux de 1 à 255, sinon
  `None` ; `played_before`/`played_after` gardés seulement ensemble et si `played_after > played_before`, comme le
  jeton `P`). Même lecture pour `audit_reports` et `audit_pending` ; envoyés aussi par un membre simple (preuves de
  triche).
- Audits portés (`bridge_carried_audits[*].payload`) : chaîne opaque, le jeton `P` voyage tel quel.
- Photo multi-PC (`model::baseline`, ajout 2026-11-03) : `snapshot.played`, `snapshot.played_observed`,
  `snapshot.level` facultatifs (mêmes bornes ; `played`/`played_observed` gardés seulement ensemble et si leur
  somme ne dépasse pas 2147483647, comme `snapshotPlayed` de l'addon), hors `checksum` (le bridge ne touche pas à
  l'empreinte) ; un champ invalide (hors bornes ou pas un entier) est écarté seul, la photo reste (à la lecture comme
  à la réception du site) ; réécrits dans `GSF_PushedData.auditBaselines[i].snapshot` en `played`, `playedObserved`, `level`.
  Sans eux, la photo de l'autre PC reste utilisée pour les sacs, mais l'addon n'a pas de `/played` de référence
  (pas de signal).
- Pas encore fait (facultatif, pour la liste des inactifs) : relevé du perso, `level` et `played` du dernier A de
  chaque fichier de perso, avec `savedAt`.

## Site (à faire)

- Photos multi-PC (`App\Support\Bridge\AuditBaselines::snapshot`) : garder `played` (0 à 2147483647) et
  `played_observed` (0 à 315360000) seulement ensemble et si leur somme ≤ 2147483647, et `level` (1 à 255), un champ
  invalide écarté seul ; aujourd'hui ils sont jetés à la réécriture, donc la photo de l'autre PC arrive sans
  `/played` de référence.
- `audit_reports` : colonnes `played_before`, `played_after`, `level_before`, `level_after` (nullables).
  `SerializedAudit::decode` accepte le jeton `P` (mêmes contrôles que l'addon).
- Tribunal : règle « Joué sans l'addon » (écart `played_after − played_before`, seuil réglable, 10 min par défaut),
  **à examiner**, jamais Tricheur automatique ; les règles existantes (objets, or, équipement) ne voient rien de
  nouveau dans ce rapport (aucun changement d'objet). Phrase dans « Son histoire » : « Joué 45 min sans l'addon
  (niveau 20 → 22) ».
- Limites connues à afficher : un plantage du jeu (sauvegarde non écrite) ou un 2e PC sans la case « Synchroniser »
  du bridge peuvent produire un écart ; le site peut le recouper avec `addon_seen_at` et les rapports de l'autre PC.

## Addon (0.22.11, `Audit.lua`)

- `RequestTimePlayed()` 5 s après le début du suivi (3 essais, 10 s d'attente) ; message de chat masqué pour cette
  demande seulement (fenêtres de discussion désabonnées de `TIME_PLAYED_MSG`, réabonnées après la réponse, au bout de
  10 s, ou tout de suite si le joueur tape `/played`). Tout `TIME_PLAYED_MSG` est une lecture valable.
- Temps vu par l'addon mesuré avec `GetTime` (horloge de la session), jamais l'horloge système ou serveur.
- Référence : le A choisi à la connexion (copie dédiée, secours du compte ou photo de l'autre PC), ses propres
  `played`/`playedObserved` ; un A périmé (`/reload` avec ancienne copie, sauvegarde revenue en arrière) n'est pas
  utilisé pour le `/played`. Marque de déconnexion `GSFAuditPlayedMark` (CVar : `<clé perso>:<attendu>:<heure>:<niveau>`) :
  la valeur attendue la plus haute l'emporte ; une marque plus ancienne que le A choisi est ignorée, la marque est
  effacée quand rien n'est connu et à une nouvelle inscription. Chaque session portée sans lecture serveur ajoute 60 s
  de crédit (temps avant le chargement de l'addon, écrans de chargement).
- Pas de signal : référence sans `/played` (premier passage après la mise à jour), pas de `TIME_PLAYED_MSG`,
  nouvelle inscription, `/played` plus bas qu'attendu (noté « incohérent » dans `/gsf auditstatus`).

---

# Ajout 2026-11-04 (b) : retours d'inactifs (site, bridge 0.7.4, addon 0.22.12)

Un ancien membre exclu pour inactivité (ou parti pendant une absence déclarée) revient en chuchotant « gsf » à un
recruteur, **sans** la règle du niveau 1, et arrive avec le statut « Retour à vérifier » jusqu'au feu vert d'un
officier sur le site. Jamais membre automatiquement ; un signal est un cas à examiner, jamais Tricheur automatique ;
un banni (ban de perso, ban de la personne, liste noire) n'est jamais réinvité. Tout est additif : un ancien bridge
ignore la clé, un ancien addon aussi (il garde la règle du niveau 1), `schema` inchangé.

## Données poussées : `returns` (collecteurs seulement)

Présente pour tout bridge collecteur, jamais pour un membre simple ; incluse dans le hash `version`.

```json
"returns": {
  "allowed": [
    { "name": "Nom Prénom", "bt": "<32 hex>", "acct": "<16 hex>", "reason": "inactivity", "since": 1800000000 }
  ],
  "validated": [ { "name": "Nom Prénom", "at": 1800100000 } ]
}
```

- `allowed` (500 au plus) : persos de la guilde demandée qui peuvent revenir par « gsf » : hors guilde (ni retraité,
  ni signalé), exclus au motif `inactivity` ou partis pendant une absence déclarée (`absence`), sans ban actif ni
  identité bannie ni entrée de liste noire, avec une identité connue (`bt` et/ou `acct`, chacun omis si inconnu).
  `since` = heure du départ.
- `validated` (500 au plus) : retours validés par un officier (ou cas « à examiner » classé sans suite) dans les
  30 derniers jours ; `at` = heure de la décision. L'addon d'un officier repasse alors « Retour à vérifier » à
  inscrit si `flaggedAt <= at`.

## Bridge (0.7.4) : `returns`

Chaque entrée validée seule, jamais tout le fichier : nom (règles des bans), `bt` gardé seulement s'il fait exactement
32 caractères `[0-9a-f]`, `acct` 16, entrée `allowed` écartée s'il ne reste aucun des deux ; `reason` gardée seulement
si `inactivity` ou `absence` ; `since` négatif omis ; entrée `validated` écartée si `at` est négatif ; 500 au plus par
liste. Clé écrite seulement si le site l'envoie :

```lua
returns = {
    allowed = { { name = "Nom Prénom", bt = "…", acct = "…", reason = "inactivity", since = 1800000000 }, … },
    validated = { { name = "Nom Prénom", at = 1800100000 }, … },
},
```

## Addon (0.22.12)

- Recruteur avec la liste : « gsf » chuchoté par un perso de `allowed` → mêmes refus qu'avant (retraité, banni,
  identité bannie, liste noire), pas de contrôle niveau 1 / 0 XP / 0 cuivre / sacs de départ, mais l'identité
  déclarée par l'addon du candidat (`INVID`) doit correspondre à l'entrée (`bt`, ou `acct` si l'entrée n'a pas de
  `bt`). Un autre perso de la même personne n'est pas accepté par ce chemin.
- Arrivée en `TO_VERIFY` avec `flagSource = "return"` (« Retour à vérifier » : échanges et courrier bloqués), envoyé
  au site par le roster d'aventure du snapshot, sans changement du modèle.
- L'audit du perso compare à sa dernière référence d'avant le départ ; le temps joué sans l'addon reste un cas à
  examiner.
- Limite : l'identité est déclarée par l'addon du candidat (falsifiable par un addon modifié qui connaîtrait le
  BattleTag de l'ancien joueur) ; le nom du chuchoteur est garanti par le serveur, il faut donc aussi tenir le perso.

---

# Ajout 2026-11-10 : témoins de présence, artisans, journal de banque, identité propre (bridge 0.8.0)

Phase 1 de la refonte anti-triche (plan hors dépôt) : la détection ne doit plus croire le client du joueur audité,
seulement un signal observé par d'autres ou par le serveur. Additif, `MODEL_VERSION` reste 1 : un ancien bridge
ignore les nouvelles clés, un ancien site aussi.

## SavedVariables (profil de guilde de l'addon, ≥ 0.23.0)

```lua
presenceGaps["<suspectKey>:<startedAt>"] = { suspect = "Key", by = "WitnessKey", s = <epoch>, e = <epoch> | nil,
  lc = <epoch>, m = <minutes>, why = "addon" | "offline" | "witness_offline" | "open", rank = 1 | 2,
  rem = <int>, enr = true | nil, ever = true | nil, ver = "0.22.50" | nil }
craftedSightings["<itemId>|<guid>|<holder>|<source>"] = { item = <int>, guid = "Player-6937-0049F204", realm = 6937,
  cname = "Nom" | nil, ckind = "unknown" | "allied",
  src = "guild_bank" | "bank_log" | "trade" | "mail" | "bags" | "inspect",
  holder = "Key" | "guildbank:3", by = "ObserverKey", first = <epoch>, last = <epoch>, n = <int> }
guildBankLog["<tab>:<readAt>:<i>"] = { tab = 1..8 | 0 (0 = or),
  k = "deposit" | "withdraw" | "move" | "repair" | "buyTab" | "withdrawForTab", who = "Name", item = <int> | nil,
  n = <int> | nil, amt = <cuivre> | nil, guid = "Player-…" | nil, at = <epoch approx>, age = <heures>,
  read = <epoch>, by = "ReaderKey" }
selfIdentity["<ownKey>"] = { bt = "<32 hex>", acct = "<16 hex>", btag = "Nom#1234", at = <epoch> }
bridgeRoster[key].guid = "Player-…"   -- facultatif
```

Plafonds côté addon : `presenceGaps` 300 / 14 j ; `craftedSightings` 300 / 30 j ; `guildBankLog` 500 / 21 j.

## `GuildData` (snapshot bridge → site)

```json
"presence_gaps": [{ "id": "kael:1794500000:torvi quatre", "suspect": "<CharacterRef>", "witness": "<CharacterRef>",
  "started_at": 1794500000, "ended_at": 1794501800, "last_check_at": 1794501800, "minutes": 30,
  "end_reason": "addon", "witness_rank": 1, "reminders": 1, "enrolled": true, "ever_seen": true,
  "last_version": "0.22.50" }],
"crafted_sightings": [{ "id": "…", "item_id": 2059, "crafter_guid": "Player-6937-0049F204", "crafter_realm_id": 6937,
  "crafter_name": null, "crafter_kind": "unknown", "source": "trade", "holder": "<CharacterRef>",
  "holder_kind": "character", "observer": "<CharacterRef>", "first_seen_at": 1794500000, "last_seen_at": 1794500000,
  "count": 1 }],
"guild_bank_log": [{ "id": "3:1794500000:7", "tab": 3, "kind": "deposit", "player": "<CharacterRef>",
  "player_name": "Kael", "item_id": 2059, "count": 5, "amount": null, "crafter_guid": null,
  "approx_at": 1794496400, "age_hours": 1, "read_at": 1794500000, "reader": "<CharacterRef>" }],
"self_identity": [{ "character": "<CharacterRef>", "bt": "<32 hex>", "acct": "<16 hex>", "btag": "Nom#1234",
  "at": 1794500000 }],
"bridge_roster": [{ "...existant...": "...", "guid": "Player-6937-0049F204" }]
```

- `presence_gaps`, `crafted_sightings`, `self_identity` (défaut liste vide) : n'importe quel bridge d'officier, de
  vigie ou de membre de confiance peut en porter ; ajoutés à `snapshot::MEMBER_GUILD_KEYS`, un membre simple les envoie
  donc aussi. `guild_bank_log` (`None` si aucun profil n'en a) reste réservé aux collecteurs, jamais envoyé par un
  membre simple.
- Identifiants : `guild_bank_log[].id` et `crafted_sightings[].id` reprennent la clé brute des SavedVariables
  (`"<tab>:<readAt>:<i>"`, `"<itemId>|<guid>|<holder>|<source>"`). `presence_gaps[].id` est recomposé
  `<suspect>:<started_at>:<witness>` (clés résolues) : la clé brute des SavedVariables (`<suspectKey>:<startedAt>`) ne
  distingue pas les deux témoins élus pour le même suspect et le même début.
- `crafted_sightings[].holder_kind` = `"character"` (perso résolu comme les autres références) ou `"guild_bank"`
  (`holder = "guildbank:<n>"`, onglet de banque de guilde, jamais résolu comme une identité de joueur).
- Validation champ par champ (valeur invalide = champ abandonné, jamais tout l'enregistrement, sauf le champ qui sert
  à construire l'identifiant — `s`/`item`/`tab` — où l'enregistrement entier est alors abandonné, fusion impossible
  sans lui) : GUID `^Player-\d{1,5}-[0-9A-F]{8}$` (`crafted_sightings[].crafter_guid`, `guild_bank_log[].crafter_guid`,
  `bridge_roster[].guid`) ; listes fermées `end_reason`, `crafter_kind`, `source`, `kind` (ci-dessus) ; entiers bornés
  (`witness_rank` 1 ou 2, `crafter_realm_id` 1–99999, `tab` 0–8…).
- Fusion entre profils par identifiant, le plus récent gagne **en entier** (jamais champ par champ) : `presence_gaps`
  par `ended_at`/`last_check_at` (le plus grand des deux présents), `crafted_sightings` par `last_seen_at`,
  `guild_bank_log` par `read_at`, `self_identity` par `at` (même règle que `member_identities`).
- `learn_names` (résolution des alias « prénom seul ») apprend aussi les noms complets trouvés dans `suspect`/`by`
  (`presence_gaps`), `holder`/`by` (`crafted_sightings`), `who`/`by` (`guild_bank_log`).

## Confidentialité : `member_identities[].btag` n'est plus transmis

Le bridge arrête de transmettre `memberIdentity[clé].btag` au site (champ gardé une version, toujours `None`
désormais) : un officier ne doit plus voir passer le BattleTag en clair d'un autre joueur via son propre bridge.
Remplacé par `self_identity` : chaque joueur envoie son propre BattleTag par son propre bridge seulement.

- Site : `self_identity[].btag` accepté seulement si `hash(btag) === bt` de la même entrée **et** si l'envoi vient du
  compte qui possède ce personnage (jamais celui rapporté par un tiers).
- `trust` (`officer` | `vigie` | `trusted_member` | `member`, déduit du jeton comme ailleurs dans ce contrat) enregistré
  sur chaque ligne ingérée de `presence_gaps`, `crafted_sightings`, `guild_bank_log`, `self_identity`.

## Ajout : statistiques du serveur (bridge 0.8.0)

Statistiques Forever relevées en jeu (GSF Probe, build 70862) : 329 Mises en vente HV, 330 Achats HV, 919 Argent
gagné HV, 331 Plus grosse enchère HV, 332 Plus grosse vente HV, 1148 Argent dépensé en courriers, 328 Argent total
obtenu, 333 Argent ramassé, 326 Argent de quêtes, 921 Argent des marchands, 98 Quêtes achevées, 334 Plus grande
quantité d'argent possédée. Valeurs lues par
`GetStatistic(id)` (soi) ou `GetComparisonStatistic(id)` (autre joueur, après `SetAchievementComparisonUnit(unit)` +
`INSPECT_ACHIEVEMENT_READY`, portée d'inspection) ; les montants arrivent en texte avec icônes, convertis en cuivre
côté addon ; « -- » = 0. Plus `played` (hors `GetStatistic`) : temps de jeu total du personnage en secondes,
entier ≥ 0, borné à 100 000 000 s (≈ 3 169 ans) au lieu de la borne large des autres compteurs. Additif,
`MODEL_VERSION` reste 1.

### SavedVariables (profil de guilde de l'addon, ≥ 0.23.0)

```lua
serverStats["<ownKey>"] = { at = <epoch>, lvl = <int>,
  stats = "329:0,330:0,919:0,331:0,332:0,1148:3090,328:428550,333:29709,326:112235,921:286606,98:272,334:950000,played:1036800" }
  -- relevé par le joueur lui-même à la connexion et toutes les 30 min ; pas de nouveau message, les valeurs
  -- voyagent dans le rapport d'audit existant (même relais vers les officiers/collecteurs) et chaque
  -- officier/collecteur qui le reçoit les range dans serverStats[<clé du joueur>] (dernière valeur par joueur,
  -- cap 150 joueurs, TTL 21 j côté addon ; contrat : maxima 800 / 30 j) ; le joueur garde aussi la sienne.
statChecks["<targetKey>:<at>"] = { target = "Key", by = "OfficerKey", at = <epoch>, how = "auto" | "manual",
  lvl = <int> | nil, stats = "...mêmes paires..." }
  -- comparaison faite par un officier sur un autre joueur (cap 60, TTL 21 j côté addon ; contrat : maxima
  -- 200 / 30 j)
```

`stats` : chaîne compacte `"<id>:<valeur>,...[,played:<secondes>]"` dans l'ordre des identifiants autorisés, `played` en dernier quand connu (addon ≥ 0.23.0, 2e revue : une table imbriquée coûtait trop cher en SavedVariables). Le bridge lit `stats` en priorité et retombe sur une table `s = { [id] = valeur, ... }` (forme antérieure) si `stats` est absent, pour ne jamais casser une fixture ou une ancienne version de l'addon.

### `GuildData` (snapshot bridge → site)

```json
"server_stats": [{ "character": "<CharacterRef>", "at": 1794500000, "level": 25,
  "stats": { "329": 0, "330": 0, "919": 0, "331": 0, "332": 0, "1148": 3090, "328": 428550, "333": 29709,
    "326": 112235, "921": 286606, "98": 272, "334": 950000, "played": 1036800 } }],
"stat_checks": [{ "id": "dorian:1794500000", "target": "<CharacterRef>", "officer": "<CharacterRef>",
  "at": 1794500000, "how": "auto", "level": 25, "stats": { "...mêmes clés..." } }]
```

- `stats` : uniquement les 13 identifiants ci-dessus (chaînes), valeurs entières ≥ 0 (cuivre pour les montants,
  secondes pour `played`, borné à 100 000 000) ; identifiant hors liste, paire malformée (pas de `:`) ou valeur
  négative/hors borne = paire abandonnée seule, jamais tout l'enregistrement.
- `server_stats` (défaut liste vide) : depuis un bridge officier/collecteur pour n'importe quel membre (relayé par
  l'audit, donc déclaré par le joueur), depuis un bridge de simple membre seulement pour ses propres personnages
  (ajouté à `snapshot::MEMBER_GUILD_KEYS`, le site filtre aux personnages du compte qui envoie). Fusionné entre
  profils par personnage, le plus récent `at` gagne, **en entier** (jamais champ par champ).
- `stat_checks` : seulement depuis un bridge officier/collecteur ; un simple membre ne l'envoie pas
  (`snapshot::MEMBER_GUILD_KEYS` inchangé pour cette clé). `id` reprend la clé brute des SavedVariables
  (`"<targetKey>:<at>"`) ; `target` recalé sur la clé si absent du relevé (comme `presenceGaps`/`suspect`).
  Fusionné entre profils par identifiant, **en entier**.
- `learn_names` (résolution des alias « prénom seul ») apprend aussi les noms complets trouvés dans `target`/`by`
  (`statChecks`).

Site : un compteur HV (329, 330 ou 919) > 0 vu par un officier (`stat_checks`) = cas « Hôtel des ventes (serveur) »,
preuve serveur ; s'il était déjà > 0 au premier relevé connu, le cas le précise (« utilisé avant l'entrée ? ») ; une
valeur envoyée par le joueur (`server_stats`) inférieure à celle vue par un officier au même moment ou après
(compteurs croissants) = cas « Statistiques falsifiées » (gravité haute).

---

# Ajout 2026-11-10 (c) : audits soldés (§3, `PushedData`, additif, bridge 0.8.1)

Réponse `GET /api/v1/bridge/pushed-data` : clé `audits_settled` **optionnelle**, réservée aux collecteurs
(officier ou vigie), en snake_case comme `cleared`/`allied_guilds`.

```json
"audits_settled": [
  { "k": "annbee", "id": "report-42" }
]
```

- `k` : clé joueur, nom en jeu minuscule (clé `WowName` du site). `id` : identifiant du rapport d'audit soldé.
  L'addon s'en sert pour retirer ce rapport du badge de ce joueur.
- Mêmes règles que `cleared` pour la validation d'un champ texte (non vide, ≤ 64 octets, sans caractère de
  contrôle ni `|`) ; une entrée invalide (`k` ou `id`) est écartée seule. Jusqu'à 3000 entrées servies par le
  site, `MAX_AUDITS_SETTLED = 5000` côté bridge (défensif).
- Fait partie du hash `version` côté site (comme `cleared`).

Le bridge écrit `GSF_PushedData = { …, audits_settled = { { k = "...", id = "..." }, … } }` : **contrairement à
`cleared`** (toujours présente, même vide), `audits_settled` est omise si absente du contrat ou si plus rien n'est
valide, pour ne pas faire réécrire le fichier sans raison.

```lua
GSF_PushedData = {
    schema = 1,
    guildKey = "realm#home",
    bans = {},
    alliedGuilds = {},
    cleared = {},
    audits_settled = {
        [1] = { k = "annbee", id = "report-42" },
    },
}
```
