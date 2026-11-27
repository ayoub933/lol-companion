# LoL Companion (prototype perso)

Petite app desktop Tauri 2 (cœur Rust + interface HTML/JS) qui se connecte en local
au client League of Legends et à la partie en cours. Elle importe automatiquement
les runes, le build, les sorts et un set d'objets recommandés par OP.GG, aide à choisir
son champion, montre les rangs en partie et garde l'historique de tes parties.
Aucune clé API Riot n'est nécessaire.

## Installer (avec mises à jour automatiques)

L'installateur Windows est fabriqué par GitHub à chaque nouvelle version ; l'app
installée vérifie au démarrage puis toutes les 3 h s'il y en a une plus récente, et l'installe
toute seule quand tu n'es ni en sélection ni en partie.
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
   nouvel installateur. L'app installée la voit à son démarrage ou dans les 3 h (5 min après un échec,
   par exemple internet pas encore là au démarrage de Windows) et affiche un bandeau « Nouvelle
   version disponible » ; son bouton installe et relance l'app tout de suite.
   Sinon, elle s'installe toute seule après une minute sans sélection ni partie (ni file d'attente, ni
   fin de partie), fenêtre au premier plan ou non : une petite fenêtre de progression apparaît quelques
   secondes, puis l'app revient comme elle était (cachée près de l'horloge, ou
   affichée). 3 essais au plus par lancement, à 30 min d'écart ; ensuite, le bouton.

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
En développement, l'app ne cherche pas de mise à jour (sauf avec la variable `UPDATE_URL`, adresse
d'un `latest.json` de test). « Lancer avec Windows » n'y
fait rien : seule la version installée touche au démarrage de Windows.

## Réglages (roue ⚙ en haut à droite)

- Import automatique en sélection : page de runes, set d'objets, sorts d'invocateur
  (chacun peut être coupé). Les recommandations restent affichées.
- Saut éclair sur D ou F, ou « sa touche actuelle » (sinon F).
- Sets d'objets : un seul, remplacé à chaque partie, ou un par champion.
- En partie : le panneau des objets à acheter sur le bord gauche du jeu, en tenant Tab ou toute la
  partie, et les écarts d'or à côté du tableau des scores en tenant Tab (chacun peut être coupé).
- Noms des objets, runes et sorts (sélection, et « TON BUILD » en partie) : comme le client, ou en
  anglais. Les noms anglais sont ceux des fichiers du client en anglais, pris sur CommunityDragon
  (raw.communitydragon.org) la première fois ; s'il ne répond pas, les noms du client restent et l'app
  réessaie une minute plus tard. Les champions gardent le nom du client.
- Lancer avec Windows (l'app démarre cachée près de l'horloge), afficher la fenêtre
  quand le client LoL s'ouvre, notification quand une partie est trouvée, et ce que fait la croix.
  Le lancement avec Windows est activé par défaut, une seule fois : à la première ouverture de la version
  installée (ou à la mise à jour vers la 0.10.11 pour une installation plus ancienne). Décoché ensuite, il
  le reste. Jamais en développement (`cargo run`). Coché, la version installée réécrit sa commande à chaque
  ouverture (son chemin entre guillemets, puis `--minimized`) : une ancienne qui lancerait autre chose (exe de
  développement, avec un terminal ; ancien dossier) est remplacée.

Un réglage d'import changé en pleine sélection s'applique au prochain import (nouveau champion ou
adversaire) ; celui des noms, tout de suite. Le panneau se referme avec « FERMER », la roue, Échap, ou un clic
n'importe où ailleurs dans l'app (ce clic ne déclenche rien d'autre).

## Sélection des champions

En haut, ton champion avec le skin choisi (pour un chroma, l'illustration de son skin) face à
ton adversaire direct, puis le duel selon OP.GG : % de victoires de chaque côté, avantage, conseil.

Les deux équipes s'affichent en colonnes : la tienne à gauche, dans l'ordre des postes
(un champion seulement survolé est atténué, le tien est en bleu), les adversaires à droite, chacun en face
de l'allié du même poste (dans les parties avec postes).

Sous les colonnes, les dégâts de chaque équipe d'après la fiche de chaque champion dans le client : une barre
physiques (ambre) / magiques (violet), « 2 AD · 2 AP · 1 tank » (un champion aux dégâts mixtes compte pour moitié
de chaque côté). Tes alliés seulement survolés comptent : tu vois l'effet d'un choix avant de le verrouiller.
Un conseil s'ajoute quand une équipe n'a presque qu'un type de dégâts : pour la tienne, dès 3 champions, « peu de
dégâts magiques : un champion AP aiderait » (ou physiques) ; en face, « presque que de l'AD : l'armure sera
rentable » (3,5 champions sur 5 au moins, ou de l'AP : la résistance magique).

Le client ne donne pas les postes adverses : l'app les devine d'après OP.GG, avec la part des parties de
chaque champion à chaque poste. Elle prend la répartition la plus probable de toute l'équipe, deux
adversaires jamais au même poste (Elise jungle 78 % et Amumu jungle 70 % / support 26 % : Elise en jungle,
Amumu en support). Un adversaire dont OP.GG n'a pas encore donné les postes attend dans une case libre ; au
survol d'un adversaire, la part de ses parties à ce poste. Le terminal note les postes lus et la répartition
(« [postes] en face : Renekton top, Elise jungle, Zed mid… »).

**Pendant les bans : bans conseillés** (tant que ton ban n'est pas fait, dès la phase de déclaration). Trois
champions, avec les données OP.GG :
- d'abord, si tu as survolé ton champion, les 2 qui le battent le mieux à ton poste (liseré rouge, « BAT
  AHRI » et leur % de victoires contre lui), seulement s'ils gagnent plus d'une partie sur deux contre lui ;
- puis les mieux classés de ton poste dans le classement d'OP.GG (tier en couleur, % de victoires à ce
  poste).

Jamais un champion déjà banni ou pris, ni un champion de ton équipe (survolé ou choisi). Sans champion
survolé, les trois viennent du classement. Le bloc disparaît une fois ton ban fait.

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
1. écrit les runes dans ta page de runes sélectionnée, qui prend le nom « LOLC - <champion> » ;
2. met les sorts d'invocateur conseillés, le Saut éclair sur ta touche ;
3. affiche le build en icônes (leur nom au survol) : sorts d'invocateur (leur touche D ou F dans le coin),
   objets de départ (le nombre dans le coin), 3 objets principaux dans l'ordre d'achat, bottes, choix
   suivants, ordre de montée des compétences (icônes du champion, la touche dans le coin), puis la
   compétence de chaque niveau ;
4. ajoute à la boutique un set d'objets « LoL Companion · <champion> ».

- Runes : pas besoin de page libre, ce que contenait ta page sélectionnée est remplacé. Le client ne
  laisse pas modifier une page : l'app la supprime et la recrée à la même place ; si la recréation
  échoue, ta page est remise telle qu'elle était. Si la page sélectionnée est une page par défaut de Riot
  (qui ne se supprime pas), l'app remplace sa page « LOLC - … » (ou « LoL Companion » d'avant la 0.10.24), ou en
  crée une s'il reste de la place. Si tu changes de page ou de runes après l'import, l'app n'y touche plus jusqu'à la fin de la
  sélection (sauf si tu choisis d'un clic une autre page du duel, voir plus bas).
- Sorts : si tu les changes toi-même pendant la sélection, l'app n'y touche plus. Avec « sa touche
  actuelle », le Saut éclair reste là où tu l'avais mis ; si le client donne des sorts illisibles au
  moment de l'import, l'app prend ceux qu'elle a lus juste avant, et n'y touche pas si elle n'en a aucun.
  Le terminal note chaque changement (« [sorts] avant D = 4, F = 14 ; après … »).
- Set d'objets : le client n'accepte que la liste complète de tes sets. L'app la relit
  juste avant d'écrire, garde tes autres sets tels quels et n'écrit rien si la liste
  est illisible. Elle vérifie ensuite que tes sets sont toujours là.
- Support : OP.GG ne compte pas l'Atlas dans les objets de départ, l'app le remet en tête.

**Duel contre ton adversaire direct** (parties avec postes : classées, normales avec choix des postes) :
- L'app devine ton adversaire : celui placé à ton poste par la répartition des postes (voir plus haut), s'il y
  joue au moins 15 % de ses parties sur OP.GG, ou dès que les 5 adversaires sont connus. Clique sur un
  adversaire pour choisir toi-même : il passe à ton poste, les autres se répartissent autour ; reclique
  dessus pour revenir à la devinette.
- Runes, build et set d'objets passent alors à ceux du duel (« vs Sett »), avec tes stats contre lui
  et le conseil d'OP.GG (en anglais, OP.GG ne le traduit pas). Moins de 50 parties pour la page de
  runes du duel : les runes générales restent.
- Pages de runes du duel : la plus jouée, puis jusqu'à 2 autres (au moins 50 parties chacune) qui changent
  au moins une rune (pas seulement les fragments), chacune avec ses runes en icônes (nom au survol), son
  % de victoires et ses parties à droite. Celle mise dans le client est en bleu (au départ, la plus jouée).
  Un clic sur une autre la met à sa place, dans la page « LOLC - … » de l'app, jamais dans une de tes
  pages que tu aurais sélectionnée depuis ; choisie pendant l'aperçu, elle est importée au verrouillage.
  Sans adversaire direct connu, OP.GG ne donne qu'une page (son analyse générale) : les autres arrivent
  avec lui.
- Objets du duel (départ, objets principaux, bottes) : le meilleur % de victoires parmi les choix
  joués au moins 100 fois (en dessous, un % élevé peut être du hasard), sinon le plus joué. L'autre choix
  du 1er et du 2e objet n'est jamais un des 3 objets principaux.
- Si l'app a déjà enregistré des parties de ce duel, ton bilan s'affiche (« 2 V – 1 D »).

**Kayn : forme rouge ou bleue.** L'analyse générale d'OP.GG mélange ses deux formes (sa page la plus jouée
peut être bleue et son build rouge). Pour Kayn, l'app prend donc runes, sorts, build et set d'objets dans les
données du duel, triées par forme :
- runes : voie principale Précision (Conquérant) pour le rouge, une autre (Moisson noire, Grêle de lames…)
  pour le bleu ;
- objets principaux : une combinaison est rouge si elle compte plus d'objets de combattant (santé ou armure :
  Couperet noir, Danse de la mort…) que d'objets de létalité (Hubris, Cyclosabre voltaïque…), bleue sinon,
  d'après les catégories de la boutique du client. Autres choix : un objet présent dans les combinaisons va
  avec une forme si ses parties dans les combinaisons de cette forme font au moins le quart de celles de l'autre
  (Cyclosabre voltaïque va aux deux, Danse de la mort au rouge seulement) ; un objet absent des combinaisons
  est écarté s'il est de l'autre forme (combattant pour le bleu, létalité seule pour le rouge) ;
- tant que ton adversaire direct n'est pas connu (ou si son duel a trop peu de parties), le duel de Kayn
  contre Lee Sin, un jungler très joué, sert de référence.

La forme est conseillée d'après l'équipe d'en face : rouge s'il y a plus de champions de mêlée (ils donnent les
orbes rouges), bleue s'il y a plus de champions à distance ; à égalité, rien n'est conseillé (build général en
attendant). Les boutons Rouge / Bleue du bloc Runes la choisissent ; un reclic revient à la forme conseillée.
Le set d'objets s'appelle « LoL Companion · Kayn rouge vs … ». En partie, le panneau sur le jeu et « TON
BUILD » suivent la forme retenue en sélection. Le terminal note ce qui a été pris (« [forme] Kayn red : page
8010 (432 parties contre Lee Sin) »).

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

**Fiche rapide de chaque joueur**, sur la ligne de son rang, d'après ses 20 dernières parties (les mêmes, lues
pour les duos ; toutes files, sans les parties annulées) :
- sa série en cours, à partir de 3 d'affilée : « 3 V d'affilée » en vert, « 4 D d'affilée » en rouge ;
- ses parties sur le champion qu'il joue : « Darius : 12 sur 20 · 58 % » (le % à partir de 3 parties), ou
  « Zed : 0 sur ses 20 dernières ».

**OTP** (« one-trick », il joue surtout ce champion) : une pastille « OTP » vert vif à côté du nom, quand au moins
60 % de ses dernières parties (10 lues au moins) sont sur ce champion ; au survol, combien et son % de victoires.

Elle arrive pendant l'écran de chargement, au fur et à mesure (2 joueurs par seconde). Rien pour un joueur en
mode streamer.

**Smurf probable** : un hexagone rouge avec un masque à côté du nom (au survol, son bilan et le niveau de son
compte). D'après son rang dans la file de la partie (solo/duo, flexible pour une flexible), avec au moins 10
parties classées cette saison :
- peu de parties (100 au plus), au moins 65 % de victoires, et trop pour être de la chance (victoires − défaites
  ≥ 2 × √parties, soit plus de 2 écarts-types au-dessus de 50 %). Exemples : 15 V – 5 D, oui ; 14 V – 6 D, non
  (peut être de la chance) ; 130 V – 70 D, non (beaucoup de parties : juste un bon joueur) ;
- ou compte jeune (niveau sous 60 : les classées s'ouvrent au niveau 30), au moins 60 % de victoires et
  victoires − défaites ≥ √parties. Exemple : 13 V – 7 D au niveau 41, oui (au niveau 120, non).

Le niveau du compte est lu avec ses dernières parties pendant l'écran de chargement (une demande de plus par
joueur, une fois par partie) : avant, seule la première règle compte.

Le client met à 0 les défaites des autres joueurs (vérifié sur un vrai client en novembre 2026 : ses victoires
sont justes). Dans ce cas, les mêmes règles se lisent sur ses dernières parties dans la file de la partie (au
moins 10, lues pendant le chargement), et « peu de parties » devient au plus 65 victoires cette saison
(100 parties à 65 %). Le survol donne alors ce bilan récent et ses victoires de la saison.

Le terminal note les parties lues, celles jouées ensemble et les joueurs croisés
(« [groupes] partie … : … ; ensemble dans la même équipe : Jinx + Nami 3 ; déjà croisés : … »).

Une fois la partie lancée :
- deux bandes : les éliminations de chaque équipe et le temps de jeu, puis l'or en objets de chaque
  équipe (valeur de ses objets au prix de la boutique, l'or en poche ne compte pas) avec l'écart en
  grand au milieu, sur une case bleue si ton équipe est devant, orange si c'est l'équipe d'en face
  (grise à moins de 100 pièces d'écart) ;
- ta carte : ton skin, KDA, CS, or en poche ;
- en dessous, « TON BUILD » (un peu plus de la moitié de la colonne), le même build que le panneau sur le jeu mais en détail
  (au survol d'une icône : nom et prix) :
  - prochain achat : l'objet, son prix, ce qu'il te manque avec ton or en poche (ses composants que tu as
    déjà sont déduits, comme dans la boutique) et ses composants, ceux que tu as cochés ;
  - ordre : départ › 1er objet (son autre choix en petit) › bottes › 2e › 3e, ce que tu as grisé, la
    prochaine étape cerclée d'or. Au poste bas, les bottes de l'emplacement de quête n'apparaissent pas dans
    l'API de la partie (seulement tes 6 emplacements et la balise) : de l'or dépensé sans objet de plus dans
    ton inventaire visible compte comme des bottes dans cet emplacement (300 : de base ; dès 450 : améliorées) ;
  - sets d'objets : les 3 objets principaux les plus joués dans ce duel (le classique, puis 2 autres, au moins
    50 parties chacun), avec leur % de victoires et leurs parties ; celui de l'ordre en bleu. Seulement dans
    l'app, pas sur le panneau posé sur le jeu ;
  - ensuite : les choix suivants avec leur % de victoires sur OP.GG ; contre eux : les objets contre l'équipe
    d'en face, avec la raison ;
  - la montée des compétences (icônes de ton champion), et ce que l'app voit en face (« 4 AD · 1 AP · 3 tanks ·
    soins : Dr. Mundo ») ;
  - l'astuce d'OP.GG contre ton adversaire de couloir de la partie, avec le style de jeu conseillé (« joue
    prudemment »), sur 2 lignes (en entier au survol ; en anglais, OP.GG ne la traduit pas) ;
- le face-à-face, couloir par couloir : niveau, KDA et rang de chacun, et au milieu l'écart d'or
  du couloir, sur une case aux mêmes couleurs. Sans postes (ARAM...), les deux équipes côte à côte.
  « Rang masqué » : joueur en mode streamer.

Deux choses s'affichent par-dessus le jeu : le panneau des objets (en tenant Tab, ou toute la partie
selon le réglage) et les écarts d'or (en tenant Tab, avec le tableau des scores). Leurs fenêtres sont créées
au début de la partie et fermées une fois la partie finie (« [panneau] fenêtre … fermée » dans le terminal) :
entre deux parties, elles ne gardent pas de mémoire.

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

**Ctrl+X** pendant la partie (écran de chargement compris), jeu au premier plan : la fenêtre de l'app vient
par-dessus le jeu, au milieu, aux 2/3 de sa hauteur (1280 × 720 sur un jeu en 1920 × 1080), sans bordure, et
prend le clavier. Ctrl+X encore (l'app ou le jeu au premier plan) : elle revient où et comme elle était (autre
écran, agrandie, réduite ou cachée près de l'horloge) et le jeu reprend le clavier ; la fin de la partie la
remet aussi à sa place. Les touches sont seulement lues (comme Tab) : Ctrl+X reste « couper » dans les autres
apps, et arrive aussi au jeu. Il faut LoL en « Sans bordure », comme pour les panneaux.

Par-dessus le jeu, l'app montre les 10 joueurs en cartes espacées, comme l'écran de chargement : ton équipe en
haut, sous « TON ÉQUIPE » en bleu, celle d'en face en bas, sous « EN FACE » en orange, chacune rangée par poste ;
le fond des cartes est à peine teinté de la couleur de l'équipe. Ta carte a un cadre bleu clair et « TOI » après
ton champion. Chaque carte :
- l'illustration de chargement de son skin (chroma compris), son champion, son pseudo ; ses pastilles (venu en
  groupe, déjà croisé, smurf, OTP, série de 3 ou plus) ; en partie, son niveau et son score ;
- son rang (« Émeraude II · 45 LP ») ;
- sa saison en classée (solo et flexible ensemble) d'après OP.GG, en gros et sans libellés pour se lire d'un coup
  d'œil : % de victoires, parties et KDA sur le champion qu'il joue (son icône devant), puis sur toute sa saison
  (une coupe devant) ; ses 3 champions les plus joués avec leur % de victoires (une étoile devant). Le KDA : par
  partie (« 4,5 / 7,1 / 8 », sous le %) ou en ratio (« 2,6 », à droite), au choix dans les réglages. % de
  victoires en bleu clair à partir de 55 %, en rouge à 45 % ou moins, en gris sous 10 parties ; KDA en bleu clair
  à partir de 3,5, en rouge sous 2 ;
- un clic : son profil (« LA PARTIE » pour revenir aux cartes).

À droite, ton build : le même que sur l'écran de partie, en plus grand et réparti sur toute la hauteur
(prochain achat et l'or qu'il te manque, ordre des achats, sets d'objets du duel, suite, objets contre eux,
montée des compétences, ce qu'il y a en face, astuce du duel en entier). Il arrive au début de la partie. En
bas, sur une ligne : l'or en objets de chaque équipe et l'écart, en bleu quand ton équipe est devant, en rouge
quand c'est l'autre.

Consommation : la saison de chaque joueur est demandée une fois par partie à OP.GG (une petite réponse par
joueur, l'une après l'autre, à part de la lecture de la partie) ; si OP.GG ne la donne pas, elle est redemandée
jusqu'à 2 fois, 15 s plus tard, puis sa carte le dit (« introuvable sur OP.GG » ou « OP.GG ne répond pas ») au
lieu d'attendre. Les 10 illustrations (~50 Ko chacune) sont gardées pour la partie en cours seulement. Aucune
fenêtre en plus : c'est celle de l'app.

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
(victoire ou défaite, de la plus ancienne à gauche à la plus récente à droite), tes champions du moment (voir
plus bas), l'historique
de tes 10 dernières parties (la plus récente en grand avec toutes ses stats, puis une ligne par
partie : résultat, file, KDA, CS, durée, quand ; victoire en vert-bleu, défaite en rouge-rose ;
adversaire et poste pour les parties suivies par l'app), et tes stats par champion (KDA, CS par
minute, % de victoires). L'ARAM est compté à part. Les stats de l'app commencent à la
première partie jouée avec elle ; l'historique des 10 parties vient du client et les montre toutes.

Le client renvoie parfois une liste de parties raccourcie, jusqu'à une seule (juste après une partie, ou pendant
qu'il recharge ton historique chez Riot) : l'app la complète avec les parties déjà vues (lecture précédente,
parties qu'elle a suivies), sans jamais en perdre ; le terminal le note (« [profil] le client n'a donné que
1 partie(s) : complété… »). Tant qu'il en donne moins de 20, ton profil est relu toutes les 10 s (6 fois au
plus) : après une connexion ou un changement de compte, ton historique et ta courbe de LP reviennent dès que
le client a fini de les recharger.

**Tes LP** (colonne de droite) : la courbe de tes LP en solo/duo sur tes dernières classées suivies par l'app,
avec le total gagné ou perdu (« +45 LP sur tes 5 dernières classées »). Un point par partie (vert-bleu :
victoire, rouge-rose : défaite), les limites de division en repère (« É II » : Émeraude II), et au survol la
partie (LP, champion, quand) et ton rang après elle. Elle est retrouvée à rebours depuis ton rang actuel avec
les LP que l'app a vus après chaque partie, et s'arrête à la première classée dont elle ne connaît pas les LP
(jouée sans l'app, par exemple) : au-delà, elle serait fausse. Rien à charger en plus.

**Champions du moment** : tes 3 champions les plus joués sur tes 20 dernières parties, avec ton bilan, et leur
place dans le classement d'OP.GG (« tier list ») du patch : tier en couleur (T1, le meilleur, en or ; T2 en
vert-bleu ; T3 en bleu ; T4 en gris ; T5 en rouge-rose), poste, % de victoires à ce poste, et places gagnées
(▲, en vert-bleu) ou perdues (▼, en rouge-rose) depuis le patch précédent (« = » : même place). Au survol :
sa place exacte (« 3e sur 52 ») et celle du patch précédent. Le poste est celui où tu joues le plus ce
champion d'après les parties suivies par l'app, sinon son poste principal sur OP.GG. Le classement est
celui d'OP.GG tous rangs confondus (leur outil ne le donne pas par rang) : une seule petite réponse
(15 ko environ) pour tous les champions, relue toutes les 3 heures (5 minutes après un échec). Le terminal
note sa lecture (« [méta] classement OP.GG : top 52, jungle 48… ; noms non retrouvés dans le client : aucun »).

En classée (solo/duo et flexible), les LP gagnés ou perdus : l'app lit ton rang au début de la partie
et après (une fois que ton rang compte la partie : une victoire ou une défaite de plus, ou d'autres LP), et
affiche l'écart avec une flèche vers le haut en
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

## Rechercher un joueur ou un champion

La barre en haut, alignée sur la colonne du milieu, sur tous les écrans : son Riot ID (`Pseudo#TAG`), sa
région, puis Entrée. Ou le nom d'un champion (voir plus bas).
Son salon s'affiche alors à la place de l'écran du moment (rang, forme, champions du moment, 10 dernières
parties, détail d'une partie d'un clic). Le bouton à gauche de la barre (« Mon profil », « La sélection »
ou « La partie ») ou Échap dans la barre te ramènent à cet écran ; un nouvel écran (sélection qui commence,
partie qui démarre ou se termine) aussi.

**Tes amis** : un clic sur la barre ouvre ta liste d'amis du client juste en dessous (rien au salon sinon).
D'abord ceux qui jouent (en partie avec leur champion, leur file et depuis combien de minutes, en
sélection, en file, sur un autre jeu Riot), puis ceux en ligne (dans un salon, en ligne, absent, sur
mobile), puis les autres (hors ligne, vus le plus récemment d'abord) ; la molette fait défiler. Ce que tu
tapes ensuite la filtre ; un clic sur un ami ouvre son profil. Un clic ailleurs ou Échap la referme. Elle
n'est lue dans le client qu'à l'ouverture puis toutes les 20 s tant qu'elle est ouverte ; les icônes de
profil ne se chargent qu'en arrivant à l'écran.

Un clic sur un joueur ouvre aussi son profil : sur son nom dans le détail d'une partie (y compris celles
d'un joueur cherché), ou sur sa case dans l'écran de la partie en cours (sauf les joueurs en mode streamer).
Pour les joueurs de tes parties, l'app le lit directement par son compte dans le client.

- Dans la région de ton client : l'app le lit dans le client, comme ton salon (avec le score carry).
- Ailleurs, ou si le client ne le trouve pas (ou est fermé) : sur OP.GG. Le détail d'une partie donne
  alors la note OP.GG (sur 10, ×10) et sa place à la place du score carry ; pas de score de vision.

Le client met à 0 les défaites des autres joueurs : leur % de victoires de la saison vient alors d'OP.GG
(« (OP.GG) » après le nombre de parties ; parfois en retard de quelques parties), le rang et les LP restant
ceux du client. Si OP.GG ne répond pas : ses victoires seulement (« défaites inconnues »). Même chose pour
ton salon si le client cachait aussi tes défaites.

Ses LP gagnés ou perdus par partie ne sont pas connus : l'app ne les voit que pour tes parties.

**Un champion** : en tapant, les champions dont le nom commence par ce texte s'affichent au-dessus de tes
amis (« da » : Darius ; « s » : Samira, Sejuani, Senna, Séraphine…), 6 au plus, sans tenir compte des accents
ni des signes (« kai » : Kai'Sa) ; un mot du nom marche aussi (« yi » : Maître Yi). Un clic, ou Entrée (le
premier proposé), ouvre sa page, comme sur DPM, d'après OP.GG (parties classées du patch en cours) :
- à gauche : son illustration, son tier et sa place au classement de son poste ; ses stats (victoires,
  choix, bans, parties, KDA, part de ses parties à ce poste), un onglet par poste où il est joué (au moins 1 %
  de ses parties) ; sa page de runes (avec son % de victoires) et ses compétences : sa montée, puis celle
  prise à chaque niveau ;
- au milieu : son build (sorts, départ, bottes, objets principaux, avec leur % de victoires et leurs
  parties ; ses 4e, 5e et 6e objets les plus joués, le % de victoires de chacun dessous) et ses 5 duels les
  plus faciles et les 5 plus durs à ce poste (son % de victoires contre eux) ;
- à droite : ses meilleurs partenaires (2 par poste) et ses 5 meilleurs joueurs dans la région de ton client.

Un clic sur un champion (duel, partenaire) ouvre sa page ; un clic sur un joueur, son profil. Sans poste
choisi, la page s'ouvre à son poste le plus joué. Elle est lue sur OP.GG seulement à l'ouverture (3 ou 4
petites demandes), puis gardée par l'app jusqu'à sa fermeture. OP.GG ne donne pas l'évolution par patch.

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
- `src-tauri/src/search.rs` : recherche d'un joueur (client ou OP.GG) pour afficher son salon, et page d'un
  champion (OP.GG).
- `src-tauri/src/history.rs` : fichier d'historique et stats.
- `src-tauri/src/settings.rs` : réglages.
- `src-tauri/src/lib.rs` : fenêtre, icône près de l'horloge, mises à jour, boucle qui prévient
  l'interface quand l'état change.
- `src-tauri/src/overlay.rs` : fenêtres posées sur le jeu (panneau des objets, écarts d'or), touche Tab, et
  l'app par-dessus le jeu avec Ctrl+X.
- `src/` : l'interface (`index.html`, `main.js`, `styles.css`) et les fenêtres en jeu (`overlay.html`
  pour le panneau, `gold.html` pour les écarts d'or).
- `src/fonts/` : les polices Barlow et Barlow Condensed de l'interface (licence SIL OFL 1.1, `OFL.txt`).
- `.github/workflows/release.yml` : construction de l'installateur sur GitHub.

Si le client n'est pas détecté (installation inhabituelle), indique le chemin du lockfile :

```powershell
$env:LOL_LOCKFILE = "D:\Riot Games\League of Legends\lockfile"
cargo run
```
