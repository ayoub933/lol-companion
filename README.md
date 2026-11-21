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

Versions suivantes (ex. 0.9.1), depuis le dossier `lol-companion` :

1. Le numéro, à 3 endroits : dans `src-tauri/tauri.conf.json`, `"version": "0.9.1"` et le titre
   `"LoL Companion 0.9.1"` ; dans `src-tauri/Cargo.toml`, `version = "0.9.1"`.
2. `cargo build --manifest-path src-tauri/Cargo.toml` (met `Cargo.lock` au même numéro).
3. Envoyer le code puis le tag :
   ```powershell
   git add -A
   git commit -m "Version 0.9.1"
   git push
   git tag v0.9.1
   git push origin v0.9.1
   ```
4. L'onglet Actions du dépôt montre la construction (10 à 15 minutes), puis la page Releases a le
   nouvel installateur. L'app installée affiche un bandeau « Nouvelle version disponible » à son
   démarrage suivant ; le bouton installe et relance l'app.

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
- En partie : le panneau des objets à acheter sur le bord gauche du jeu, en tenant Tab ou toute la
  partie, et les écarts d'or à côté du tableau des scores en tenant Tab (chacun peut être coupé).
- Noms des objets, runes et sorts (dans la sélection) : comme le client, ou en anglais. Les noms anglais
  sont ceux des fichiers du client en anglais, pris sur CommunityDragon (raw.communitydragon.org) la
  première fois ; s'il ne répond pas, les noms du client restent et l'app réessaie une minute plus tard.
  Les champions gardent le nom du client.
- Lancer avec Windows (l'app démarre cachée près de l'horloge), afficher la fenêtre
  quand le client LoL s'ouvre, notification quand une partie est trouvée, et ce que fait la croix.

Un réglage d'import changé en pleine sélection s'applique au prochain import (nouveau champion ou
adversaire) ; celui des noms, tout de suite.

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

**Un clic sur un champion de la grille, sans le verrouiller**, affiche déjà ses runes, son build et le
duel contre ton adversaire (« Aperçu ») : rien n'est mis dans le client tant qu'il n'est pas verrouillé.

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

**Duos et groupes**, dans les deux équipes : une pastille ronde et brillante avec un maillon à côté du
champion des joueurs venus ensemble, de la même couleur pour tout le groupe (or, violet…) ; au survol,
avec qui. L'app lit les 20 dernières parties de chaque joueur (le client les demande à Riot, 2 joueurs par
seconde, donc pendant l'écran de chargement) : deux joueurs de la même équipe qui en ont joué au moins 2
ensemble, du même côté, sont venus ensemble (une seule partie en commun peut être un hasard du matchmaking).
Un duo qui n'a encore jamais joué ensemble n'est donc pas repéré, et les joueurs en mode streamer ne peuvent
pas être vérifiés.

**Joueurs déjà croisés** : une pastille carrée argentée (une horloge qui recule) à côté des joueurs, alliés
ou adversaires, qui étaient dans une de tes 3 dernières parties sans être venus avec toi ; au survol, quand
(dernière, avant-dernière ou 3e partie), le champion qu'il jouait et s'il était avec toi ou contre toi.

Le terminal note les parties lues, celles jouées ensemble et les joueurs croisés
(« [groupes] partie … : … ; ensemble dans la même équipe : Jinx + Nami 3 ; déjà croisés : … »).

Une fois la partie lancée :
- deux bandes : les éliminations de chaque équipe et le temps de jeu, puis l'or en objets de chaque
  équipe (valeur de ses objets au prix de la boutique, l'or en poche ne compte pas) avec l'écart en
  grand au milieu, sur une case bleue si ton équipe est devant, orange si c'est l'équipe d'en face
  (grise à moins de 100 pièces d'écart) ;
- ta carte : ton skin, KDA, CS, or en poche ;
- le face-à-face, couloir par couloir : niveau, KDA et rang de chacun, et au milieu l'écart d'or
  du couloir, sur une case aux mêmes couleurs. Sans postes (ARAM...), les deux équipes côte à côte.
  « Rang masqué » : joueur en mode streamer.

Deux choses s'affichent par-dessus le jeu : le panneau des objets (en tenant Tab, ou toute la partie
selon le réglage) et les écarts d'or (en tenant Tab, avec le tableau des scores).

**Panneau des objets à acheter**, collé au bord gauche de l'écran, à mi-hauteur : juste les icônes,
une rangée par étape, de haut en bas :
- 1. départ, 2. 1er objet (avec un autre choix, plus petit), 3. bottes, 4. 2e objet (avec un autre
  choix), 5. 3e objet. Ce sont les objets du duel contre ton vrai adversaire de couloir, choisis comme
  en sélection ; trop peu de parties sur ce duel : ceux de ton champion à ton poste ;
- « + » : la suite, les 3 choix suivants les plus joués sur OP.GG ;
- « VS » : jusqu'à 3 objets contre l'équipe d'en face, pris parmi les choix d'OP.GG pour ton champion,
  avec la raison sous l'icône :
  - SOINS : un gros soigneur en face (Soraka, Aatrox, Vladimir, Dr. Mundo…) : un objet à Blessures
    graves, sinon le composant qui va avec tes dégâts (Appel du bourreau, Orbe d'oubli, Gilet épineux
    pour un tank) ;
  - AD ou AP : en face, au moins 3,5 champions sur 5 font des dégâts physiques (ou magiques ; un
    champion mixte compte pour moitié) : un objet d'armure (ou de résistance magique) ;
  - TANKS : 2 tanks ou plus en face : de la pénétration d'armure (magique si tes dégâts sont magiques).

  Le type de dégâts et les rôles viennent de la fiche de chaque champion dans le client, les
  catégories des objets de la boutique. Le terminal note ce qu'il voit en face
  (« [panneau] en face : 4 AD, 1 AP, 3 tank(s), 1 soigneur(s) »).

Rien ne disparaît du panneau :
- un objet que tu as est grisé, avec une coche verte ; une étape faite aussi (le départ dès ton premier
  achat, même si c'est un autre départ ; les bottes dès que tu as des bottes améliorées, même d'autres) ;
- un objet gardé 30 secondes reste grisé même vendu ou utilisé ensuite (un achat annulé dans les
  30 secondes ne compte pas) ;
- la prochaine étape a son numéro et un contour dorés.

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
(victoire ou défaite, de la plus ancienne à gauche à la plus récente à droite), tes champions du moment, l'historique
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
partie), or, CS, balises posées et objets. Pour le refermer : le bouton rouge « FERMER ✕ », Échap, ou un clic
n'importe où ailleurs dans l'app. Le détail vient du client : il doit être lancé.

Le score carry (0 à 100) classe les 10 joueurs : médaille d'or avec une étoile pour le meilleur score,
d'argent et de bronze pour les 2e et 3e, et l'icône de leur champion dans un cadre épais de la même
couleur ; les autres ont leur place dans un rond. Comme sur dpm.lol, « MVP » marque le meilleur de
l'équipe gagnante et « ACE » le meilleur de l'équipe perdante.

Le barème reprend celui de dpm.lol, retrouvé à partir de leurs notes détaillées (12 parties classées,
120 joueurs) : 15 points de base, puis

- global : kills et assistances (+0,75, +0,8 en haut, +0,85/+0,9 en support), morts (−1,5, −1,25 en
  support), premier sang (+5) ; or, dégâts, CS et vision par minute comparés à une référence du rôle
  (de −4 à +10 chacun ; pas de CS pour le support) ;
- couloir : écart d'or et de CS par minute avec le joueur du même rôle en face (de −5 à +10) ;
- objectifs : ceux de l'équipe (dragon et nashor 2, héraut 3, larve 0,5) et les dégâts aux objectifs ;
- équipe : participation aux kills (au-delà de 25 %), part des dégâts et des dégâts encaissés ;
- résultat : +3 pour une victoire, −3 pour une défaite ;
- rôle : première tour de son couloir (+5 au joueur dont l'équipe l'a prise ; le client ne dit que qui a
  pris la première tour de la partie, et dans les autres couloirs c'est l'équipe gagnante, comme dans 30
  couloirs sur 36 chez dpm.lol) et tours détruites ; pour le support, balises de contrôle, balises
  détruites et quête de support finie ; et, pour ce que le client ne donne pas (plaques, solo kills,
  crabes, alliés sauvés…), la moyenne de dpm.lol pour ce rôle.

Le client ne donne pas les stats de début de partie (avance à 15 minutes, CS à 10 minutes) : le couloir
utilise donc les écarts de fin de partie. Sur ces 12 parties, le classement obtenu suit celui de dpm.lol
à 0,97 près (corrélation de rang) et les notes à environ 4 points près.

Le rôle de chaque joueur vient de l'objet lié au rôle que le client donne (quêtes de rôle) ; à défaut,
équipe par équipe : Châtiment → jungle, objet de support → support, puis le couloir annoncé par le client
pour le haut, le milieu et le bas (une fois chacun, le dernier rôle libre au dernier joueur). Ce couloir
vient d'une vieille estimation de Riot qui se trompe parfois (des tops annoncés en jungle) : la jungle et
le support ne s'y fient jamais. Hors de la Faille (ARAM…), tout le monde est noté comme un milieu, sans
couloir ni rôle.

Plusieurs comptes sur le même PC : chaque partie est enregistrée avec le compte qui l'a jouée, et le
salon ne montre que celles du compte connecté (client fermé : celles du dernier compte qui a joué).
Les parties enregistrées avant ce rangement sont rangées sous leur compte au lancement suivant avec le
client : l'app relit chaque partie dans le client, 5 par 5, à l'accueil ou au salon.

Fichiers, dans `%APPDATA%\com.perso.lol-companion\` :
- `historique.jsonl` : une ligne par partie, avec le compte qui l'a jouée ;
- `derniere-partie.json` : la dernière partie complète, telle que le client l'a donnée ;
- `reglages.json` : tes réglages.

## Rechercher un joueur

La barre au milieu en haut, sur tous les écrans : son Riot ID (`Pseudo#TAG`), sa région, puis Entrée.
Son salon s'affiche alors à la place de l'écran du moment (rang, forme, champions du moment, 10 dernières
parties, détail d'une partie d'un clic). Le bouton à gauche de la barre (« Mon profil », « La sélection »
ou « La partie ») ou Échap dans la barre te ramènent à cet écran ; un nouvel écran (sélection qui commence,
partie qui démarre ou se termine) aussi.

Un clic sur un joueur ouvre aussi son profil : sur son nom dans le détail d'une partie (y compris celles
d'un joueur cherché), ou sur sa case dans l'écran de la partie en cours (sauf les joueurs en mode streamer).
Pour les joueurs de tes parties, l'app le lit directement par son compte dans le client.

- Dans la région de ton client : l'app le lit dans le client, comme ton salon (avec le score carry).
- Ailleurs, ou si le client ne le trouve pas (ou est fermé) : sur OP.GG. Le détail d'une partie donne
  alors la note OP.GG (sur 10, ×10) et sa place à la place du score carry ; pas de score de vision.

Ses LP gagnés ou perdus par partie ne sont pas connus : l'app ne les voit que pour tes parties.

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
- `src-tauri/src/search.rs` : recherche d'un joueur (client ou OP.GG) pour afficher son salon.
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
