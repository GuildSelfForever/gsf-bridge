# Passer le bridge en dépôt public

Procédure suivie (ou à suivre) pour publier le code du bridge avec un historique propre : un nouveau dépôt
`GuildSelfForever/gsf-bridge` public qui part d'un seul commit, l'ancien dépôt privé étant renommé puis archivé.
Pourquoi un nouveau dépôt plutôt que rendre l'ancien public : l'ancien historique contient les noms et e-mails
personnels des auteurs et des fixtures de vrais personnages, et ses issues, pull requests et journaux d'Actions
deviendraient visibles.

Rien ici n'est automatique : chaque étape en ligne se lance à la main, dans l'ordre.

## 1. Avant de commencer

- Accord des titulaires des droits sur la licence MIT (donné le 2026-10-31).
- La branche à publier est fusionnée sur `main` du dépôt privé, CI verte.
- La clé privée de signature des mises à jour est à portée de main (`~/.tauri/gsf-bridge.key` et son mot de
  passe) et **sauvegardée hors du poste**. Le nouveau dépôt garde la même clé publique : les bridges installés
  continueront d'accepter les mises à jour.
- Une liste des noms réels à ne jamais publier (pseudos et vrais noms des membres, e-mails, adresse IP du
  serveur…), une expression par ligne, dans un fichier **hors du dépôt**, par exemple `~/gsf/secrets/blocklist.txt`.

## 2. Préparer le dépôt en local

```sh
AUTHOR_NAME="Guild Self Forever" \
AUTHOR_EMAIL="<id>+<compte>@users.noreply.github.com" \
BLOCKLIST_FILE=~/gsf/secrets/blocklist.txt \
  scripts/prepare-public-repo.sh main ../gsf-bridge-public
```

Le script copie les fichiers suivis de `main`, lance gitleaks et la liste de blocage, crée un dépôt avec un seul
commit (auteur choisi) et le tag de la version courante. Il ne pousse rien. Relire ensuite le résultat :
`git -C ../gsf-bridge-public show --stat`.

## 3. Libérer le nom et créer le dépôt public

```sh
# L'ancien dépôt devient gsf-bridge-private (il reste privé ; ses releases et secrets restent dessus).
gh repo rename gsf-bridge-private --repo GuildSelfForever/gsf-bridge

# Nouveau dépôt public, vide.
gh repo create GuildSelfForever/gsf-bridge --public \
  --description "GSF Bridge : application de bureau qui relie l'addon GuildSelfForever au site de la guilde"
```

## 4. Régler le dépôt avant le premier push

Dans l'interface GitHub du nouveau dépôt (Settings) :

1. **Environments** : créer `release`. Deployment branches : seulement `main`. Required reviewers : au moins un
   mainteneur (chaque publication attend alors un clic « Approve »).
2. Secrets de l'environnement `release` :
   ```sh
   gh secret set TAURI_SIGNING_PRIVATE_KEY --env release --repo GuildSelfForever/gsf-bridge < ~/.tauri/gsf-bridge.key
   gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --env release --repo GuildSelfForever/gsf-bridge < ~/.tauri/gsf-bridge.password
   gh secret set DEPLOY_HOST --env release --repo GuildSelfForever/gsf-bridge      # puis DEPLOY_USER, DEPLOY_SSH_KEY,
   # DEPLOY_PORT et DEPLOY_KNOWN_HOSTS : mêmes valeurs que sur l'ancien dépôt (les secrets ne se relisent pas :
   # les reprendre depuis leur source, jamais depuis un fichier du dépôt).
   ```
3. **Branches** : règle de protection de `main` (pull request obligatoire, CI `linux-core`, `secrets-scan` et
   `windows-build` vertes, pas de force-push ni de suppression).
4. **Code security** : activer « Private vulnerability reporting » (utilisé par `SECURITY.md`), Dependabot alerts
   et Secret scanning (gratuits sur un dépôt public).
5. **Actions, General** : « Workflow permissions » en lecture seule par défaut (les workflows demandent
   eux-mêmes ce qu'il leur faut).

## 5. Pousser

```sh
cd ../gsf-bridge-public
git remote add origin https://github.com/GuildSelfForever/gsf-bridge.git
git push origin main "v$(sed -n '/^\[workspace\.package\]/,/^\[/{s/^version *= *"\([^"]*\)".*/\1/p}' Cargo.toml | head -1)"
```

Le tag poussé en même temps empêche le workflow Release de republier la version courante. Vérifier que la CI
passe, puis lancer à la main le workflow Release (essai à blanc coché) pour vérifier builds, signature,
`SHA256SUMS` et attestation sans rien publier.

## 6. Après

- Archiver l'ancien dépôt : `gh repo archive GuildSelfForever/gsf-bridge-private`.
- Les clones locaux pointent encore vers l'ancien dépôt : `git remote set-url origin
  https://github.com/GuildSelfForever/gsf-bridge.git` puis repartir de `main` du nouveau dépôt (historique
  différent : pas de fusion avec l'ancien).
- La prochaine version publiée (version montée dans `Cargo.toml`) part du nouveau dépôt, avec `SHA256SUMS` et
  attestation de build.
- Le lien « Voir le code source » de l'application (`REPO_URL`, `src-tauri/src/commands.rs`) vise déjà
  `https://github.com/GuildSelfForever/gsf-bridge`.
