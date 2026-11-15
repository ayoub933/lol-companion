//! Connexion au client LoL (API locale « LCU ») et à la partie en cours
//! (Live Client Data API). Tout se passe en local sur 127.0.0.1 :
//! aucune clé API Riot n'est nécessaire.

use crate::{history, opgg, settings::Settings};
use reqwest::Method;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// API officielle de Riot pour la partie en cours (disponible seulement en jeu).
const LIVE_CLIENT: &str = "https://127.0.0.1:2999/liveclientdata";
/// Nom de la page de runes que l'app remplace à chaque import,
/// et début du titre de son set d'objets (« LoL Companion · Ahri »).
const PAGE_NAME: &str = "LoL Companion";
/// Part minimale des parties à ton poste pour deviner qu'un champion adverse est ton adversaire direct.
const MIN_ROLE_RATE: f64 = 0.15;
/// En dessous de ce nombre de parties pour la page de runes du duel, on garde les runes générales.
const MIN_MATCHUP_GAMES: u64 = 50;
/// Atlas, l'objet de départ de tous les supports (OP.GG ne le compte pas dans les départs).
const ATLAS: i64 = 3865;
/// Saut éclair.
const FLASH: i64 = 4;
/// Partie plus courte (en secondes) : partie annulée (« remake »), pas comptée.
const REMAKE_SECONDS: u64 = 300;
/// Parties montrées dans l'historique du salon.
const MATCHES_SHOWN: usize = 10;
/// Après une partie, ses stats arrivent dans l'historique du client avec un peu de retard.
const RECORD_RETRY: Duration = Duration::from_secs(5);
const RECORD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// Prix total à partir duquel un objet qui ne s'améliore plus compte comme objet terminé (hors bottes).
const FINISHED_PRICE: u64 = 2000;
/// Illustrations de skins gardées en mémoire (les plus récentes).
const SPLASH_CACHE: usize = 6;

/// Ce que l'interface affiche. Envoyé au front à chaque changement.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub client_found: bool,
    pub riot_id: Option<String>,
    pub summoner_level: Option<u64>,
    /// Phase du client : "Lobby", "ChampSelect", "InProgress"...
    pub phase: Option<String>,
    /// Ton rang et tes dernières parties, d'après le client.
    pub profile: Option<Profile>,
    /// Présent seulement pendant la sélection des champions.
    pub runes: Option<RuneImport>,
    /// Présent seulement pendant une partie.
    pub game: Option<Game>,
    /// Historique de l'app : dernière partie et stats (absent tant qu'aucune partie n'est enregistrée).
    pub history: Option<history::Overview>,
    /// true pendant la récupération des stats de la partie qui vient de finir.
    pub recording: bool,
    /// Nouvelle version de l'app à installer (ajoutée par lib.rs).
    pub update: Option<String>,
}

/// Rang classé dans une file. `tier` vide = non classé.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Rank {
    /// "IRON", "BRONZE"... "CHALLENGER".
    pub tier: String,
    /// "I" à "IV" ; vide à partir de Maître.
    pub division: String,
    pub lp: i64,
    pub wins: u64,
    pub losses: u64,
}

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub solo: Option<Rank>,
    pub flex: Option<Rank>,
    /// Tes 20 dernières parties, sans les parties annulées (« remakes ») : idem pour tout ce qui suit.
    pub recent_games: u64,
    pub recent_wins: u64,
    /// Victoire (true) ou défaite pour chacune de ces parties, de la plus récente à la plus ancienne.
    pub recent_results: Vec<bool>,
    /// Champion de ta partie la plus récente ; 0 si aucune.
    pub last_champion_id: i64,
    /// Tes champions les plus joués sur ces parties.
    pub recent_champions: Vec<RecentChampion>,
    /// Tes 10 dernières parties, de la plus récente à la plus ancienne (adversaire et poste si l'app les a suivies).
    pub matches: Vec<history::GameRecord>,
}

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecentChampion {
    pub champion_id: i64,
    pub name: String,
    pub games: u64,
    pub wins: u64,
}

/// Résultat de l'import (runes, build, set d'objets). `champion` vide = champion pas encore verrouillé.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RuneImport {
    pub champion: String,
    /// "top", "jungle", "mid", "adc" ou "support".
    pub position: String,
    /// true pendant la recherche sur OP.GG.
    pub pending: bool,
    /// Erreur sur les runes (le build peut quand même être là).
    pub error: Option<String>,
    pub keystone: String,
    pub primary: String,
    pub secondary: String,
    /// Numéros de la rune clé et des deux voies (pour leurs icônes) ; 0 si inconnus.
    pub keystone_id: i64,
    pub primary_id: i64,
    pub secondary_id: i64,
    pub games: u64,
    pub wins: u64,
    pub build: Option<opgg::Build>,
    /// Noms des sorts d'invocateur conseillés, dans la langue du client.
    pub spells: Vec<String>,
    /// true si le set d'objets est dans le client.
    pub item_set_ok: bool,
    pub item_set_error: Option<String>,
    /// Ton champion (pour son icône).
    pub champion_id: i64,
    /// true si le client attribue des postes : il y a un adversaire direct à trouver.
    pub laning: bool,
    /// Ton équipe, dans l'ordre des postes (haut → support).
    pub allies: Vec<Ally>,
    /// Champions adverses déjà verrouillés.
    pub enemies: Vec<Enemy>,
    /// Adversaire direct retenu, et s'il a été choisi d'un clic (sinon deviné).
    pub opponent_id: Option<i64>,
    pub opponent_manual: bool,
    /// Le duel selon OP.GG, une fois l'adversaire connu.
    pub matchup: Option<MatchupInfo>,
    /// true si la page de runes est dans le client (false : import coupé dans les réglages, ou refusé).
    pub page_applied: bool,
    /// Sorts mis dans le client, sur D puis sur F (vide si l'app ne les a pas changés).
    pub spells_applied: Vec<String>,
    /// Pourquoi les sorts n'ont pas été changés.
    pub spells_note: Option<String>,
    /// Aide au choix, tant que ton champion n'est pas verrouillé.
    pub help: Option<PickHelp>,
    /// Ton bilan contre ton adversaire direct, d'après l'historique de l'app.
    pub duel_games: u64,
    pub duel_wins: u64,
    /// Skin choisi pour ton champion (numéro du client, ex. 103015) ; 0 si inconnu.
    pub skin_id: i64,
}

/// Avant de verrouiller : les champions qui battent ton adversaire direct,
/// et ceux qui vont le mieux avec ton partenaire (ton jungler, ou ton duo du bas).
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PickHelp {
    pub opponent: String,
    pub counters: Vec<opgg::Pick>,
    pub partner: String,
    pub synergies: Vec<opgg::Pick>,
    /// true tant qu'une réponse d'OP.GG manque encore.
    pub pending: bool,
}

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Ally {
    /// 0 = pas encore choisi.
    pub champion_id: i64,
    pub name: String,
    /// Poste donné par le client : "top", "jungle", "middle", "bottom", "utility" ou "".
    pub position: String,
    /// false = champion seulement survolé, pas encore verrouillé.
    pub locked: bool,
    pub me: bool,
}

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Enemy {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MatchupInfo {
    pub opponent: String,
    /// true si runes et build viennent de ce duel ; false = trop peu de parties, recommandations générales gardées.
    pub applied: bool,
    /// Parties et victoires de ton champion contre cet adversaire.
    pub games: u64,
    pub wins: u64,
    pub advantage: String,
    pub play_style: String,
    pub tip: String,
    pub error: Option<String>,
}

/// Accès au client partagé avec l'interface, pour afficher les icônes des champions et des objets.
#[derive(Clone)]
pub struct Assets {
    http: reqwest::Client,
    /// (adresse, mot de passe) de l'API du client ; None si le client est fermé.
    auth: Arc<Mutex<Option<(String, String)>>>,
    icons: Arc<Mutex<HashMap<u32, Vec<u8>>>>,
    /// Objets du jeu d'après le client, lus une fois.
    items: Arc<Mutex<HashMap<i64, ItemData>>>,
    item_icons: Arc<Mutex<HashMap<i64, Vec<u8>>>>,
    /// Dernières illustrations de skins lues, la plus récente à la fin.
    splashes: Arc<Mutex<Vec<Splash>>>,
    /// Chemin de l'icône de chaque rune et de chaque voie de runes, lus une fois dans le client.
    perk_paths: Arc<Mutex<HashMap<i64, String>>>,
    /// Icônes des runes et emblèmes des rangs déjà lus, par chemin.
    images: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// Pour Internet (CommunityDragon), avec la vérification normale des certificats.
    web: reqwest::Client,
}

/// Rangs dont le client a un emblème (images/ranked-emblem/emblem-<rang>.png).
const TIERS: [&str; 10] =
    ["iron", "bronze", "silver", "gold", "platinum", "emerald", "diamond", "master", "grandmaster", "challenger"];

/// Illustration d'un skin : (numéro du skin, image).
type Splash = (i64, Vec<u8>);

#[derive(Clone)]
struct ItemData {
    /// Chemin de l'icône dans le client.
    icon: String,
    /// Objet terminé (cher, ne s'améliore plus, pas des bottes).
    finished: bool,
    /// Prix total dans la boutique.
    price: u64,
}

impl Assets {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            // Le client et la partie utilisent un certificat auto-signé sur 127.0.0.1.
            // On l'accepte : ce client HTTP ne contacte QUE 127.0.0.1.
            .tls_danger_accept_invalid_certs(true)
            .no_proxy()
            .connect_timeout(Duration::from_millis(800))
            .timeout(Duration::from_secs(2))
            .build()
            .expect("impossible de créer le client HTTP");
        let web = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(6))
            .build()
            .expect("impossible de créer le client HTTP");
        Self {
            http,
            auth: Arc::default(),
            icons: Arc::default(),
            items: Arc::default(),
            item_icons: Arc::default(),
            splashes: Arc::default(),
            perk_paths: Arc::default(),
            images: Arc::default(),
            web,
        }
    }

    /// Icône d'une rune ou d'une voie de runes (PNG), lue dans le client puis gardée en mémoire.
    pub async fn perk_icon(&self, id: i64) -> Option<Vec<u8>> {
        if self.perk_paths.lock().unwrap().is_empty() {
            let mut paths = HashMap::new();
            // perks.json : une liste ; perkstyles.json : { "styles": [...] }.
            for file in ["/lol-game-data/assets/v1/perks.json", "/lol-game-data/assets/v1/perkstyles.json"] {
                let doc = self.client_file(file).await.and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
                let doc = doc.unwrap_or(Value::Null);
                for entry in doc.as_array().or(doc["styles"].as_array()).into_iter().flatten() {
                    if let (Some(id), Some(path)) = (entry["id"].as_i64(), entry["iconPath"].as_str()) {
                        paths.insert(id, path.to_string());
                    }
                }
            }
            if paths.is_empty() {
                return None;
            }
            *self.perk_paths.lock().unwrap() = paths;
        }
        let path = self.perk_paths.lock().unwrap().get(&id)?.clone();
        self.cached_client_file(&path).await
    }

    /// Emblème d'un rang (PNG 16:9, l'emblème au milieu), lu dans le client ; sinon sur CommunityDragon,
    /// qui reprend les fichiers du client. Gardé en mémoire.
    pub async fn rank_emblem(&self, tier: &str) -> Option<Vec<u8>> {
        let tier = TIERS.iter().find(|t| **t == tier)?;
        let path = format!("/fe/lol-static-assets/images/ranked-emblem/emblem-{tier}.png");
        if let Some(png) = self.cached_client_file(&path).await {
            return Some(png);
        }
        let url = format!(
            "https://raw.communitydragon.org/latest/plugins/rcp-fe-lol-static-assets/global/default/images/ranked-emblem/emblem-{tier}.png"
        );
        let resp = self.web.get(url).send().await.ok().filter(|r| r.status().is_success())?;
        let png = resp.bytes().await.ok()?.to_vec();
        self.images.lock().unwrap().insert(path, png.clone());
        Some(png)
    }

    /// Un fichier du client gardé en mémoire après la première lecture.
    async fn cached_client_file(&self, path: &str) -> Option<Vec<u8>> {
        if let Some(bytes) = self.images.lock().unwrap().get(path) {
            return Some(bytes.clone());
        }
        let bytes = self.client_file(path).await?;
        self.images.lock().unwrap().insert(path.to_string(), bytes.clone());
        Some(bytes)
    }

    /// Icône carrée d'un champion (PNG), lue dans le client puis gardée en mémoire.
    pub async fn champion_icon(&self, id: u32) -> Option<Vec<u8>> {
        if let Some(png) = self.icons.lock().unwrap().get(&id) {
            return Some(png.clone());
        }
        let (base, password) = self.auth.lock().unwrap().clone()?;
        let resp = self
            .http
            .get(format!("{base}/lol-game-data/assets/v1/champion-icons/{id}.png"))
            .basic_auth("riot", Some(password))
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let png = resp.bytes().await.ok()?.to_vec();
        self.icons.lock().unwrap().insert(id, png.clone());
        Some(png)
    }

    /// Icône d'un objet (PNG), lue dans le client puis gardée en mémoire.
    pub async fn item_icon(&self, id: i64) -> Option<Vec<u8>> {
        if let Some(png) = self.item_icons.lock().unwrap().get(&id) {
            return Some(png.clone());
        }
        if !self.load_items().await {
            return None;
        }
        let icon = self.items.lock().unwrap().get(&id)?.icon.clone();
        let (base, password) = self.auth.lock().unwrap().clone()?;
        let resp = self.http.get(format!("{base}{icon}")).basic_auth("riot", Some(password)).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let png = resp.bytes().await.ok()?.to_vec();
        self.item_icons.lock().unwrap().insert(id, png.clone());
        Some(png)
    }

    /// Lit une fois la liste des objets du client. false si elle n'est pas disponible.
    async fn load_items(&self) -> bool {
        if !self.items.lock().unwrap().is_empty() {
            return true;
        }
        let Some((base, password)) = self.auth.lock().unwrap().clone() else {
            return false;
        };
        let url = format!("{base}/lol-game-data/assets/v1/items.json");
        let Ok(resp) = self.http.get(url).basic_auth("riot", Some(password)).send().await else {
            return false;
        };
        let Ok(Value::Array(list)) = resp.json::<Value>().await else {
            return false;
        };
        let items: HashMap<i64, ItemData> = list
            .iter()
            .filter_map(|it| {
                let boots = it["categories"].as_array().is_some_and(|c| c.iter().any(|x| x == "Boots"));
                let last = it["to"].as_array().is_none_or(Vec::is_empty);
                let price = it["priceTotal"].as_u64().unwrap_or(0);
                let finished = !boots && last && price >= FINISHED_PRICE;
                Some((it["id"].as_i64()?, ItemData { icon: it["iconPath"].as_str()?.to_string(), finished, price }))
            })
            .collect();
        let loaded = !items.is_empty();
        *self.items.lock().unwrap() = items;
        loaded
    }

    /// Nombre d'objets terminés parmi ceux-ci (0 tant que la liste des objets n'est pas lue).
    fn finished_count(&self, owned: &[i64]) -> usize {
        let items = self.items.lock().unwrap();
        owned.iter().filter(|id| items.get(id).is_some_and(|i| i.finished)).count()
    }

    /// Valeur d'un inventaire au prix de la boutique : (objet, nombre) ; None tant que les prix ne sont pas lus.
    fn items_value(&self, owned: &[(i64, u64)]) -> Option<u64> {
        let items = self.items.lock().unwrap();
        if items.is_empty() {
            return None;
        }
        Some(owned.iter().map(|(id, count)| items.get(id).map_or(0, |i| i.price) * count).sum())
    }

    /// Illustration d'un skin (ex. 103015), lue dans le client puis gardée en mémoire.
    /// Le chemin de l'image est dans la fiche du champion ; pour un chroma, c'est celle de son skin.
    pub async fn splash(&self, skin_id: i64) -> Option<Vec<u8>> {
        if let Some((_, image)) = self.splashes.lock().unwrap().iter().find(|(id, _)| *id == skin_id) {
            return Some(image.clone());
        }
        let champion = skin_id / 1000;
        let doc = self.client_file(&format!("/lol-game-data/assets/v1/champions/{champion}.json")).await;
        let doc = doc.and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        let mut paths = doc.map_or_else(Vec::new, |doc| splash_paths(&doc, skin_id));
        // Ancien emplacement des illustrations, au cas où la fiche ne serait pas lisible.
        paths.push(format!("/lol-game-data/assets/v1/champion-splashes/{champion}/{skin_id}.jpg"));
        for path in paths {
            if let Some(image) = self.client_file(&path).await {
                let mut cache = self.splashes.lock().unwrap();
                cache.push((skin_id, image.clone()));
                if cache.len() > SPLASH_CACHE {
                    cache.remove(0);
                }
                return Some(image);
            }
        }
        None
    }

    /// Un fichier du client (`path` commence par « / ») ; None si le client est fermé ou ne l'a pas.
    async fn client_file(&self, path: &str) -> Option<Vec<u8>> {
        let (base, password) = self.auth.lock().unwrap().clone()?;
        let resp = self.http.get(format!("{base}{path}")).basic_auth("riot", Some(password)).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        Some(resp.bytes().await.ok()?.to_vec())
    }
}

/// Chemins possibles de l'illustration d'un skin, d'après la fiche du champion (le premier qui existe gagne) :
/// le skin lui-même, ou celui dont c'est un chroma, ou le palier d'un skin à paliers ; sinon le skin de base.
fn splash_paths(champion: &Value, skin_id: i64) -> Vec<String> {
    let skins: Vec<&Value> = champion["skins"].as_array().into_iter().flatten().collect();
    let has = |list: &Value| list.as_array().is_some_and(|l| l.iter().any(|x| x["id"] == skin_id));
    let tier = skins
        .iter()
        .flat_map(|s| s["questSkinInfo"]["tiers"].as_array().into_iter().flatten())
        .find(|t| t["id"] == skin_id);
    let skin = skins
        .iter()
        .find(|s| s["id"] == skin_id || has(&s["chromas"]) || has(&s["questSkinInfo"]["tiers"]));
    let base = skins.iter().find(|s| s["isBase"] == true).or(skins.first());
    let mut paths = Vec::new();
    for entry in [tier, skin.copied(), base.copied()].into_iter().flatten() {
        for key in ["splashPath", "uncenteredSplashPath"] {
            let new = |path: &&str| path.starts_with('/') && !paths.iter().any(|known| known == path);
            if let Some(path) = entry[key].as_str().filter(new) {
                paths.push(path.to_string());
            }
        }
    }
    paths
}

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    /// false tant que la partie charge (écran de chargement) : seuls les champions et les rangs sont connus.
    pub ready: bool,
    /// Temps de jeu en secondes.
    pub game_time: f64,
    pub champion: String,
    pub level: u64,
    pub kills: u64,
    pub deaths: u64,
    pub assists: u64,
    pub cs: u64,
    pub gold: u64,
    pub items: Vec<String>,
    /// Ton poste : "TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY" ; vide ou "NONE" dans les modes sans postes.
    pub position: String,
    /// Ton skin (numéro du client, ex. 267002) ; 0 si inconnu.
    pub skin_id: i64,
    /// Les 10 champions : ton équipe et l'équipe adverse, chacune triée par poste.
    pub allies: Vec<Player>,
    pub enemies: Vec<Player>,
    /// Objets à acheter, pour le panneau posé sur le jeu : ceux du duel contre ton adversaire de couloir.
    pub build: Option<opgg::Build>,
    /// true une fois tes deux premiers objets faits : le panneau se range.
    pub build_done: bool,
}

/// Un champion de la partie, tel que le tableau des scores (Tab) le montre.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub champion: String,
    /// Pour son icône ; 0 si inconnu.
    pub champion_id: i64,
    pub level: u64,
    pub kills: u64,
    pub deaths: u64,
    pub assists: u64,
    /// true = toi.
    pub me: bool,
    /// Rang dans la file de la partie (aucun pour un joueur en mode streamer).
    pub rank: Option<Rank>,
    /// true si le client cache ce joueur (mode streamer) : pas de rang à montrer.
    pub rank_hidden: bool,
    /// Poste : "TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY" ; vide ou "NONE" dans les modes sans postes.
    pub position: String,
    /// Valeur de ses objets au prix de la boutique ; None tant que les prix ne sont pas lus.
    pub items_value: Option<u64>,
}

/// La partie en cours : ses joueurs et leurs rangs (connus dès l'écran de chargement),
/// et de quoi l'enregistrer dans l'historique à la fin.
#[derive(Clone, Default)]
struct CurrentGame {
    game_id: i64,
    /// Ton champion, ton poste (au sens d'OP.GG) et ton adversaire direct.
    champion_id: i64,
    position: String,
    opponent_id: Option<i64>,
    /// Mode de jeu au sens d'OP.GG ("ranked", "aram"...).
    mode: String,
    allies: Vec<Player>,
    enemies: Vec<Player>,
    /// Rang de chaque joueur, par champion.
    ranks: HashMap<i64, Rank>,
    /// Champions des joueurs que le client donne (les autres sont en mode streamer).
    listed: HashSet<i64>,
    /// Ton skin, pour l'historique.
    skin_id: i64,
    /// Objets du panneau, et pour quels (champion, poste, adversaire) ils ont été cherchés.
    build: Option<opgg::Build>,
    build_for: Option<(i64, String, Option<i64>)>,
}

/// Partie finie, en attente de ses stats dans l'historique du client.
struct Finished {
    game: CurrentGame,
    next_try: Instant,
    give_up: Instant,
}

/// Lit l'état du client et de la partie, en gardant ce qui ne change pas souvent.
pub struct Tracker {
    http: reqwest::Client,
    assets: Assets,
    opgg: opgg::Client,
    /// Réglages du moment (relus à chaque tour).
    settings: Settings,
    history: history::History,
    puuid: Option<String>,
    summoner_id: Option<u64>,
    profile: Option<Profile>,
    /// Profil à relire (connexion au client, partie enregistrée).
    profile_due: bool,
    /// Contres et partenaires d'OP.GG par (champion, poste, mode).
    help_cache: HashMap<(i64, String, String), opgg::Help>,
    /// Sorts mis par l'app pendant cette sélection : s'ils ont changé depuis, c'est toi qui les as changés.
    spells_set: Option<[i64; 2]>,
    /// Ton champion, ton poste et ton adversaire à la fin de la sélection, pour l'historique.
    last_pick: Option<(i64, String, Option<i64>)>,
    game: Option<CurrentGame>,
    finished: Option<Finished>,
    /// Contenu du lockfile utilisé : s'il change, le client a redémarré.
    lockfile: String,
    /// (adresse, mot de passe) de l'API du client, tirés du lockfile.
    lcu: Option<(String, String)>,
    last_phase: Option<String>,
    riot_id: Option<String>,
    summoner_level: Option<u64>,
    /// Import fait pendant cette sélection : (champion, poste, mode, adversaire) et son résultat.
    rune_import: Option<(ImportKey, RuneImport)>,
    /// Réponses OP.GG déjà récupérées, pour ne pas redemander à chaque partie.
    cache: HashMap<(i64, String, String), opgg::Analysis>,
    matchups: HashMap<(i64, i64, String), opgg::Matchup>,
    /// Part des parties d'un champion à un poste, selon OP.GG.
    role_rates: HashMap<(i64, String), f64>,
    /// Noms des champions, lus une fois dans le client.
    champs: HashMap<i64, opgg::Champ>,
    /// Numéro de chaque champion d'après son nom interne en minuscules (« monkeyking » → 62).
    champion_ids: HashMap<String, i64>,
    /// Noms des sorts d'invocateur, lus une fois dans le client.
    spell_names: HashMap<i64, String>,
    /// Dernière ligne écrite dans le terminal par sujet, pour ne pas la répéter.
    last_log: HashMap<&'static str, String>,
}

/// Ce qui déclenche un nouvel import : (champion, poste, mode, adversaire direct).
type ImportKey = (i64, String, String, Option<i64>);

impl Tracker {
    /// `data_dir` : dossier de l'historique.
    pub fn new(assets: Assets, data_dir: PathBuf) -> Self {
        Self {
            http: assets.http.clone(),
            assets,
            opgg: opgg::Client::new(),
            settings: Settings::default(),
            history: history::History::load(data_dir),
            puuid: None,
            summoner_id: None,
            profile: None,
            profile_due: true,
            help_cache: HashMap::new(),
            spells_set: None,
            last_pick: None,
            game: None,
            finished: None,
            lockfile: String::new(),
            lcu: None,
            last_phase: None,
            riot_id: None,
            summoner_level: None,
            rune_import: None,
            cache: HashMap::new(),
            matchups: HashMap::new(),
            role_rates: HashMap::new(),
            champs: HashMap::new(),
            champion_ids: HashMap::new(),
            spell_names: HashMap::new(),
            last_log: HashMap::new(),
        }
    }

    /// Délai avant la lecture suivante : rapide en sélection et en partie, lent quand rien ne tourne.
    pub fn next_delay(snap: &Snapshot) -> Duration {
        if snap.game.is_some() || snap.runes.is_some() {
            Duration::from_secs(1)
        } else if snap.client_found {
            Duration::from_secs(2)
        } else {
            Duration::from_secs(3)
        }
    }

    /// `manual` = adversaire direct choisi d'un clic dans l'interface.
    pub async fn poll(&mut self, manual: Option<i64>, settings: &Settings) -> Snapshot {
        self.settings = settings.clone();
        self.refresh_lockfile();
        // Client fermé (ou en train de démarrer) : l'historique de l'app reste consultable.
        let offline = Snapshot { history: self.history.overview(), ..Default::default() };
        let Some((base, password)) = self.lcu.clone() else {
            return offline;
        };
        let phase = match self.get(&format!("{base}/lol-gameflow/v1/gameflow-phase"), Some(&password)).await {
            Some(Value::String(phase)) => phase,
            _ => return offline,
        };

        let phase_changed = self.last_phase.as_deref() != Some(phase.as_str());
        if phase_changed {
            eprintln!("[client] phase : {phase}");
        }
        // Compte : relu quand la phase change (ex. niveau gagné en fin de partie).
        let me = if self.riot_id.is_none() || phase_changed {
            self.get(&format!("{base}/lol-summoner/v1/current-summoner"), Some(&password)).await
        } else {
            None
        };
        if let Some(me) = me {
            let name = me["gameName"].as_str().unwrap_or_default();
            let tag = me["tagLine"].as_str().unwrap_or_default();
            if !name.is_empty() {
                self.riot_id = Some(format!("{name}#{tag}"));
                self.summoner_level = me["summonerLevel"].as_u64();
                self.puuid = me["puuid"].as_str().map(str::to_string);
                self.summoner_id = me["summonerId"].as_u64();
            }
        }
        // Rang et dernières parties : relus à la connexion, au retour à l'accueil ou au salon,
        // et quand une partie vient d'être enregistrée. En cas d'échec, on garde l'ancien profil.
        if self.profile_due || (phase_changed && matches!(phase.as_str(), "None" | "Lobby")) {
            self.profile_due = false;
            if let Some(profile) = self.read_profile(&base, &password).await {
                self.profile = Some(profile);
            }
        }
        if phase_changed && phase == "ChampSelect" {
            self.last_pick = None;
        }
        self.last_phase = Some(phase.clone());

        let runes = if phase == "ChampSelect" {
            Some(self.champ_select_runes(&base, &password, manual).await)
        } else {
            self.rune_import = None;
            self.spells_set = None;
            None
        };

        let in_game = matches!(phase.as_str(), "GameStart" | "InProgress" | "Reconnect");
        let game = if in_game { Some(self.live_game(&base, &password).await) } else { None };
        // Fin de partie : on va chercher ses stats exactes dans l'historique du client.
        if let Some(game) = self.game.take_if(|_| !in_game) {
            let now = Instant::now();
            self.finished = Some(Finished { game, next_try: now, give_up: now + RECORD_TIMEOUT });
        }
        self.record_finished(&base, &password).await;

        Snapshot {
            client_found: true,
            riot_id: self.riot_id.clone(),
            summoner_level: self.summoner_level,
            phase: Some(phase),
            profile: self.profile.clone(),
            runes,
            game,
            history: self.history.overview(),
            recording: self.finished.is_some(),
            update: None,
        }
    }

    /// En sélection : dès que ton champion est verrouillé, importe runes et build OP.GG,
    /// puis ceux du duel contre ton adversaire direct dès qu'il est connu.
    async fn champ_select_runes(&mut self, base: &str, password: &str, manual: Option<i64>) -> RuneImport {
        let session = self.get(&format!("{base}/lol-champ-select/v1/session"), Some(password)).await.unwrap_or(Value::Null);
        // Ce que le client dit de l'équipe d'en face.
        let their_team = session["theirTeam"].as_array().cloned().unwrap_or_default();
        let ids: Vec<&Value> = their_team.iter().map(|p| &p["championId"]).collect();
        let positions: Vec<&Value> = their_team.iter().map(|p| &p["assignedPosition"]).collect();
        self.log("adversaires", format!("[adversaires] champions {}, postes {}", json!(ids), json!(positions)));

        // Ton poste (attribué par le client) et le mode de jeu.
        let assigned = my_cell(&session)
            .and_then(|p| p["assignedPosition"].as_str())
            .unwrap_or_default()
            .to_string();
        let flow = self.get(&format!("{base}/lol-gameflow/v1/session"), Some(password)).await.unwrap_or(Value::Null);
        let queue = &flow["gameData"]["queue"];
        let mode = game_mode(queue);
        let position = lane(&assigned, mode);
        // Carte de la partie (11 = Faille de l'invocateur, 12 = Abîme hurlant), pour le set d'objets.
        let map_id = flow["map"]["id"].as_i64().unwrap_or(0);

        // Les deux équipes, affichées dès le début de la sélection.
        let allies = self.allies(base, password, &session).await;
        let mut enemies: Vec<opgg::Champ> = Vec::new();
        for id in their_team.iter().filter_map(|p| p["championId"].as_i64()).filter(|id| *id > 0) {
            enemies.push(self.champ(base, password, id).await);
        }
        // Adversaire direct : seulement quand le client attribue des postes (classée, normale avec postes...).
        let laning = !assigned.is_empty() && !position.is_empty();
        let (opponent, opponent_manual) = match manual.and_then(|id| enemies.iter().find(|e| e.id == id)) {
            _ if !laning => (None, false),
            Some(chosen) => (Some(chosen.clone()), true),
            None => (self.guess_opponent(&enemies, position, mode).await, false),
        };

        let current = self.get(&format!("{base}/lol-champ-select/v1/current-champion"), Some(password)).await;
        let champion_id = locked_champion(current.as_ref(), &session);
        let mut result = if champion_id <= 0 {
            self.log("runes", format!(
                "[runes] en attente du verrouillage (current-champion = {}, session lue = {})",
                current.map(|v| v.to_string()).unwrap_or_else(|| "erreur/404".into()),
                !session.is_null()
            ));
            RuneImport::default()
        } else {
            self.log("runes", format!(
                "[runes] champion {champion_id}, poste client \"{assigned}\", file {}/{}, carte {map_id} -> OP.GG poste \"{position}\", mode \"{mode}\"",
                queue["id"], queue["gameMode"]
            ));
            self.locked_import(base, password, champion_id, position, mode, map_id, opponent.as_ref()).await
        };

        if laning && champion_id <= 0 {
            // Pas encore verrouillé : qui battrait ton adversaire, qui irait bien avec ton partenaire.
            let partner = partner_position(position).and_then(|(client_position, opgg_position)| {
                let ally = allies.iter().find(|a| !a.me && a.position == client_position && a.champion_id > 0)?;
                Some((ally.champion_id, opgg_position))
            });
            let taken = taken_champions(&session, &enemies);
            result.help = self.pick_help(base, password, position, mode, opponent.as_ref(), partner, &taken).await;
        }
        if champion_id > 0 {
            let opponent_id = opponent.as_ref().map(|o| o.id);
            if let Some(id) = opponent_id {
                (result.duel_games, result.duel_wins) = self.history.versus(champion_id, id);
            }
            self.last_pick = Some((champion_id, position.to_string(), opponent_id));
        }
        result.laning = laning;
        result.skin_id = my_cell(&session).and_then(|p| p["selectedSkinId"].as_i64()).unwrap_or(0);
        result.allies = allies;
        result.enemies = enemies.iter().map(|e| Enemy { id: e.id, name: e.name.clone() }).collect();
        result.opponent_id = opponent.map(|o| o.id);
        result.opponent_manual = opponent_manual;
        result
    }

    /// Une seule question à OP.GG par tour (la suivante au tour d'après), pour que l'interface reste réactive.
    #[allow(clippy::too_many_arguments)]
    async fn pick_help(
        &mut self,
        base: &str,
        password: &str,
        position: &str,
        mode: &str,
        opponent: Option<&opgg::Champ>,
        partner: Option<(i64, &str)>,
        taken: &[i64],
    ) -> Option<PickHelp> {
        if opponent.is_none() && partner.is_none() {
            return None;
        }
        // Les 3 meilleurs, sans les champions déjà pris ou bannis.
        let free = |list: &[opgg::Pick]| -> Vec<opgg::Pick> {
            list.iter().filter(|p| !taken.contains(&p.champion_id)).take(3).cloned().collect()
        };
        let mut help = PickHelp::default();
        let mut asked = false;
        if let Some(opponent) = opponent {
            help.opponent = opponent.name.clone();
            match self.help_of(opponent, position, mode, &mut asked).await {
                Some(found) => help.counters = free(&found.counters),
                None => help.pending = true,
            }
        }
        if let Some((id, partner_position)) = partner {
            let partner = self.champ(base, password, id).await;
            help.partner = partner.name.clone();
            match self.help_of(&partner, partner_position, mode, &mut asked).await {
                Some(found) => help.synergies = free(found.synergies.get(position).map(Vec::as_slice).unwrap_or_default()),
                None => help.pending = true,
            }
        }
        Some(help)
    }

    /// Contres et partenaires d'OP.GG, gardés en mémoire. None : question remise au tour suivant (`asked`).
    async fn help_of(&mut self, champ: &opgg::Champ, position: &str, mode: &str, asked: &mut bool) -> Option<opgg::Help> {
        let key = (champ.id, position.to_string(), mode.to_string());
        if let Some(found) = self.help_cache.get(&key) {
            return Some(found.clone());
        }
        if *asked {
            return None;
        }
        *asked = true;
        // En cas d'échec, on garde une aide vide pour ne pas redemander à chaque tour.
        let found = match self.opgg.help(champ, position, mode).await {
            Ok(found) => {
                eprintln!("[aide] {} {position} : {} contres, {} postes de partenaires", champ.name, found.counters.len(), found.synergies.len());
                found
            }
            Err(e) => {
                eprintln!("[aide] {} {position} : {e}", champ.name);
                opgg::Help::default()
            }
        };
        self.help_cache.insert(key, found.clone());
        Some(found)
    }

    /// Ton champion est verrouillé : import des runes et du build (une fois par champion, poste, mode et adversaire).
    #[allow(clippy::too_many_arguments)]
    async fn locked_import(
        &mut self,
        base: &str,
        password: &str,
        champion_id: i64,
        position: &str,
        mode: &str,
        map_id: i64,
        opponent: Option<&opgg::Champ>,
    ) -> RuneImport {
        let me = self.champ(base, password, champion_id).await;
        let key: ImportKey = (champion_id, position.to_string(), mode.to_string(), opponent.as_ref().map(|o| o.id));
        match self.rune_import.clone() {
            // Déjà fait pour ce champion, ce poste, ce mode et cet adversaire.
            Some((done, result)) if done == key && !result.pending => result,
            // Recherche annoncée au tour précédent : on la fait maintenant.
            Some((done, _)) if done == key => {
                let result = self.import(base, password, &me, position, mode, map_id, opponent).await;
                self.rune_import = Some((key, result.clone()));
                result
            }
            // Nouveau champion ou nouvel adversaire : « Recherche… » d'abord, l'import suit au tour suivant.
            // Pour le même champion, on garde l'affichage précédent en attendant.
            previous => {
                let mut pending = previous.map(|(_, r)| r).filter(|r| r.champion_id == champion_id).unwrap_or_default();
                pending.champion = me.name.clone();
                pending.champion_id = champion_id;
                pending.position = position.to_string();
                pending.pending = true;
                pending.matchup = None;
                self.rune_import = Some((key, pending.clone()));
                pending
            }
        }
    }

    /// Ton équipe : champion verrouillé ou seulement survolé, dans l'ordre des postes.
    async fn allies(&mut self, base: &str, password: &str, session: &Value) -> Vec<Ally> {
        let picks: Vec<&Value> = session["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_array)
            .flatten()
            .filter(|a| a["type"] == "pick")
            .collect();
        let mut allies = Vec::new();
        for p in session["myTeam"].as_array().into_iter().flatten() {
            let picked = p["championId"].as_i64().unwrap_or(0);
            let intent = p["championPickIntent"].as_i64().unwrap_or(0);
            let id = if picked > 0 { picked } else { intent.max(0) };
            // Verrouillé : son action « choix » est terminée (sans actions de choix, comme en ARAM : champion attribué).
            let locked = id > 0
                && if picks.is_empty() {
                    picked > 0
                } else {
                    picks.iter().any(|a| a["actorCellId"] == p["cellId"] && a["completed"] == true)
                };
            let name = if id > 0 { self.champ(base, password, id).await.name } else { String::new() };
            allies.push(Ally {
                champion_id: id,
                name,
                position: p["assignedPosition"].as_str().unwrap_or_default().to_string(),
                locked,
                me: p["cellId"] == session["localPlayerCellId"],
            });
        }
        let lane_order = |ally: &Ally| {
            ["top", "jungle", "middle", "bottom", "utility"].iter().position(|l| *l == ally.position).unwrap_or(5)
        };
        allies.sort_by_key(lane_order);
        allies
    }

    /// Adversaire direct deviné : le champion adverse qui joue le plus souvent à ton poste, selon OP.GG.
    /// Une seule question à OP.GG par tour, pour que l'interface reste réactive.
    async fn guess_opponent(&mut self, enemies: &[opgg::Champ], position: &str, mode: &str) -> Option<opgg::Champ> {
        let unknown = enemies.iter().find(|e| !self.role_rates.contains_key(&(e.id, position.to_string())));
        if let Some(enemy) = unknown.cloned() {
            let rate = match self.opgg.positions(&enemy, position, mode).await {
                Ok(list) => list.iter().find(|(p, _)| p == position).map_or(0.0, |(_, rate)| *rate),
                Err(e) => {
                    eprintln!("[duel] postes de {} introuvables : {e}", enemy.name);
                    0.0
                }
            };
            eprintln!("[duel] {} : {:.0} % de ses parties au poste {position}", enemy.name, rate * 100.0);
            self.role_rates.insert((enemy.id, position.to_string()), rate);
        }
        enemies
            .iter()
            .filter_map(|e| Some((e, *self.role_rates.get(&(e.id, position.to_string()))?)))
            .filter(|(_, rate)| *rate >= MIN_ROLE_RATE)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(e, _)| e.clone())
    }

    /// Runes, build et set d'objets : ceux du duel si l'adversaire est connu (et les parties assez nombreuses),
    /// sinon les recommandations générales du champion.
    #[allow(clippy::too_many_arguments)]
    async fn import(
        &mut self,
        base: &str,
        password: &str,
        me: &opgg::Champ,
        position: &str,
        mode: &str,
        map_id: i64,
        opponent: Option<&opgg::Champ>,
    ) -> RuneImport {
        let mut result = RuneImport {
            champion: me.name.clone(),
            champion_id: me.id,
            position: position.to_string(),
            ..Default::default()
        };
        let mut chosen = match self.analysis(me, position, mode).await {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[runes] échec : {e}");
                result.error = Some(e);
                return result;
            }
        };
        let mut versus = String::new();
        if let Some(opponent) = opponent {
            let mut info = MatchupInfo { opponent: opponent.name.clone(), ..Default::default() };
            match self.matchup(me, opponent, position).await {
                Ok(m) => {
                    info.applied = m.analysis.runes.games >= MIN_MATCHUP_GAMES;
                    eprintln!(
                        "[duel] {} vs {} : page la plus jouée sur {} parties{}",
                        me.name,
                        opponent.name,
                        m.analysis.runes.games,
                        if info.applied { "" } else { " (trop peu : runes générales gardées)" }
                    );
                    info.games = m.games;
                    info.wins = m.wins;
                    info.advantage = m.advantage;
                    info.play_style = m.play_style;
                    info.tip = m.tip;
                    if info.applied {
                        chosen = m.analysis;
                        versus = format!(" vs {}", opponent.name);
                    }
                }
                Err(e) => {
                    eprintln!("[duel] échec : {e}");
                    info.error = Some(e);
                }
            }
            result.matchup = Some(info);
        }

        // Runes affichées dans tous les cas ; mises dans le client seulement si l'import est activé.
        let runes = &chosen.runes;
        result.keystone = runes.keystone.clone();
        result.primary = runes.primary_name.clone();
        result.secondary = runes.secondary_name.clone();
        result.keystone_id = runes.perks.first().copied().unwrap_or(0);
        result.primary_id = runes.primary_style;
        result.secondary_id = runes.sub_style;
        result.games = runes.games;
        result.wins = runes.wins;
        if self.settings.import_runes {
            match self.apply_page(base, password, runes).await {
                Ok(()) => {
                    eprintln!("[runes] page appliquée{versus} : {} ({} / {})", runes.keystone, runes.primary_name, runes.secondary_name);
                    result.page_applied = true;
                }
                Err(e) => {
                    eprintln!("[runes] échec : {e}");
                    result.error = Some(e);
                }
            }
        }

        // Le build s'affiche et part dans le client même si la page de runes a été refusée.
        let build = with_atlas(chosen.build, position);
        if !build.is_empty() {
            result.spells = self.spell_names(base, password, &build.spells).await;
            if self.settings.import_spells {
                match self.apply_spells(base, password, &build.spells).await {
                    Ok(spells) => {
                        result.spells_applied = self.spell_names(base, password, &spells).await;
                        eprintln!("[sorts] D = {}, F = {}", spells[0], spells[1]);
                    }
                    Err(note) => {
                        eprintln!("[sorts] {note}");
                        result.spells_note = Some(note);
                    }
                }
            }
            if self.settings.import_item_set {
                let title = format!("{PAGE_NAME} · {}{versus}", me.name);
                match self.apply_item_set(base, password, me.id, &title, map_id, &build).await {
                    Ok(()) => {
                        eprintln!("[objets] set « {title} » importé");
                        result.item_set_ok = true;
                    }
                    Err(e) => {
                        eprintln!("[objets] échec : {e}");
                        result.item_set_error = Some(e);
                    }
                }
            }
            result.build = Some(build);
        }
        result
    }

    /// Met les sorts conseillés sur D et F, le Saut éclair sur la touche choisie dans les réglages.
    /// Si tes sorts ont changé depuis le dernier envoi de l'app, c'est toi : on n'y touche plus.
    async fn apply_spells(&mut self, base: &str, password: &str, wanted: &[i64]) -> Result<[i64; 2], String> {
        let &[a, b] = wanted else {
            return Err("Sorts OP.GG incomplets : tes sorts n'ont pas été changés.".to_string());
        };
        let session = self.get(&format!("{base}/lol-champ-select/v1/session"), Some(password)).await.unwrap_or(Value::Null);
        let cell = my_cell(&session).ok_or("Sélection illisible : tes sorts n'ont pas été changés.")?;
        let current = [cell["spell1Id"].as_i64().unwrap_or(0), cell["spell2Id"].as_i64().unwrap_or(0)];
        if self.spells_set.is_some_and(|set| set != current) {
            return Err("Sorts gardés : tu les as changés à la main.".to_string());
        }
        let spells = spell_slots([a, b], current, &self.settings.flash_key);
        if spells != current {
            let body = json!({ "spell1Id": spells[0], "spell2Id": spells[1] });
            let url = format!("{base}/lol-champ-select/v1/session/my-selection");
            self.send(Method::PATCH, &url, Some(password), Some(&body))
                .await
                .map_err(|e| format!("Le client a refusé les sorts ({e})."))?;
        }
        self.spells_set = Some(spells);
        Ok(spells)
    }

    /// Affiche une ligne dans le terminal (lancement par `cargo run`), sans répéter la même.
    fn log(&mut self, topic: &'static str, line: String) {
        if self.last_log.get(topic) != Some(&line) {
            eprintln!("{line}");
            self.last_log.insert(topic, line);
        }
    }

    /// Nom et nom interne d'un champion, lus une fois dans le client.
    async fn champ(&mut self, base: &str, password: &str, id: i64) -> opgg::Champ {
        if let Some(champ) = self.champs.get(&id) {
            return champ.clone();
        }
        let url = format!("{base}/lol-game-data/assets/v1/champions/{id}.json");
        let Some(info) = self.get(&url, Some(password)).await else {
            // Pas gardé en mémoire : on réessaiera au tour suivant.
            return opgg::Champ { id, name: id.to_string(), alias: String::new() };
        };
        let champ = opgg::Champ {
            id,
            name: info["name"].as_str().map_or_else(|| id.to_string(), str::to_string),
            alias: info["alias"].as_str().unwrap_or_default().to_string(),
        };
        self.champs.insert(id, champ.clone());
        champ
    }

    /// Runes et build OP.GG pour (champion, poste, mode), gardés en mémoire.
    async fn analysis(&mut self, me: &opgg::Champ, position: &str, mode: &str) -> Result<opgg::Analysis, String> {
        if position.is_empty() {
            return Err("Pas de poste attribué dans ce mode : import ignoré.".to_string());
        }
        let key = (me.id, position.to_string(), mode.to_string());
        if let Some(a) = self.cache.get(&key) {
            return Ok(a.clone());
        }
        let a = self.opgg.find(me, position, mode).await?;
        self.cache.insert(key, a.clone());
        Ok(a)
    }

    /// Duel OP.GG contre cet adversaire, gardé en mémoire.
    async fn matchup(&mut self, me: &opgg::Champ, opponent: &opgg::Champ, position: &str) -> Result<opgg::Matchup, String> {
        let key = (me.id, opponent.id, position.to_string());
        if let Some(m) = self.matchups.get(&key) {
            return Ok(m.clone());
        }
        let m = self.opgg.matchup(me, opponent, position).await?;
        self.matchups.insert(key, m.clone());
        Ok(m)
    }

    /// Noms des sorts d'invocateur (« Saut éclair »...), tels que le client les affiche.
    async fn spell_names(&mut self, base: &str, password: &str, ids: &[i64]) -> Vec<String> {
        if self.spell_names.is_empty() {
            let url = format!("{base}/lol-game-data/assets/v1/summoner-spells.json");
            if let Some(Value::Array(list)) = self.get(&url, Some(password)).await {
                self.spell_names = list
                    .iter()
                    .filter_map(|s| Some((s["id"].as_i64()?, s["name"].as_str()?.to_string())))
                    .collect();
            }
        }
        ids.iter().filter_map(|id| self.spell_names.get(id).cloned()).collect()
    }

    /// Écrit le set d'objets « LoL Companion · … » à la place de l'ancien : le seul de l'app,
    /// ou celui de ce champion si les réglages demandent un set par champion.
    /// Le client n'accepte que la liste complète des sets du compte : on la relit juste avant,
    /// on garde tels quels tous les autres sets, et on n'écrit rien si la liste est illisible.
    async fn apply_item_set(
        &self,
        base: &str,
        password: &str,
        champion_id: i64,
        title: &str,
        map_id: i64,
        build: &opgg::Build,
    ) -> Result<(), String> {
        let me = self.get(&format!("{base}/lol-summoner/v1/current-summoner"), Some(password)).await;
        let Some(summoner_id) = me.as_ref().and_then(|m| m["summonerId"].as_u64()) else {
            return Err("Compte introuvable : set d'objets non importé.".to_string());
        };
        let url = format!("{base}/lol-item-sets/v1/item-sets/{summoner_id}/sets");
        let unreadable = |detail: String| format!("Liste de tes sets d'objets illisible ({detail}) : rien n'a été modifié.");
        let mut list = self.send(Method::GET, &url, Some(password), None).await.map_err(unreadable)?;
        let Some(sets) = list["itemSets"].as_array().cloned() else {
            return Err(unreadable("format inattendu".to_string()));
        };

        let per_champion = self.settings.item_set_per_champion;
        let is_ours = |set: &Value| {
            set["title"].as_str().is_some_and(|t| t.starts_with(PAGE_NAME))
                && (!per_champion || set["associatedChampions"] == json!([champion_id]))
        };
        let theirs: Vec<Value> = sets.iter().filter(|s| !is_ours(s)).map(|s| s["uid"].clone()).collect();
        let mut mine = Some(item_set(champion_id, title, map_id, build));
        let mut updated: Vec<Value> = Vec::new();
        for set in sets {
            if !is_ours(&set) {
                updated.push(set);
            } else if let Some(mut new_set) = mine.take() {
                // Ancien set de l'app : remplacé à la même place, avec le même identifiant.
                for key in ["uid", "sortrank"] {
                    if !set[key].is_null() {
                        new_set[key] = set[key].clone();
                    }
                }
                updated.push(new_set);
            } // Sinon c'est un doublon de l'app : retiré.
        }
        updated.extend(mine);
        list["itemSets"] = Value::Array(updated);

        self.send(Method::PUT, &url, Some(password), Some(&list))
            .await
            .map_err(|e| format!("Le client a refusé le set d'objets ({e})."))?;

        // Vérification : tous tes sets doivent toujours être là.
        let after = self.get(&url, Some(password)).await.unwrap_or(Value::Null);
        let Some(after_sets) = after["itemSets"].as_array() else {
            eprintln!("[objets] vérification impossible : liste relue illisible");
            return Ok(());
        };
        let missing = theirs.iter().filter(|uid| !after_sets.iter().any(|s| s["uid"] == **uid)).count();
        if missing > 0 {
            let which = if missing == 1 {
                "un de tes sets d'objets n'apparaît plus".to_string()
            } else {
                format!("{missing} de tes sets d'objets n'apparaissent plus")
            };
            return Err(format!("Set importé, mais {which} : vérifie ta Collection > Objets."));
        }
        Ok(())
    }

    /// Remplace la page « LoL Companion » (ou en crée une) et la rend active.
    async fn apply_page(&self, base: &str, password: &str, runes: &opgg::Runes) -> Result<(), String> {
        let pages = self.get(&format!("{base}/lol-perks/v1/pages"), Some(password)).await.unwrap_or(Value::Null);
        let old = pages.as_array().and_then(|list| list.iter().find(|p| p["name"] == PAGE_NAME));
        if let Some(id) = old.and_then(|p| p["id"].as_i64()) {
            let _ = self.send(Method::DELETE, &format!("{base}/lol-perks/v1/pages/{id}"), Some(password), None).await;
        }
        let page = json!({
            "name": PAGE_NAME,
            "primaryStyleId": runes.primary_style,
            "subStyleId": runes.sub_style,
            "selectedPerkIds": runes.perks,
            "current": true,
        });
        self.send(Method::POST, &format!("{base}/lol-perks/v1/pages"), Some(password), Some(&page))
            .await
            .map(|_| ())
            .map_err(|e| {
                format!("Le client a refusé la page ({e}). Si toutes tes pages sont prises, renomme-en une « {PAGE_NAME} ».")
            })
    }

    /// Tes stats dans la partie en cours, via la Live Client Data API.
    /// Pendant l'écran de chargement : les champions et les rangs, d'après le client.
    async fn live_game(&mut self, base: &str, password: &str) -> Game {
        if self.champion_ids.is_empty() {
            let url = format!("{base}/lol-game-data/assets/v1/champion-summary.json");
            if let Some(Value::Array(list)) = self.get(&url, Some(password)).await {
                self.champion_ids = list
                    .iter()
                    .filter_map(|c| Some((c["alias"].as_str()?.to_lowercase(), c["id"].as_i64()?)))
                    .filter(|(_, id)| *id > 0)
                    .collect();
            }
        }
        if self.game.is_none() {
            self.game = self.start_game(base, password).await;
        }
        // Prix des objets, pour la valeur des inventaires (lus une fois).
        self.assets.load_items().await;
        let loading = match &self.game {
            Some(g) => Game { allies: g.allies.clone(), enemies: g.enemies.clone(), ..Default::default() },
            None => Game::default(),
        };
        let Some(me) = self.get(&format!("{LIVE_CLIENT}/activeplayer"), None).await else {
            return loading;
        };
        let Some(Value::Array(players)) = self.get(&format!("{LIVE_CLIENT}/playerlist"), None).await else {
            return loading;
        };
        let stats = self
            .get(&format!("{LIVE_CLIENT}/gamestats"), None)
            .await
            .unwrap_or(Value::Null);

        // Retrouver « moi » dans la liste des joueurs.
        let same = |p: &Value, key: &str| !me[key].is_null() && p[key] == me[key];
        let Some(p) = players
            .iter()
            .find(|p| same(p, "riotId"))
            .or_else(|| players.iter().find(|p| same(p, "summonerName")))
        else {
            return loading;
        };

        let mut items: Vec<(u64, String)> = p["items"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|it| {
                        let name = it["displayName"].as_str()?.to_string();
                        Some((it["slot"].as_u64().unwrap_or(99), name))
                    })
                    .collect()
            })
            .unwrap_or_default();
        items.sort_by_key(|(slot, _)| *slot);

        // Les 10 champions, chaque équipe dans l'ordre haut, jungle, milieu, bas, support.
        let lane_order = |player: &Value| {
            let lane = player["position"].as_str().unwrap_or_default();
            ["TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY"].iter().position(|l| *l == lane).unwrap_or(5)
        };
        let mut sorted: Vec<&Value> = players.iter().collect();
        sorted.sort_by_key(|player| lane_order(player));
        // Nom interne du champion, ex. « game_character_displayname_MonkeyKing ».
        let champion_id = |player: &Value| {
            let raw = player["rawChampionName"].as_str().unwrap_or_default();
            let alias = raw.trim_start_matches("game_character_displayname_").to_lowercase();
            self.champion_ids.get(&alias).copied().unwrap_or(0)
        };
        let ranks = self.game.as_ref().map(|g| &g.ranks);
        // Joueur absent de ceux que le client donne : il est en mode streamer, son rang est caché.
        let hidden = |id: i64| self.game.as_ref().is_some_and(|g| !g.listed.is_empty() && !g.listed.contains(&id));
        // Inventaire : (objet, nombre).
        let inventory = |player: &Value| -> Vec<(i64, u64)> {
            let list = player["items"].as_array().into_iter().flatten();
            list.filter_map(|it| Some((it["itemID"].as_i64()?, it["count"].as_u64().unwrap_or(1)))).collect()
        };
        let (mut allies, mut enemies) = (Vec::new(), Vec::new());
        for player in sorted {
            let score = |key: &str| player["scores"][key].as_u64().unwrap_or(0);
            let id = champion_id(player);
            let rank = ranks.and_then(|r| r.get(&id)).cloned();
            let entry = Player {
                champion: player["championName"].as_str().unwrap_or("?").to_string(),
                champion_id: id,
                level: player["level"].as_u64().unwrap_or(0),
                kills: score("kills"),
                deaths: score("deaths"),
                assists: score("assists"),
                me: std::ptr::eq(player, p),
                rank_hidden: rank.is_none() && hidden(id),
                rank,
                position: player["position"].as_str().unwrap_or_default().to_string(),
                items_value: self.assets.items_value(&inventory(player)),
            };
            if player["team"] == p["team"] {
                allies.push(entry);
            } else {
                enemies.push(entry);
            }
        }

        // Pour l'historique : ton champion, ton poste et l'adversaire au même poste (s'il y a des postes).
        let my_lane = p["position"].as_str().unwrap_or_default();
        let opponent = players
            .iter()
            .find(|q| !my_lane.is_empty() && q["team"] != p["team"] && q["position"] == my_lane)
            .map(champion_id)
            .filter(|id| *id > 0);
        let my_id = champion_id(p);
        // Numéro du skin dans le client : champion × 1000 + numéro du skin chez ce champion.
        let skin_id = match p["skinID"].as_i64() {
            Some(n) if my_id > 0 && n >= 0 => my_id * 1000 + n,
            _ => 0,
        };
        if let Some(game) = &mut self.game {
            if my_id > 0 {
                game.champion_id = my_id;
            }
            if skin_id > 0 {
                game.skin_id = skin_id;
            }
            let position = lane(&my_lane.to_lowercase(), "");
            if !position.is_empty() {
                game.position = position.to_string();
            }
            if opponent.is_some() {
                game.opponent_id = opponent;
            }
        }
        // Objets du panneau, et s'ils sont déjà faits d'après ton inventaire.
        let owned: Vec<i64> = p["items"].as_array().into_iter().flatten().filter_map(|it| it["itemID"].as_i64()).collect();
        let build = self.game_build(base, password).await;
        let build_done = build.as_ref().is_some_and(|b| self.first_items_done(b, &owned));

        let score = |key: &str| p["scores"][key].as_u64().unwrap_or(0);
        Game {
            ready: true,
            game_time: stats["gameTime"].as_f64().unwrap_or(0.0),
            champion: p["championName"].as_str().unwrap_or("?").to_string(),
            level: p["level"].as_u64().unwrap_or(0),
            kills: score("kills"),
            deaths: score("deaths"),
            assists: score("assists"),
            cs: score("creepScore"),
            gold: me["currentGold"].as_f64().unwrap_or(0.0) as u64,
            items: items.into_iter().map(|(_, name)| name).collect(),
            position: my_lane.to_string(),
            skin_id,
            allies,
            enemies,
            build,
            build_done,
        }
    }

    /// Objets du panneau en jeu : ceux du duel contre ton adversaire de couloir (si OP.GG a assez de parties),
    /// sinon ceux de ton champion à ton poste. Cherchés une fois par (champion, poste, adversaire).
    async fn game_build(&mut self, base: &str, password: &str) -> Option<opgg::Build> {
        let game = self.game.as_ref()?;
        let key = (game.champion_id, game.position.clone(), game.opponent_id);
        if game.build_for.as_ref() == Some(&key) {
            return game.build.clone();
        }
        if key.0 <= 0 || key.1.is_empty() {
            return None;
        }
        let mode = game.mode.clone();
        let me = self.champ(base, password, key.0).await;
        let mut build = None;
        if let Some(id) = key.2 {
            let opponent = self.champ(base, password, id).await;
            match self.matchup(&me, &opponent, &key.1).await {
                Ok(m) if m.analysis.runes.games >= MIN_MATCHUP_GAMES => build = Some(m.analysis.build),
                Ok(_) => eprintln!("[panneau] {} vs {} : trop peu de parties, objets généraux", me.name, opponent.name),
                Err(e) => eprintln!("[panneau] duel introuvable ({e}), objets généraux"),
            }
        }
        if build.is_none() {
            match self.analysis(&me, &key.1, &mode).await {
                Ok(a) => build = Some(a.build),
                Err(e) => eprintln!("[panneau] objets introuvables : {e}"),
            }
        }
        let build = build.filter(|b| !b.is_empty()).map(|b| with_atlas(b, &key.1));
        if let Some(b) = &build {
            let name = |i: Option<&opgg::Item>| i.map_or_else(|| "-".to_string(), |i| i.name.clone());
            eprintln!(
                "[panneau] {} : départ {}, 1er {} (ou {}), bottes {}, 2e {} (ou {})",
                me.name,
                b.starter.iter().map(|i| i.name.as_str()).collect::<Vec<_>>().join(" + "),
                name(b.core.first()),
                name(b.first_alt.as_ref()),
                name(b.boots.first()),
                name(b.core.get(1)),
                name(b.second_alt.as_ref()),
            );
            // Liste des objets du client : pour les icônes et pour voir quand tes objets sont faits.
            self.assets.load_items().await;
        }
        if let Some(game) = &mut self.game {
            game.build_for = Some(key);
            game.build = build.clone();
        }
        build
    }

    /// true quand tes deux premiers objets sont faits : ceux du panneau (ou leur autre choix),
    /// ou deux autres objets terminés si tu as pris un autre chemin.
    fn first_items_done(&self, build: &opgg::Build, owned: &[i64]) -> bool {
        let has = |main: Option<&opgg::Item>, alt: Option<&opgg::Item>| {
            [main, alt].into_iter().flatten().any(|i| owned.contains(&i.id))
        };
        let planned = has(build.core.first(), build.first_alt.as_ref()) && has(build.core.get(1), build.second_alt.as_ref());
        planned || self.assets.finished_count(owned) >= 2
    }

    /// La partie d'après le client : les joueurs (sauf ceux en mode streamer, que le client ne donne pas)
    /// avec leur rang dans la file de la partie. Lue une fois, dès l'écran de chargement.
    async fn start_game(&mut self, base: &str, password: &str) -> Option<CurrentGame> {
        let flow = self.get(&format!("{base}/lol-gameflow/v1/session"), Some(password)).await?;
        let data = &flow["gameData"];
        let game_id = data["gameId"].as_i64().filter(|id| *id > 0)?;
        let queue = if data["queue"]["id"] == 440 { "RANKED_FLEX_SR" } else { "RANKED_SOLO_5x5" };
        let me = self.puuid.clone().unwrap_or_default();
        let team = |key: &str| data[key].as_array().cloned().unwrap_or_default();
        let (mut mine, mut theirs) = (team("teamOne"), team("teamTwo"));
        if !me.is_empty() && theirs.iter().any(|p| p["puuid"] == me.as_str()) {
            std::mem::swap(&mut mine, &mut theirs);
        }
        let mut game = CurrentGame { game_id, mode: game_mode(&data["queue"]).to_string(), ..Default::default() };
        // Ce que la sélection a donné ; la partie en cours le précisera.
        if let Some((champion_id, position, opponent_id)) = self.last_pick.clone() {
            game.champion_id = champion_id;
            game.position = position;
            game.opponent_id = opponent_id;
        }
        for (team, allies) in [(mine, true), (theirs, false)] {
            let mut players: Vec<(usize, Player)> = Vec::new();
            for p in &team {
                let champion_id = p["championId"].as_i64().unwrap_or(0);
                let champion = if champion_id > 0 { self.champ(base, password, champion_id).await.name } else { String::new() };
                let puuid = p["puuid"].as_str().filter(|puuid| !puuid.is_empty());
                let rank = match puuid {
                    Some(puuid) => {
                        let url = format!("{base}/lol-ranked/v1/ranked-stats/{puuid}");
                        self.get(&url, Some(password)).await.map(|stats| rank_in(&stats, queue))
                    }
                    None => None,
                };
                if champion_id > 0 && puuid.is_some() {
                    game.listed.insert(champion_id);
                }
                if let Some(rank) = rank.as_ref().filter(|_| champion_id > 0) {
                    game.ranks.insert(champion_id, rank.clone());
                }
                let is_me = !me.is_empty() && p["puuid"] == me.as_str();
                if is_me && champion_id > 0 {
                    game.champion_id = champion_id;
                }
                let lane = p["selectedPosition"].as_str().unwrap_or_default();
                let order = ["TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY"].iter().position(|l| *l == lane).unwrap_or(5);
                let player = Player {
                    champion,
                    champion_id,
                    me: is_me,
                    rank_hidden: puuid.is_none(),
                    rank,
                    position: lane.to_string(),
                    ..Default::default()
                };
                players.push((order, player));
            }
            players.sort_by_key(|(order, _)| *order);
            let players = players.into_iter().map(|(_, p)| p).collect();
            if allies {
                game.allies = players;
            } else {
                game.enemies = players;
            }
        }
        eprintln!("[partie] {game_id} : {} joueurs, {} rangs lus", game.allies.len() + game.enemies.len(), game.ranks.len());
        Some(game)
    }

    /// Ton rang (solo/duo et flexible) et tes 20 dernières parties, d'après le client.
    async fn read_profile(&mut self, base: &str, password: &str) -> Option<Profile> {
        let ranked = self.get(&format!("{base}/lol-ranked/v1/current-ranked-stats"), Some(password)).await;
        let url = format!("{base}/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex=19");
        let matches = self.get(&url, Some(password)).await;
        if ranked.is_none() && matches.is_none() {
            return None;
        }
        let mut profile = Profile {
            solo: ranked.as_ref().map(|r| rank_in(r, "RANKED_SOLO_5x5")),
            flex: ranked.as_ref().map(|r| rank_in(r, "RANKED_FLEX_SR")),
            ..Default::default()
        };
        let games = matches.as_ref().and_then(|m| m["games"]["games"].as_array()).cloned().unwrap_or_default();
        let mut counts: Vec<(i64, u64, u64)> = Vec::new();
        let mut records = Vec::new();
        for game in &games {
            let Some(record) = history::record_from(game, self.puuid.as_deref(), self.summoner_id, 0) else {
                continue;
            };
            if record.duration < REMAKE_SECONDS {
                continue;
            }
            profile.recent_games += 1;
            profile.recent_wins += u64::from(record.win);
            match counts.iter_mut().find(|(id, _, _)| *id == record.champion_id) {
                Some((_, played, won)) => {
                    *played += 1;
                    *won += u64::from(record.win);
                }
                None => counts.push((record.champion_id, 1, u64::from(record.win))),
            }
            records.push(record);
        }
        // De la plus récente à la plus ancienne.
        records.sort_by_key(|r| std::cmp::Reverse(r.date));
        profile.recent_results = records.iter().map(|r| r.win).collect();
        profile.last_champion_id = records.first().map_or(0, |r| r.champion_id);
        // Les plus joués d'abord (à égalité, dans l'ordre de la liste du client).
        counts.sort_by_key(|(_, games, _)| std::cmp::Reverse(*games));
        for (champion_id, games, wins) in counts.into_iter().take(3) {
            let name = self.champ(base, password, champion_id).await.name;
            profile.recent_champions.push(RecentChampion { champion_id, name, games, wins });
        }
        records.truncate(MATCHES_SHOWN);
        for mut record in records {
            record.champion = self.champ(base, password, record.champion_id).await.name;
            // Partie suivie par l'app : on connaît aussi ton adversaire direct, ton poste et ton skin.
            if let Some(known) = self.history.get(record.game_id) {
                record.position.clone_from(&known.position);
                record.opponent_id = known.opponent_id;
                record.opponent.clone_from(&known.opponent);
                record.skin_id = known.skin_id;
            }
            profile.matches.push(record);
        }
        Some(profile)
    }

    /// Partie finie : ses stats exactes arrivent dans l'historique du client un peu après la fin.
    /// On réessaie toutes les 5 secondes, 20 minutes au plus.
    async fn record_finished(&mut self, base: &str, password: &str) {
        let Some(finished) = &self.finished else {
            return;
        };
        let now = Instant::now();
        if now < finished.next_try {
            return;
        }
        let game = finished.game.clone();
        if now > finished.give_up || self.history.contains(game.game_id) {
            eprintln!("[historique] partie {} : abandon (introuvable ou déjà enregistrée)", game.game_id);
            self.finished = None;
            return;
        }
        let doc = self.get(&format!("{base}/lol-match-history/v1/games/{}", game.game_id), Some(password)).await;
        let record = doc
            .as_ref()
            .and_then(|doc| history::record_from(doc, self.puuid.as_deref(), self.summoner_id, game.champion_id));
        let (Some(doc), Some(mut record)) = (doc, record) else {
            if let Some(finished) = &mut self.finished {
                finished.next_try = now + RECORD_RETRY;
            }
            return;
        };
        self.finished = None;
        if record.duration < REMAKE_SECONDS {
            eprintln!("[historique] partie {} annulée (remake) : pas enregistrée", game.game_id);
            return;
        }
        record.champion = self.champ(base, password, record.champion_id).await.name;
        record.position = game.position;
        // Le skin joué, s'il est bien celui de ce champion.
        if game.skin_id / 1000 == record.champion_id {
            record.skin_id = game.skin_id;
        }
        if let Some(id) = game.opponent_id {
            record.opponent_id = id;
            record.opponent = self.champ(base, password, id).await.name;
        }
        let line = format!(
            "[historique] partie {} enregistrée : {} {}, {}/{}/{}, {} CS",
            record.game_id,
            record.champion,
            if record.win { "victoire" } else { "défaite" },
            record.kills,
            record.deaths,
            record.assists,
            record.cs
        );
        match self.history.add(record, &doc) {
            Ok(()) => {
                eprintln!("{line}");
                self.profile_due = true;
            }
            Err(e) => eprintln!("[historique] {e}"),
        }
    }

    /// Requête locale. `password` = authentification de l'API du client (utilisateur « riot »).
    async fn send(&self, method: Method, url: &str, password: Option<&str>, body: Option<&Value>) -> Result<Value, String> {
        let mut req = self.http.request(method, url);
        if let Some(password) = password {
            req = req.basic_auth("riot", Some(password));
        }
        if let Some(body) = body {
            req = req.json(body);
        }
        let resp = req.send().await.map_err(|_| "client injoignable".to_string())?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("HTTP {}", status.as_u16()));
        }
        let text = resp.text().await.unwrap_or_default();
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn get(&self, url: &str, password: Option<&str>) -> Option<Value> {
        self.send(Method::GET, url, password, None).await.ok()
    }

    /// Le client écrit un fichier « lockfile » à chaque démarrage :
    /// nom:pid:port:mot_de_passe:protocole
    fn refresh_lockfile(&mut self) {
        let raw = read_lockfile().unwrap_or_default();
        if raw == self.lockfile {
            return;
        }
        let parts: Vec<&str> = raw.trim().split(':').collect();
        self.lcu = match parts.as_slice() {
            [_, _, port, password, _, ..] => port
                .parse::<u16>()
                .ok()
                .map(|port| (format!("https://127.0.0.1:{port}"), password.to_string())),
            _ => None,
        };
        self.lockfile = raw;
        self.riot_id = None;
        self.summoner_level = None;
        // Le client a redémarré (peut-être sur un autre compte) : profil à relire.
        self.puuid = None;
        self.summoner_id = None;
        self.profile = None;
        self.profile_due = true;
        self.spells_set = None;
        self.last_phase = None;
        self.rune_import = None;
        self.spell_names.clear();
        self.champs.clear();
        self.champion_ids.clear();
        *self.assets.auth.lock().unwrap() = self.lcu.clone();
    }
}

/// Ta case dans l'équipe, pendant la sélection.
fn my_cell(session: &Value) -> Option<&Value> {
    session["myTeam"]
        .as_array()?
        .iter()
        .find(|p| p["cellId"] == session["localPlayerCellId"])
}

/// Champion verrouillé : d'après le client, sinon d'après ton action « choix » terminée,
/// sinon (ARAM, sans choix) le champion qui t'est attribué. 0 = pas encore.
fn locked_champion(current: Option<&Value>, session: &Value) -> i64 {
    if let Some(id) = current.and_then(Value::as_i64).filter(|id| *id > 0) {
        return id;
    }
    let me = &session["localPlayerCellId"];
    let my_picks: Vec<&Value> = session["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
        .filter(|a| a["actorCellId"] == *me && a["type"] == "pick")
        .collect();
    if let Some(id) = my_picks
        .iter()
        .find(|a| a["completed"] == true)
        .and_then(|a| a["championId"].as_i64())
        .filter(|id| *id > 0)
    {
        return id;
    }
    if my_picks.is_empty() {
        return my_cell(session).and_then(|p| p["championId"].as_i64()).unwrap_or(0);
    }
    0
}

/// Champions déjà pris ou bannis, par une équipe ou l'autre.
fn taken_champions(session: &Value, enemies: &[opgg::Champ]) -> Vec<i64> {
    let actions = session["actions"].as_array().into_iter().flatten().filter_map(Value::as_array).flatten();
    let done = actions.filter(|a| a["completed"] == true).filter_map(|a| a["championId"].as_i64());
    let bans = ["myTeamBans", "theirTeamBans"]
        .iter()
        .flat_map(|team| session["bans"][team].as_array().into_iter().flatten().filter_map(Value::as_i64));
    done.chain(bans).chain(enemies.iter().map(|e| e.id)).filter(|id| *id > 0).collect()
}

/// Ton partenaire le plus proche : (son poste pour le client, son poste pour OP.GG).
/// Le jungler pour le haut et le milieu, le milieu pour le jungler, l'autre joueur du bas pour le duo.
fn partner_position(position: &str) -> Option<(&'static str, &'static str)> {
    match position {
        "top" | "mid" => Some(("jungle", "jungle")),
        "jungle" => Some(("middle", "mid")),
        "adc" => Some(("utility", "support")),
        "support" => Some(("bottom", "adc")),
        _ => None,
    }
}

/// Place deux sorts sur D (premier) et F (second). Le Saut éclair va sur la touche choisie
/// (« auto » : là où il est déjà, sinon F) ; sinon, un sort déjà en place ne bouge pas.
fn spell_slots(wanted: [i64; 2], current: [i64; 2], flash_key: &str) -> [i64; 2] {
    let [a, b] = wanted;
    if wanted.contains(&FLASH) {
        let other = if a == FLASH { b } else { a };
        let flash_on_d = match flash_key {
            "D" => true,
            "F" => false,
            _ => current[0] == FLASH,
        };
        return if flash_on_d { [FLASH, other] } else { [other, FLASH] };
    }
    if a == current[1] || b == current[0] {
        [b, a]
    } else {
        wanted
    }
}

/// Rang d'une file dans une réponse de /lol-ranked (liste « queues », ou table « queueMap »).
/// Pas d'entrée pour cette file, ou tier « NONE » : non classé.
fn rank_in(stats: &Value, queue: &str) -> Rank {
    let entry = stats["queues"]
        .as_array()
        .and_then(|queues| queues.iter().find(|q| q["queueType"] == queue))
        .unwrap_or(&stats["queueMap"][queue]);
    let text = |key: &str| entry[key].as_str().unwrap_or_default().to_string();
    let tier = text("tier");
    if matches!(tier.as_str(), "" | "NONE" | "UNRANKED") {
        return Rank::default();
    }
    let division = text("division");
    Rank {
        tier,
        division: if division == "NA" { String::new() } else { division },
        lp: entry["leaguePoints"].as_i64().unwrap_or(0),
        wins: entry["wins"].as_u64().unwrap_or(0),
        losses: entry["losses"].as_u64().unwrap_or(0),
    }
}

/// Support : l'Atlas en premier objet de départ (OP.GG ne le compte pas dans les départs).
fn with_atlas(mut build: opgg::Build, position: &str) -> opgg::Build {
    if position == "support" && !build.is_empty() && !build.starter.iter().any(|i| i.id == ATLAS) {
        build.starter.insert(0, opgg::Item { id: ATLAS, name: "Atlas".to_string(), count: 1 });
    }
    build
}

/// Set d'objets au format du client : il apparaît dans la boutique, pour ce champion.
fn item_set(champion_id: i64, title: &str, map_id: i64, build: &opgg::Build) -> Value {
    let block = |title: String, items: &[opgg::Item]| {
        json!({
            "type": title,
            "items": items.iter().map(|i| json!({ "id": i.id.to_string(), "count": i.count })).collect::<Vec<_>>(),
            "showIfSummonerSpell": "",
            "hideIfSummonerSpell": "",
        })
    };
    // L'ordre de montée des compétences est rappelé dans le titre du premier bloc.
    let starter = match build.skill_max.join(" > ") {
        skills if skills.is_empty() => "Départ".to_string(),
        skills => format!("Départ · {skills}"),
    };
    let blocks: Vec<Value> = [
        (starter, &build.starter),
        ("Build principal".to_string(), &build.core),
        ("Bottes".to_string(), &build.boots),
        ("Ensuite (au choix)".to_string(), &build.options),
    ]
    .into_iter()
    .filter(|(_, items)| !items.is_empty())
    .map(|(title, items)| block(title, items))
    .collect();
    // Carte inconnue : Faille de l'invocateur et Abîme hurlant.
    let maps = if map_id > 0 { vec![map_id] } else { vec![11, 12] };
    json!({
        "uid": new_uid(),
        "title": title,
        "type": "custom",
        "map": "any",
        "mode": "any",
        "sortrank": 0,
        "startedFrom": "blank",
        "associatedChampions": [champion_id],
        "associatedMaps": maps,
        "blocks": blocks,
        "preferredItemSlots": [],
    })
}

/// Identifiant au format UUID pour un nouveau set (aléatoire, sans dépendance en plus).
fn new_uid() -> String {
    use std::hash::{BuildHasher, Hasher};
    let random = || std::collections::hash_map::RandomState::new().build_hasher().finish();
    let hex = format!("{:016x}{:016x}", random(), random());
    format!("{}-{}-4{}-8{}-{}", &hex[0..8], &hex[8..12], &hex[13..16], &hex[17..20], &hex[20..32])
}

/// Mode de jeu au sens d'OP.GG, d'après la file d'attente du client.
fn game_mode(queue: &Value) -> &'static str {
    match (queue["id"].as_i64().unwrap_or(0), queue["gameMode"].as_str().unwrap_or_default()) {
        (440, _) => "flex",
        (_, "ARAM") => "aram",
        (_, "URF" | "ARURF") => "urf",
        (_, "NEXUSBLITZ") => "nexus_blitz",
        _ => "ranked",
    }
}

/// Poste au sens d'OP.GG. Vide si le client n'en attribue pas (hors ARAM/URF/Nexus Blitz).
fn lane(assigned: &str, mode: &str) -> &'static str {
    match assigned {
        "top" => "top",
        "jungle" => "jungle",
        "middle" => "mid",
        "bottom" => "adc",
        "utility" => "support",
        // Modes sans couloirs : OP.GG demande quand même un poste.
        _ if matches!(mode, "aram" | "urf" | "nexus_blitz") => "mid",
        _ => "",
    }
}

fn read_lockfile() -> Option<String> {
    let mut paths: Vec<PathBuf> = Vec::new();
    // Chemin forcé à la main si besoin : variable d'environnement LOL_LOCKFILE.
    if let Ok(path) = std::env::var("LOL_LOCKFILE") {
        paths.push(path.into());
    }
    paths.push(r"C:\Riot Games\League of Legends\lockfile".into());
    paths.push("/Applications/League of Legends.app/Contents/LoL/lockfile".into());
    if let Some(raw) = paths.iter().find_map(|p| fs::read_to_string(p).ok()) {
        return Some(raw);
    }

    // Jeu installé ailleurs (ex. D:\) : le Riot Client liste ses dossiers d'installation ici.
    let program_data = std::env::var("PROGRAMDATA").ok()?;
    let installs = PathBuf::from(program_data).join("Riot Games").join("RiotClientInstalls.json");
    let json: Value = serde_json::from_str(&fs::read_to_string(installs).ok()?).ok()?;
    json["associated_client"]
        .as_object()?
        .keys()
        .map(PathBuf::from)
        .filter(|dir| dir.join("LeagueClient.exe").exists())
        .find_map(|dir| fs::read_to_string(dir.join("lockfile")).ok())
}
