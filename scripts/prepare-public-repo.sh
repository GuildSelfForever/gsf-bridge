#!/usr/bin/env bash
# Prépare, EN LOCAL SEULEMENT, le futur dépôt public de GSF Bridge avec un historique
# propre : un seul commit de départ, sans l'historique privé (noms et e-mails des
# auteurs, anciennes fixtures). Ne pousse rien, ne crée aucun dépôt GitHub, ne touche
# à aucun secret. Les étapes en ligne sont dans PUBLISHING.md.
#
# Usage :
#   AUTHOR_NAME="Guild Self Forever" AUTHOR_EMAIL="<id>+<login>@users.noreply.github.com" \
#     scripts/prepare-public-repo.sh [ref] [dossier-cible]
#
#   ref            commit ou branche à publier (défaut : main)
#   dossier-cible  dossier à créer (défaut : ../gsf-bridge-public), qui ne doit pas exister
#
# Variables facultatives :
#   BLOCKLIST_FILE  fichier HORS dépôt, une expression par ligne (vrais pseudos, noms, e-mails,
#                   adresse IP du serveur…), qui ne doivent apparaître nulle part dans les fichiers
#                   publiés. Recherche sans casse ; le script s'arrête au moindre résultat.
#   GITLEAKS        chemin de gitleaks (défaut : gitleaks dans le PATH).
set -euo pipefail

ref="${1:-main}"
here="$(cd "$(dirname "$0")/.." && pwd)"
target="${2:-$here/../gsf-bridge-public}"

: "${AUTHOR_NAME:?AUTHOR_NAME requis (nom affiché du commit de départ)}"
: "${AUTHOR_EMAIL:?AUTHOR_EMAIL requis (de préférence une adresse noreply GitHub)}"
gitleaks="${GITLEAKS:-gitleaks}"

if [ -e "$target" ]; then
  echo "Le dossier $target existe déjà : choisis-en un autre ou supprime-le." >&2
  exit 1
fi
if ! command -v "$gitleaks" > /dev/null; then
  echo "gitleaks introuvable (https://github.com/gitleaks/gitleaks/releases) ; ou GITLEAKS=/chemin/gitleaks" >&2
  exit 1
fi

commit="$(git -C "$here" rev-parse --verify "$ref^{commit}")"
version="$(sed -n '/^\[workspace\.package\]/,/^\[/{s/^version *= *"\([^"]*\)".*/\1/p}' "$here/Cargo.toml" | head -1)"
echo "Publication de $ref ($commit), version $version, vers $target"

# 1. Copie des seuls fichiers suivis du commit (rien de non suivi, pas de target/, pas de .git).
mkdir -p "$target"
git -C "$here" archive --format=tar "$commit" | tar -x -C "$target"

# Ce script et PUBLISHING.md restent dans le dépôt public : ils documentent comment il a été créé.

# 2. Recherche de secrets dans les fichiers publiés.
"$gitleaks" dir "$target" --config "$target/.gitleaks.toml" --redact --no-banner

# 3. Recherche des noms réels (liste gardée hors du dépôt).
if [ -n "${BLOCKLIST_FILE:-}" ]; then
  if grep -rniI -f <(grep -v '^[[:space:]]*$' "$BLOCKLIST_FILE") "$target"; then
    echo "Des expressions de $BLOCKLIST_FILE apparaissent ci-dessus : à retirer avant publication." >&2
    exit 1
  fi
  echo "Liste de blocage : aucun résultat."
else
  echo "Attention : BLOCKLIST_FILE non fourni, recherche des noms réels non faite." >&2
fi

# 4. Nouveau dépôt, un seul commit, auteur choisi (aucune trace de l'historique privé).
git -C "$target" init -q -b main
git -C "$target" add -A
GIT_AUTHOR_NAME="$AUTHOR_NAME" GIT_AUTHOR_EMAIL="$AUTHOR_EMAIL" \
GIT_COMMITTER_NAME="$AUTHOR_NAME" GIT_COMMITTER_EMAIL="$AUTHOR_EMAIL" \
  git -C "$target" -c commit.gpgsign=false commit -q -m "chore: initial public release of GSF Bridge $version" \
  -m "Open-source release under the MIT License. Earlier history stays in the private repository."
# Tag de la version courante : le workflow Release du nouveau dépôt ne la republie pas au premier push.
GIT_COMMITTER_NAME="$AUTHOR_NAME" GIT_COMMITTER_EMAIL="$AUTHOR_EMAIL" \
  git -C "$target" -c tag.gpgsign=false tag -a "v$version" -m "GSF Bridge $version"

echo
git -C "$target" log --format='%h %an <%ae> %s' --all
echo
echo "Prêt dans $target (rien n'a été poussé). Suite : PUBLISHING.md, étape 3."
