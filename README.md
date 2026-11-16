# LoL Companion (prototype perso)

Petite app desktop Tauri 2 (cœur Rust + interface HTML/JS) qui se connecte en local
au client League of Legends et à la partie en cours. Elle importe automatiquement
les runes, le build, les sorts et un set d'objets recommandés par OP.GG, aide à choisir
son champion, montre les rangs en partie et garde l'historique de tes parties.
Aucune clé API Riot n'est nécessaire.

## Installer (avec mises à jour automatiques)

L'installateur Windows est fabriqué par GitHub à chaque nouvelle version ; l'app
installée vérifie au démarrage s'il y en a une plus récente et propose de l'installer.
Mise en place, une seule fois :

1. **Dépôt GitHub public** : crée un dépôt vide `lol-companion`, puis depuis le dossier du projet :
   ```powershell
   git init
   git add .
   git commit -m "LoL Companion 0.8.0"
   git branch -M main
   git remote add origin https://github.com/TON_PSEUDO/lol-companion.git
   git push -u origin main
   ```
2. **Clés de signature** des mises à jour (l'app refuse toute mise à jour non signée par toi) :
   ```powershell
   cargo install tauri-cli --version "^2.0.0" --locked
   cargo tauri signer generate -w "$HOME\.tauri\lol-companion.key"
   ```
   Choisis un mot de passe. Ça crée `lol-companion.key` (clé privée : à garder secrète
   et à sauvegarder, sans elle plus aucune mise à jour possible) et `lol-companion.key.pub`.
3. **Dans `src-tauri/tauri.conf.json`** (section `plugins.updater`) : colle le contenu de
   `lol-companion.key.pub` à la place de `COLLE_ICI_TA_CLE_PUBLIQUE`, et remplace `TON_PSEUDO`
   par ton pseudo GitHub. Commit et push.
4. **Secrets GitHub** (dépôt > Settings > Secrets and variables > Actions > New repository secret) :
   - `TAURI_SIGNING_PRIVATE_KEY` : le contenu de `lol-companion.key` ;
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` : ton mot de passe.
5. **Publier** : le numéro de version de `tauri.conf.json` (et de `Cargo.toml`) doit être celui du tag.
   ```powershell
   git tag v0.8.0
   git push origin v0.8.0
   ```
   L'onglet Actions du dépôt montre la construction (10 à 15 minutes). Ensuite, la page
   Releases contient `LoL Companion_0.8.0_x64-setup.exe` et `latest.json`.
   Si l'étape échoue avec « Resource not accessible by integration » : Settings > Actions >
   General > Workflow permissions > « Read and write permissions ».
6. **Installer** : télécharge le `…-setup.exe` et lance-le. Il n'est pas signé avec un
   certificat payant : Windows SmartScreen affiche « Windows a protégé votre ordinateur »,
   clique sur « Informations complémentaires » puis « Exécuter quand même ».

Versions suivantes : change la version dans `tauri.conf.json` et `Cargo.toml`, commit, push,
puis un nouveau tag (`v0.8.1`...). L'app installée affiche un bandeau « Nouvelle version
disponible » au démarrage suivant ; le bouton installe et relance l'app.

Installateur fabriqué sur ton PC plutôt que sur GitHub (dans `src-tauri\target\release\bundle\nsis\`) :

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content "$HOME\.tauri\lol-companion.key" -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "ton mot de passe"
cd src-tauri
cargo tauri build
```

## Lancer en développement (Windows)

Prérequis, une seule fois :
1. Microsoft C++ Build Tools, avec « Développement Desktop en C++ » coché.
2. WebView2 (déjà présent sur Windows 10/11 à jour).
3. Rust : `winget install --id Rustlang.Rustup` (puis rouvrir le terminal).

Puis :

```powershell
cd lol-companion\src-tauri
cargo run
```

La première compilation prend quelques minutes, les suivantes quelques secondes.
En développement, l'app ne cherche pas de mise à jour. Évite d'y cocher « Lancer avec
Windows » : c'est l'exe de développement qui serait lancé au démarrage.

## Réglages (roue ⚙ en haut à droite)

- Import automatique en sélection : page de runes, set d'objets, sorts d'invocateur
  (chacun peut être coupé). Les recommandations restent affichées.
- Saut éclair sur D ou F, ou « sa touche actuelle » (sinon F).
- Sets d'objets : un seul, remplacé à chaque partie, ou un par champion.
- En partie, quand tu tiens Tab : le panneau des objets à acheter sur le bord gauche du jeu, et les
  écarts d'or à côté du tableau des scores (chacun peut être coupé).
- Lancer avec Windows (l'app démarre cachée près de l'horloge), afficher la fenêtre
  quand le client LoL s'ouvre, notification quand une partie est trouvée, et ce que fait la croix.

Un réglage changé en pleine sélection s'applique au prochain import (nouveau champion ou adversaire).

## Sélection des champions

En haut, ton champion avec le skin choisi (pour un chroma, l'illustration de son skin) face à
ton adversaire direct, puis le duel selon OP.GG : % de victoires de chaque côté, avantage, conseil.

Les deux équipes s'affichent en colonnes : la tienne à gauche, dans l'ordre des postes
(un champion seulement survolé est atténué, le tien est en bleu), les adversaires à droite.

**Avant de verrouiller : aide au choix.** Avec les données OP.GG :
- « Contre X » : les 10 champions qui gagnent le plus contre ton adversaire direct, à ton poste,
  avec leur % de victoires contre lui ; les 3 premiers sur un podium (or, argent, bronze). La liste
  vient de tous les duels de ton adversaire sur OP.GG (ceux joués dans au moins 0,5 % de ses parties) ;
  en attendant, ou si OP.GG ne la donne pas, les 3 contres de son analyse ;
- « Avec Y » : les 3 champions qui vont le mieux avec ton partenaire (ton jungler si tu es
  haut ou milieu, ton milieu si tu es jungler, l'autre joueur du bas pour le duo).

Les champions déjà pris ou bannis sont retirés de ces listes.

**Dès que ton champion est verrouillé**, l'app demande à OP.GG les runes et le build les plus
joués pour ce champion et ton poste, puis :
1. écrit les runes dans ta page de runes sélectionnée, qui prend le nom « LoL Companion » ;
2. met les sorts d'invocateur conseillés, le Saut éclair sur ta touche ;
3. affiche le build : objets de départ, 3 objets principaux, bottes, choix suivants,
   ordre des compétences ;
4. ajoute à la boutique un set d'objets « LoL Companion · <champion> ».

- Runes : pas besoin de page libre, ce que contenait ta page sélectionnée est remplacé. Le client ne
  laisse pas modifier une page : l'app la supprime et la recrée à la même place ; si la recréation
  échoue, ta page est remise telle qu'elle était. Si la page sélectionnée est une page par défaut de Riot
  (qui ne se supprime pas), l'app remplace sa page « LoL Companion », ou en crée une s'il reste de la
  place. Si tu changes de page ou de runes après l'import, l'app n'y touche plus jusqu'à la fin de la
  sélection.
- Sorts : si tu les changes toi-même pendant la sélection, l'app n'y touche plus. Avec « sa touche
  actuelle », le Saut éclair reste là où tu l'avais mis ; si le client donne des sorts illisibles au
  moment de l'import, l'app prend ceux qu'elle a lus juste avant, et n'y touche pas si elle n'en a aucun.
  Le terminal note chaque changement (« [sorts] avant D = 4, F = 14 ; après … »).
- Set d'objets : le client n'accepte que la liste complète de tes sets. L'app la relit
  juste avant d'écrire, garde tes autres sets tels quels et n'écrit rien si la liste
  est illisible. Elle vérifie ensuite que tes sets sont toujours là.
- Support : OP.GG ne compte pas l'Atlas dans les objets de départ, l'app le remet en tête.

**Duel contre ton adversaire direct** (parties avec postes : classées, normales avec choix des postes) :
- L'app devine ton adversaire : le champion adverse qui joue le plus souvent à ton poste
  selon OP.GG (au moins 15 % de ses parties). Clique sur un adversaire pour choisir toi-même ;
  reclique dessus pour revenir à la devinette.
- Runes, build et set d'objets passent alors à ceux du duel (« vs Sett »), avec tes stats contre lui
  et le conseil d'OP.GG (en anglais, OP.GG ne le traduit pas). Moins de 50 parties pour la page de
  runes du duel : les runes générales restent.
- Objets du duel (départ, objets principaux, bottes) : le meilleur % de victoires parmi les choix
  joués au moins 100 fois (en dessous, un % élevé peut être du hasard), sinon le plus joué.
- Si l'app a déjà enregistré des parties de ce duel, ton bilan s'affiche (« 2 V – 1 D »).

Stats OP.GG tous rangs par défaut : pour changer, modifie `TIER` dans `src-tauri/src/opgg.rs`
(par exemple `Some("emerald_plus")`). Les données OP.GG sont réservées à un usage personnel.

## En partie

Dès l'écran de chargement : les 10 champions avec le rang de chaque joueur (dans la file de la
partie : flexible pour une flexible, solo/duo sinon). Les joueurs en mode streamer restent
anonymes : le client ne donne pas leur compte, l'app n'a donc pas leur rang.

Une fois la partie lancée :
- deux bandes : les éliminations de chaque équipe et le temps de jeu, puis l'or en objets de chaque
  équipe (valeur de ses objets au prix de la boutique, l'or en poche ne compte pas) avec l'écart en
  grand au milieu, sur une case bleue si ton équipe est devant, orange si c'est l'équipe d'en face
  (grise à moins de 100 pièces d'écart) ;
- ta carte : ton skin, KDA, CS, or en poche ;
- le face-à-face, couloir par couloir : niveau, KDA et rang de chacun, et au milieu l'écart d'or
  du couloir, sur une case aux mêmes couleurs. Sans postes (ARAM...), les deux équipes côte à côte.
  « Rang masqué » : joueur en mode streamer.

En tenant Tab (avec le tableau des scores), deux choses s'affichent par-dessus le jeu.

**Panneau des objets à acheter**, collé au bord gauche de l'écran, à mi-hauteur : juste les icônes,
une rangée numérotée par achat, de haut en bas : départ, 1er objet (avec un autre choix, plus petit),
bottes, 2e objet (avec un autre choix). Le 1 est toujours le prochain achat. Ce sont les objets du duel
contre ton vrai adversaire de couloir, choisis comme en sélection ; trop peu de parties sur ce duel :
ceux de ton champion à ton poste.
- Un objet acheté et toujours là 30 secondes plus tard quitte le panneau pour de bon, même revendu
  ensuite (un achat annulé ou revendu dans les 30 secondes ne compte pas). Le 1er et le 2e objet
  partent aussi si tu as pris leur autre choix ; la rangée du départ part dès ton premier achat,
  même si c'est un autre départ. Les rangées restantes sont renumérotées ; quand tout est acheté,
  il n'y a plus de panneau.

**Écarts d'or**, en colonne juste à gauche du tableau des scores : celui de ton équipe à hauteur de
l'en-tête, avec l'icône de l'or, puis celui de chaque couloir à hauteur de sa ligne (bleu : ton joueur
est devant, rouge : celui d'en face ; « −1,2k » = 1 200 pièces de retard). Sans postes (ARAM...),
seulement celui de l'équipe. L'icône de l'or est celle du client (tableau des scores de l'historique) ;
s'il ne la donne pas, elle est prise sur CommunityDragon, sinon une pièce dessinée la remplace.
- Les positions sont celles du tableau des scores en 1920 × 1080 (relevées sur une capture), mises à
  l'échelle de la hauteur de l'écran. Si tu changes l'échelle de l'interface du jeu ou si tu déplaces le
  tableau, elles peuvent ne plus tomber en face des lignes.
- L'app lit seulement si Tab est enfoncée (sans Alt, pour ne pas réagir à Alt+Tab) : la touche arrive
  au jeu comme d'habitude. Si tu as changé la touche du tableau des scores, ça ne suit pas.

Pour les deux :
- Ils ne s'affichent que jeu au premier plan (pas après un Alt+Tab ou un clic sur un autre écran).
- Les clics les traversent et ils ne prennent jamais le clavier.
- Il faut LoL en mode d'affichage « Sans bordure » : en plein écran, le jeu passe par-dessus tout.
- Ils peuvent être coupés dans les réglages (roue ⚙, « En partie »).

CS : pendant la partie, l'API de Riot ne les donne que par paliers de 10 (0, 10, 20…).
L'app affiche une fourchette (« 80–89 ») et un rythme par minute estimé ; le compte exact
arrive après la partie.

Taille de la fenêtre : elle s'ouvre toujours à sa taille standard, 1280 × 720 comme le client LoL,
centrée (réduite sur un petit écran). Chaque écran (salon, sélection, partie) y est un tableau de bord :
des blocs encadrés, en colonnes alignées en haut et en bas, sans rien à faire défiler ; un bloc sans
données affiche un message au lieu de disparaître. Agrandie ou en plein écran, c'est le même tableau de
bord à l'échelle ; beaucoup plus petite, les blocs passent en une seule colonne, qui défile.

Icônes : la rune clé et les deux voies de runes s'affichent avec leur icône, les rangs avec leur emblème
(et la division, ou les LP à partir de Maître). Les images viennent du client LoL ; un emblème qu'il ne
donne pas est pris sur CommunityDragon (raw.communitydragon.org), qui reprend ses fichiers. Sans image,
le nom s'affiche à la place.

## Après la partie : historique

Peu après la fin (dès que le client l'a dans son historique), l'app lit la partie et l'enregistre :
résultat, KDA, CS exacts, or, dégâts aux champions, vision, durée, poste et adversaire direct.
Les parties annulées (moins de 5 minutes) ne sont pas comptées.

Au salon : l'illustration du skin de ta dernière partie (celle de base si l'app ne le connaît pas),
ton rang (solo/duo et flexible), tes 20 dernières parties d'après le client avec ta forme du moment
(victoire ou défaite, de la plus récente à la plus ancienne), tes champions du moment, l'historique
de tes 10 dernières parties (la plus récente en grand avec toutes ses stats, puis une ligne par
partie : résultat, file, KDA, CS, durée, quand ; victoire en vert-bleu, défaite en rouge-rose ;
adversaire et poste pour les parties suivies par l'app), et tes stats par champion (KDA, CS par
minute, % de victoires) et par duel. L'ARAM est compté à part. Les stats de l'app commencent à la
première partie jouée avec elle ; l'historique des 10 parties vient du client et les montre toutes.

En classée (solo/duo et flexible), les LP gagnés ou perdus : l'app lit ton rang au début de la partie
et après (une fois que ton rang compte la partie), et affiche l'écart avec une flèche vers le haut en
vert-bleu ou vers le bas en rouge-rose, dans l'historique (entre le mode de jeu et le KDA), sur la
dernière partie et dans son détail. Les montées et descentes de division sont comptées (100 LP par
division, 400 par rang, puis les LP de Maître et au-dessus). Seulement pour les parties jouées avec l'app.

Un clic sur une partie de l'historique (ou sur la dernière, en grand) déroule son détail par-dessus
le salon : les 10 joueurs, ton équipe à gauche et celle d'en face à droite (l'une sous l'autre si la
fenêtre est étroite). Pour chacun : champion et niveau, sorts, rune principale et voie secondaire,
nom et rang solo/duo actuel, score carry, KDA, dégâts aux champions (barre à l'échelle de la
partie), or, CS, balises posées et objets. Un nouveau clic sur la même partie, « FERMER » ou Échap
le referme. Le détail vient du client : il doit être lancé.

Le score carry (0 à 10) classe les 10 joueurs du MVP au dernier : médaille d'or avec une étoile pour le
MVP, d'argent et de bronze pour les 2e et 3e, et l'icône de leur champion dans un cadre épais de la même
couleur ; les autres ont leur place dans un rond. Il est calculé par l'app (U.GG et OP.GG ne publient pas
leur formule), selon le rôle de chacun : objet de support → support, Châtiment → jungle, sinon le couloir
donné par le client. Chaque stat est comparée à celle du joueur du même rôle en face (le KDA et la
participation aux kills, à toute la partie), puis moyenne pondérée selon le rôle :

| Stat | Haut | Jungle | Milieu | Bas | Support |
| --- | --- | --- | --- | --- | --- |
| KDA (ramené entre 0 et 1 : KDA / (KDA + 3)) | 3 | 3 | 3 | 3 | 4 |
| Participation aux kills | 1,5 | 2,5 | 2 | 1,5 | 2,5 |
| Dégâts aux champions | 2 | 1,5 | 2,5 | 2,5 | – |
| Or | 1,5 | 1 | 1,5 | 1,5 | – |
| CS | 1,5 | 1 | 1,5 | 2 | – |
| Vision | 0,5 | 2 | 0,5 | 0,5 | 2,5 |
| Dégâts encaissés | 1,5 | 1 | 0,5 | 0,5 | 1 |
| Dégâts aux objectifs | 1 | 2 | 0,5 | 1 | – |
| Impact : le meilleur de ses dégâts (Pyke, mages), soins et boucliers, ou contrôle | – | – | – | – | 2,5 |

Les soins viennent du client (les boucliers aussi s'il les donne). Rôle introuvable ou ARAM : KDA ×3,
participation ×2, dégâts ×2, or ×1,5, le reste ×1, comparés aux autres joueurs sans rôle (en ARAM, toute
la partie).

Plusieurs comptes sur le même PC : chaque partie est enregistrée avec le compte qui l'a jouée, et le
salon ne montre que celles du compte connecté (client fermé : celles du dernier compte qui a joué).
Les parties enregistrées avant ce rangement sont rangées sous leur compte au lancement suivant avec le
client : l'app relit chaque partie dans le client, 5 par 5, à l'accueil ou au salon.

Fichiers, dans `%APPDATA%\com.perso.lol-companion\` :
- `historique.jsonl` : une ligne par partie, avec le compte qui l'a jouée ;
- `derniere-partie.json` : la dernière partie complète, telle que le client l'a donnée ;
- `reglages.json` : tes réglages.

## Zone de notification

L'app a une icône près de l'horloge : clic gauche pour afficher la fenêtre, clic droit pour
« Afficher » ou « Quitter ». Par défaut la croix cache la fenêtre (l'app continue de suivre
le client) ; le réglage « La croix la cache… » décoché, la croix quitte. Relancer l'app
(menu Démarrer) remontre la fenêtre déjà ouverte.

Quand une partie est trouvée, une notification Windows le signale. L'app n'accepte rien
à ta place : c'est un choix de prudence vis-à-vis des règles de Riot sur les outils tiers.

## Où est quoi

- `src-tauri/src/lol.rs` : lecture du lockfile, appels au client (LCU) et à la partie (port 2999),
  imports, aide au choix, rangs, enregistrement des parties.
- `src-tauri/src/opgg.rs` : appel au service d'OP.GG et lecture de sa réponse.
- `src-tauri/src/history.rs` : fichier d'historique et stats.
- `src-tauri/src/settings.rs` : réglages.
- `src-tauri/src/lib.rs` : fenêtre, icône près de l'horloge, mises à jour, boucle qui prévient
  l'interface quand l'état change.
- `src-tauri/src/overlay.rs` : fenêtres posées sur le jeu (panneau des objets, écarts d'or) et touche Tab.
- `src/` : l'interface (`index.html`, `main.js`, `styles.css`) et les fenêtres en jeu (`overlay.html`
  pour le panneau, `gold.html` pour les écarts d'or).
- `src/fonts/` : les polices Barlow et Barlow Condensed de l'interface (licence SIL OFL 1.1, `OFL.txt`).
- `.github/workflows/release.yml` : construction de l'installateur sur GitHub.

Si le client n'est pas détecté (installation inhabituelle), indique le chemin du lockfile :

```powershell
$env:LOL_LOCKFILE = "D:\Riot Games\League of Legends\lockfile"
cargo run
```
