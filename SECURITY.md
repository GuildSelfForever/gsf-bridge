# Sécurité

> **English:** please report vulnerabilities privately through GitHub (*Security* tab, *Report a vulnerability*),
> never in a public issue. We answer in French or English.

## Signaler une faille

Merci de **ne pas ouvrir d'issue publique** pour une faille de sécurité.

1. Sur GitHub, onglet **Security**, bouton **Report a vulnerability** (signalement privé, visible seulement des
   mainteneurs).
2. Si ce n'est pas possible : message privé à un administrateur sur le Discord de la guilde Guild Self Forever, en
   demandant un canal privé, sans détailler la faille dans un salon public.

Indique la version du bridge (« Réglages »), ton système, ce qu'un attaquant peut obtenir, et comment le
reproduire. Nous accusons réception sous 7 jours et te tenons au courant jusqu'au correctif. Une faille corrigée
est publiée dans une nouvelle version, mise à jour automatiquement sur tous les bridges.

## Ce qui nous intéresse en priorité

- Tout moyen de faire installer au bridge une mise à jour qui n'est pas signée par la clé de la guilde.
- Tout moyen de lire ou d'écrire, via le bridge, autre chose que ce que liste le README (section « Ce que fait le
  bridge sur ton PC »), ou d'envoyer des données ailleurs qu'au site de la guilde.
- Le vol du jeton de connexion, ou l'interception du code de connexion reçu sur `127.0.0.1`.
- Un fichier de sauvegarde de l'addon ou une réponse du site qui ferait planter le bridge, exécuter du code, ou
  produire un `Data.lua` que l'addon interpréterait autrement que prévu.

Les failles du site lui-même (gsforever.com) ou de l'addon sont aussi les bienvenues par le même canal.

## Versions suivies

Seule la dernière version publiée reçoit des correctifs : le bridge se met à jour tout seul.
