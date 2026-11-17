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
/// Contres proposés en sélection (les champions qui battent le mieux ton adversaire).
const COUNTERS_SHOWN: usize = 10;
/// Après une partie, ses stats arrivent dans l'historique du client avec un peu de retard.
const RECORD_RETRY: Duration = Duration::from_secs(5);
const RECORD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// Partie classée dans l'historique mais pas encore comptée dans ton rang : on attend au plus ça ses LP.
const LP_WAIT: Duration = Duration::from_secs(3 * 60);
/// Objet gardé ce temps-là après son achat : acheté pour de bon, il quitte le panneau (un achat annulé ou
/// revendu tout de suite n'y compte pas).
const BOUGHT_AFTER: Duration = Duration::from_secs(30);
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
    /// Pourquoi la page de runes n'a pas été changée (tu as changé de page ou de runes depuis l'import).
    pub page_note: Option<String>,
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
    /// Chemin de l'icône de chaque sort d'invocateur, lus une fois dans le client.
    spell_paths: Arc<Mutex<HashMap<i64, String>>>,
    /// Parties déjà ouvertes en détail : (partie, ton champion) → les deux équipes.
    matches: Arc<Mutex<HashMap<(i64, i64), history::MatchDetails>>>,
    /// Nom de chaque champion d'après le client, lus une fois (salon d'un joueur cherché).
    champion_names: Arc<Mutex<HashMap<i64, String>>>,
    /// Icônes des runes et emblèmes des rangs déjà lus, par chemin.
    images: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// Requêtes en cours vers le client : 6 au plus (voir client_file).
    gate: Arc<tokio::sync::Semaphore>,
    /// Listes lues une fois (objets, runes, sorts) : une seule lecture à la fois. Sans ça, chaque icône
    /// demandée pendant la lecture relirait toute la liste.
    loading: Arc<tokio::sync::Mutex<()>>,
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
    /// Prix total dans la boutique (0 pour les balises, données gratuitement).
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
            spell_paths: Arc::default(),
            matches: Arc::default(),
            champion_names: Arc::default(),
            images: Arc::default(),
            gate: Arc::new(tokio::sync::Semaphore::new(6)),
            loading: Arc::default(),
            web,
        }
    }

    /// Une partie de ton historique en détail (les 10 joueurs et leur rang solo/duo actuel), lue dans le
    /// client puis gardée en mémoire. `champion_id` : ton champion, pour savoir quelle équipe est la tienne.
    pub async fn match_details(&self, game_id: i64, champion_id: i64) -> Result<history::MatchDetails, String> {
        if let Some(details) = self.matches.lock().unwrap().get(&(game_id, champion_id)) {
            return Ok(details.clone());
        }
        let Some((base, password)) = self.auth.lock().unwrap().clone() else {
            return Err("Lance le client League of Legends pour voir le détail de la partie.".to_string());
        };
        // Le client va la chercher chez Riot : ça peut prendre plus que les 2 secondes habituelles.
        let resp = self
            .http
            .get(format!("{base}/lol-match-history/v1/games/{game_id}"))
            .basic_auth("riot", Some(password))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .ok()
            .filter(|resp| resp.status().is_success());
        let doc = match resp {
            Some(resp) => resp.json::<Value>().await.ok(),
            None => None,
        };
        let mut details = doc
            .as_ref()
            .and_then(|doc| history::match_from(doc, champion_id))
            .ok_or("Le client n'a pas donné le détail de cette partie.")?;
        for player in details.teams.iter_mut().flat_map(|team| team.players.iter_mut()) {
            if player.puuid.is_empty() {
                continue;
            }
            let stats = self.client_file(&format!("/lol-ranked/v1/ranked-stats/{}", player.puuid)).await;
            player.rank = stats
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .map(|stats| rank_in(&stats, "RANKED_SOLO_5x5"));
        }
        self.matches.lock().unwrap().insert((game_id, champion_id), details.clone());
        Ok(details)
    }

    /// Icône d'un sort d'invocateur (PNG), lue dans le client puis gardée en mémoire.
    pub async fn spell_icon(&self, id: i64) -> Option<Vec<u8>> {
        if self.spell_paths.lock().unwrap().is_empty() {
            // Liste lue une seule fois : les icônes demandées en même temps attendent et s'en servent.
            let _loading = self.loading.lock().await;
            if self.spell_paths.lock().unwrap().is_empty() {
                let doc = self.client_file("/lol-game-data/assets/v1/summoner-spells.json").await;
                let doc = doc.and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).unwrap_or(Value::Null);
                let paths: HashMap<i64, String> = doc
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|spell| Some((spell["id"].as_i64()?, spell["iconPath"].as_str()?.to_string())))
                    .collect();
                if paths.is_empty() {
                    return None;
                }
                *self.spell_paths.lock().unwrap() = paths;
            }
        }
        let path = self.spell_paths.lock().unwrap().get(&id)?.clone();
        self.cached_client_file(&path).await
    }

    /// Icône d'une rune ou d'une voie de runes (PNG), lue dans le client puis gardée en mémoire.
    pub async fn perk_icon(&self, id: i64) -> Option<Vec<u8>> {
        if self.perk_paths.lock().unwrap().is_empty() {
            // Listes lues une seule fois (comme pour les sorts).
            let _loading = self.loading.lock().await;
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

    /// Icône de l'or (celle du tableau des scores de l'historique du client), lue dans le client ;
    /// sinon sur CommunityDragon, qui reprend ses fichiers. Gardée en mémoire.
    pub async fn gold_icon(&self) -> Option<Vec<u8>> {
        let path = "/fe/lol-match-history/icon_gold.png";
        if let Some(png) = self.cached_client_file(path).await {
            return Some(png);
        }
        let url = "https://raw.communitydragon.org/latest/plugins/rcp-fe-lol-match-history/global/default/icon_gold.png";
        let resp = self.web.get(url).send().await.ok().filter(|r| r.status().is_success())?;
        let png = resp.bytes().await.ok()?.to_vec();
        self.images.lock().unwrap().insert(path.to_string(), png.clone());
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
        let png = self.client_file(&format!("/lol-game-data/assets/v1/champion-icons/{id}.png")).await?;
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
        let png = self.client_file(&icon).await?;
        self.item_icons.lock().unwrap().insert(id, png.clone());
        Some(png)
    }

    /// Lit une fois la liste des objets du client. false si elle n'est pas disponible.
    async fn load_items(&self) -> bool {
        if !self.items.lock().unwrap().is_empty() {
            return true;
        }
        // Liste lue une seule fois (comme pour les sorts).
        let _loading = self.loading.lock().await;
        if !self.items.lock().unwrap().is_empty() {
            return true;
        }
        let Some(bytes) = self.client_file("/lol-game-data/assets/v1/items.json").await else {
            return false;
        };
        let Ok(Value::Array(list)) = serde_json::from_slice::<Value>(&bytes) else {
            return false;
        };
        let items: HashMap<i64, ItemData> = list
            .iter()
            .filter_map(|it| {
                let price = it["priceTotal"].as_u64().unwrap_or(0);
                Some((it["id"].as_i64()?, ItemData { icon: it["iconPath"].as_str()?.to_string(), price }))
            })
            .collect();
        let loaded = !items.is_empty();
        *self.items.lock().unwrap() = items;
        loaded
    }

    /// Prix d'un objet dans la boutique (0 tant que la liste des objets n'est pas lue).
    fn price(&self, id: i64) -> u64 {
        self.items.lock().unwrap().get(&id).map_or(0, |i| i.price)
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
    /// Au plus 6 à la fois, comme un navigateur : une partie en détail demande ~80 icônes d'un coup,
    /// et une rafale de connexions peut être refusée.
    async fn client_file(&self, path: &str) -> Option<Vec<u8>> {
        let (base, password) = self.auth.lock().unwrap().clone()?;
        let _turn = self.gate.acquire().await.ok()?;
        let resp = self.http.get(format!("{base}{path}")).basic_auth("riot", Some(password)).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        Some(resp.bytes().await.ok()?.to_vec())
    }

    /// Réponse JSON du client. `from_riot` : ce que le client va chercher chez Riot (10 s au plus, au lieu de 2).
    pub async fn client_json(&self, path: &str, from_riot: bool) -> Option<Value> {
        let (base, password) = self.auth.lock().unwrap().clone()?;
        let timeout = Duration::from_secs(if from_riot { 10 } else { 2 });
        let resp = self.http.get(format!("{base}{path}")).basic_auth("riot", Some(password)).timeout(timeout).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json().await.ok()
    }

    /// Région du client (« EUW », « NA »…) ; None si le client est fermé.
    pub async fn client_region(&self) -> Option<String> {
        let locale = self.client_json("/riotclient/region-locale", false).await?;
        locale["region"].as_str().filter(|r| !r.is_empty()).map(str::to_uppercase)
    }

    /// Nom d'un champion d'après le client (la liste est lue une fois) ; son numéro s'il est inconnu.
    pub async fn champion_name(&self, id: i64) -> String {
        if self.champion_names.lock().unwrap().is_empty() {
            let _once = self.loading.lock().await;
            if self.champion_names.lock().unwrap().is_empty() {
                let list = self.client_json("/lol-game-data/assets/v1/champion-summary.json", false).await;
                let names = list.as_ref().and_then(Value::as_array).into_iter().flatten();
                let names = names.filter_map(|c| Some((c["id"].as_i64()?, c["name"].as_str()?.to_string())));
                self.champion_names.lock().unwrap().extend(names);
            }
        }
        self.champion_names.lock().unwrap().get(&id).cloned().unwrap_or_else(|| id.to_string())
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
    /// Ton poste : "TOP", "JUNGLE", "MIDDLE", "BOTTOM", "UTILITY" ; vide ou "NONE" dans les modes sans postes.
    pub position: String,
    /// Ton skin (numéro du client, ex. 267002) ; 0 si inconnu.
    pub skin_id: i64,
    /// Les 10 champions : ton équipe et l'équipe adverse, chacune triée par poste.
    pub allies: Vec<Player>,
    pub enemies: Vec<Player>,
    /// Panneau posé sur le jeu : les objets à acheter (ceux du duel contre ton adversaire de couloir),
    /// une rangée par étape (départ, 1er objet, bottes, 2e objet), sans ceux déjà achetés. Vide : rien à montrer.
    pub panel: Vec<Vec<PanelItem>>,
}

/// Un objet du panneau posé sur le jeu.
#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PanelItem {
    pub id: i64,
    /// Ex. 2 pour deux potions de départ.
    pub count: u32,
    /// Taille de l'icône : "starter", "main", "boots", ou "alt" (autre choix possible, plus petit).
    pub size: &'static str,
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
    /// Joueur venu avec d'autres (duo, groupe) : numéro de son groupe dans la partie (1, 2…).
    pub group: Option<u8>,
    /// Joueur déjà croisé dans une de tes 3 dernières parties.
    pub met: Option<Met>,
}

/// Joueur déjà croisé dans une de tes 3 dernières parties, sans être venu avec toi.
#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Met {
    /// 1 = ta dernière partie, 2 = l'avant-dernière, 3.
    pub ago: u8,
    /// true : il était dans ton équipe ; false : contre toi.
    pub ally: bool,
    /// Le champion qu'il jouait.
    pub champion: String,
}

/// Une partie passée d'un joueur : son équipe, son champion et quand elle a commencé.
#[derive(Clone, Copy, Default)]
struct Past {
    team: i64,
    champion: i64,
    start: i64,
}

/// Un joueur de la partie et ses dernières parties : pour les groupes et les joueurs déjà croisés.
#[derive(Clone, Default)]
struct Recent {
    ally: bool,
    champion_id: i64,
    puuid: String,
    /// Ses 20 dernières parties (sans celle en cours), par numéro de partie.
    games: HashMap<i64, Past>,
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
    /// Ton rang dans la file de la partie (solo/duo ou flexible) au début de la partie : LP d'avant.
    lp_before: Option<Rank>,
    /// Objets du panneau, et pour quels (champion, poste, adversaire) ils ont été cherchés.
    build: Option<opgg::Build>,
    build_for: Option<(i64, String, Option<i64>)>,
    /// Depuis quand chaque objet de ton inventaire y est.
    held_since: HashMap<i64, Instant>,
    /// Objets gardés au moins 30 s : achetés pour de bon, ils ne reviennent plus dans le panneau.
    bought: HashSet<i64>,
    /// Joueurs dont il reste à lire les dernières parties, et ceux déjà lus.
    to_check: Vec<Recent>,
    checked: Vec<Recent>,
    /// Groupe de chaque joueur venu avec d'autres, par champion.
    groups: HashMap<i64, u8>,
    /// Joueurs déjà croisés dans tes 3 dernières parties, par champion.
    met: HashMap<i64, Met>,
}

/// Partie finie, en attente de ses stats dans l'historique du client.
struct Finished {
    game: CurrentGame,
    next_try: Instant,
    give_up: Instant,
    /// Partie classée déjà dans l'historique : jusqu'à quand attendre que ton rang la compte (voir LP_WAIT).
    lp_until: Option<Instant>,
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
    /// Liste complète des contres d'un champion à un poste : (champion, poste) -> contres.
    counters_cache: HashMap<(i64, String), Vec<opgg::Pick>>,
    /// Sorts mis par l'app pendant cette sélection : s'ils ont changé depuis, c'est toi qui les as changés.
    spells_set: Option<[i64; 2]>,
    /// Tes sorts tels que tu les as mis, relus à chaque tour tant que l'app n'y a pas touché :
    /// de quoi savoir où est ton Saut éclair si le client les donne mal au moment de l'import.
    seen_spells: Option<[i64; 2]>,
    /// Page de runes écrite par l'app pendant cette sélection : (numéro, runes). Si tu as changé de page
    /// ou de runes depuis, c'est ton choix : on n'y touche plus.
    page_set: Option<(i64, Vec<i64>)>,
    /// Compte pour lequel les anciennes parties sans compte ont déjà été rangées.
    owners_checked: Option<String>,
    /// Parties sans compte déjà essayées en vain depuis le démarrage du client.
    owners_skip: usize,
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
            counters_cache: HashMap::new(),
            spells_set: None,
            seen_spells: None,
            page_set: None,
            owners_checked: None,
            owners_skip: 0,
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
        // Client fermé (ou en train de démarrer) : l'historique de l'app reste consultable
        // (celui du compte de la dernière partie, tant que le compte n'est pas connu).
        let offline = Snapshot { history: self.history.overview(self.puuid.as_deref()), ..Default::default() };
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
        // Parties enregistrées avant le rangement par compte : une fois par compte connecté, petit à petit
        // et seulement à l'accueil ou au salon (le client va chercher chaque partie chez Riot).
        if self.puuid.is_some()
            && self.owners_checked != self.puuid
            && matches!(phase.as_str(), "None" | "Lobby")
            && self.assign_owners(&base, &password).await
        {
            self.owners_checked.clone_from(&self.puuid);
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
            self.seen_spells = None;
            self.page_set = None;
            None
        };

        let in_game = matches!(phase.as_str(), "GameStart" | "InProgress" | "Reconnect");
        let game = if in_game { Some(self.live_game(&base, &password).await) } else { None };
        // Fin de partie : on va chercher ses stats exactes dans l'historique du client.
        if let Some(game) = self.game.take_if(|_| !in_game) {
            let now = Instant::now();
            self.finished = Some(Finished { game, next_try: now, give_up: now + RECORD_TIMEOUT, lp_until: None });
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
            history: self.history.overview(self.puuid.as_deref()),
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

        // Tes sorts, tant que l'app n'y a pas touché.
        if self.spells_set.is_none() {
            self.seen_spells = my_cell(&session).and_then(spells_of).or(self.seen_spells);
        }

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
                (result.duel_games, result.duel_wins) = self.history.versus(self.puuid.as_deref(), champion_id, id);
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
        // Les meilleurs, sans les champions déjà pris ou bannis.
        let free = |list: &[opgg::Pick], count: usize| -> Vec<opgg::Pick> {
            list.iter().filter(|p| !taken.contains(&p.champion_id)).take(count).cloned().collect()
        };
        let mut help = PickHelp::default();
        let mut asked = false;
        if let Some(opponent) = opponent {
            help.opponent = opponent.name.clone();
            match self.help_of(opponent, position, mode, &mut asked).await {
                Some(found) => {
                    // Les 10 qui le battent le mieux ; en attendant (ou si OP.GG ne la donne pas), les 3 de l'analyse.
                    let all = self.counters_of(base, password, opponent, &found, position, &mut asked).await;
                    help.pending |= all.is_none();
                    let list = all.as_deref().filter(|all| !all.is_empty()).unwrap_or(&found.counters);
                    help.counters = free(list, COUNTERS_SHOWN);
                }
                None => help.pending = true,
            }
        }
        if let Some((id, partner_position)) = partner {
            let partner = self.champ(base, password, id).await;
            help.partner = partner.name.clone();
            match self.help_of(&partner, partner_position, mode, &mut asked).await {
                Some(found) => help.synergies = free(found.synergies.get(position).map(Vec::as_slice).unwrap_or_default(), 3),
                None => help.pending = true,
            }
        }
        Some(help)
    }

    /// Liste complète des contres de ton adversaire (gardée en mémoire). OP.GG la donne avec un guide
    /// de couloir, qui demande un duel : on prend son premier contre de l'analyse, un duel courant.
    /// None : question remise au tour suivant (`asked`).
    #[allow(clippy::too_many_arguments)]
    async fn counters_of(
        &mut self,
        base: &str,
        password: &str,
        opponent: &opgg::Champ,
        help: &opgg::Help,
        position: &str,
        asked: &mut bool,
    ) -> Option<Vec<opgg::Pick>> {
        let key = (opponent.id, position.to_string());
        if let Some(found) = self.counters_cache.get(&key) {
            return Some(found.clone());
        }
        let Some(first) = help.counters.first() else {
            return Some(Vec::new());
        };
        if *asked {
            return None;
        }
        *asked = true;
        let against = self.champ(base, password, first.champion_id).await;
        // En cas d'échec, on garde une liste vide (les 3 de l'analyse restent affichés) pour ne pas redemander.
        let found = match self.opgg.counters(opponent, &against, position).await {
            Ok(found) => {
                eprintln!("[aide] {} {position} : {} contres (liste complète)", opponent.name, found.len());
                found
            }
            Err(e) => {
                eprintln!("[aide] {} {position}, liste des contres : {e}", opponent.name);
                Vec::new()
            }
        };
        self.counters_cache.insert(key, found.clone());
        Some(found)
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
                Ok(true) => {
                    eprintln!("[runes] page appliquée{versus} : {} ({} / {})", runes.keystone, runes.primary_name, runes.secondary_name);
                    result.page_applied = true;
                }
                Ok(false) => {
                    eprintln!("[runes] page gardée : tu as changé de page ou de runes");
                    result.page_note = Some("Ta page gardée : tu as changé de page ou de runes".to_string());
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
        // Sorts du moment ; si le client les donne mal à cet instant, ceux lus juste avant (jamais au hasard).
        let current = spells_of(cell)
            .or(self.seen_spells)
            .ok_or("Sorts illisibles dans la sélection : tes sorts n'ont pas été changés.")?;
        if self.spells_set.is_some_and(|set| set != current) {
            return Err("Sorts gardés : tu les as changés à la main.".to_string());
        }
        let flash_key = self.settings.flash_key.clone();
        let spells = spell_slots([a, b], current, &flash_key);
        eprintln!(
            "[sorts] avant D = {}, F = {} ; après D = {}, F = {} (Saut éclair : {flash_key})",
            current[0], current[1], spells[0], spells[1]
        );
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
    /// Met les runes dans ta page de runes sélectionnée, renommée « LoL Companion ». Le client ne laisse pas
    /// modifier une page : elle est supprimée puis recréée à sa place (si la recréation échoue, l'ancienne est
    /// remise). Page sélectionnée par défaut (qui ne se supprime pas) : une page « LoL Companion » à part.
    /// Ok(false) : tu as changé de page ou de runes depuis l'import précédent, l'app n'y touche plus.
    async fn apply_page(&mut self, base: &str, password: &str, runes: &opgg::Runes) -> Result<bool, String> {
        let pages_url = format!("{base}/lol-perks/v1/pages");
        let current = self.get(&format!("{base}/lol-perks/v1/currentpage"), Some(password)).await;
        let current_id = current.as_ref().and_then(|p| p["id"].as_i64());
        if let Some((id, perks)) = &self.page_set {
            let unchanged = current_id == Some(*id) && current.as_ref().is_some_and(|p| sorted_perks(p) == sorted(perks));
            if !unchanged {
                return Ok(false);
            }
        }
        // La page à remplacer : la tienne si elle peut l'être, sinon l'ancienne page « LoL Companion ».
        let mine = current.filter(|p| p["isDeletable"] == true && current_id.is_some());
        let target = match &mine {
            Some(page) => Some(page.clone()),
            None => {
                let pages = self.get(&pages_url, Some(password)).await.unwrap_or(Value::Null);
                let list = pages.as_array().cloned().unwrap_or_default();
                list.into_iter().find(|p| p["name"] == PAGE_NAME && p["isDeletable"] == true)
            }
        };
        if let Some(id) = target.as_ref().and_then(|p| p["id"].as_i64()) {
            let _ = self.send(Method::DELETE, &format!("{pages_url}/{id}"), Some(password), None).await;
        }
        let page = json!({
            "name": PAGE_NAME,
            "primaryStyleId": runes.primary_style,
            "subStyleId": runes.sub_style,
            "selectedPerkIds": runes.perks,
            "current": true,
        });
        match self.send(Method::POST, &pages_url, Some(password), Some(&page)).await {
            Ok(created) => {
                let id = match created["id"].as_i64() {
                    Some(id) => Some(id),
                    None => self.get(&format!("{base}/lol-perks/v1/currentpage"), Some(password)).await.and_then(|p| p["id"].as_i64()),
                };
                self.page_set = id.map(|id| (id, runes.perks.clone()));
                Ok(true)
            }
            Err(e) => {
                // Ta page a été supprimée mais pas remplacée : on la remet telle qu'elle était.
                if let Some(old) = &mine {
                    let restored = json!({
                        "name": old["name"],
                        "primaryStyleId": old["primaryStyleId"],
                        "subStyleId": old["subStyleId"],
                        "selectedPerkIds": old["selectedPerkIds"],
                        "current": true,
                    });
                    if let Err(e) = self.send(Method::POST, &pages_url, Some(password), Some(&restored)).await {
                        eprintln!("[runes] ta page « {} » n'a pas pu être remise ({e})", old["name"].as_str().unwrap_or("?"));
                    }
                }
                Err(if mine.is_some() {
                    format!("Le client a refusé la page ({e}) : ta page sélectionnée est restée telle quelle.")
                } else {
                    format!("Le client a refusé la page ({e}). Si toutes tes pages sont prises, sélectionne une des tiennes : l'app écrira dedans.")
                })
            }
        }
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
        self.check_groups(base, password).await;
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
        let groups = self.game.as_ref().map(|g| &g.groups);
        let met = self.game.as_ref().map(|g| &g.met);
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
                group: groups.and_then(|g| g.get(&id)).copied(),
                met: met.and_then(|m| m.get(&id)).cloned(),
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
        // Objets du panneau : ceux du build, sans ceux que tu as achetés pour de bon.
        let owned: Vec<i64> = p["items"].as_array().into_iter().flatten().filter_map(|it| it["itemID"].as_i64()).collect();
        self.track_purchases(&owned);
        let build = self.game_build(base, password).await;
        let panel = match (&build, &self.game) {
            (Some(build), Some(game)) => {
                // Départ : sa rangée part dès ton premier achat gardé 30 s, même si c'est un autre départ.
                let started = game.bought.iter().any(|id| self.assets.price(*id) > 0);
                panel(build, &game.bought, started)
            }
            _ => Vec::new(),
        };

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
            position: my_lane.to_string(),
            skin_id,
            allies,
            enemies,
            panel,
        }
    }

    /// Note depuis quand chaque objet est dans ton inventaire : gardé 30 s, il est acheté pour de bon.
    fn track_purchases(&mut self, owned: &[i64]) {
        let Some(game) = &mut self.game else {
            return;
        };
        let now = Instant::now();
        game.held_since.retain(|id, _| owned.contains(id));
        for id in owned {
            game.held_since.entry(*id).or_insert(now);
        }
        for (id, since) in &game.held_since {
            if now.duration_since(*since) >= BOUGHT_AFTER && game.bought.insert(*id) {
                eprintln!("[panneau] objet {id} gardé 30 s : acheté");
            }
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
                if let Some(puuid) = puuid.filter(|_| champion_id > 0) {
                    game.listed.insert(champion_id);
                    let puuid = puuid.to_string();
                    game.to_check.push(Recent { ally: allies, champion_id, puuid, ..Default::default() });
                }
                if let Some(rank) = rank.as_ref().filter(|_| champion_id > 0) {
                    game.ranks.insert(champion_id, rank.clone());
                }
                let is_me = !me.is_empty() && p["puuid"] == me.as_str();
                if is_me && champion_id > 0 {
                    game.champion_id = champion_id;
                }
                if is_me {
                    game.lp_before.clone_from(&rank);
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

    /// Joueurs venus ensemble (duo, groupe, voir `find_groups`) et joueurs déjà croisés dans tes 3 dernières parties
    /// (voir `find_met`). Le client va chercher les 20 dernières parties de chaque joueur chez Riot : 2 joueurs par
    /// passage (une par seconde), pour ne pas ralentir la partie.
    async fn check_groups(&mut self, base: &str, password: &str) {
        let Some(game) = &self.game else {
            return;
        };
        let game_id = game.game_id;
        let batch: Vec<Recent> = game.to_check.iter().take(2).cloned().collect();
        if batch.is_empty() {
            return;
        }
        let mut read = Vec::new();
        for mut player in batch {
            let url = format!("{base}/lol-match-history/v1/products/lol/{}/matches?begIndex=0&endIndex=19", player.puuid);
            if let Some(list) = self.get_from_riot(&url, password).await {
                player.games = past_games(&list, &player.puuid, game_id);
            }
            read.push(player);
        }
        let Some(game) = &mut self.game else {
            return;
        };
        game.to_check.drain(..read.len());
        game.checked.extend(read);
        if !game.to_check.is_empty() {
            return;
        }
        game.groups = find_groups(&game.checked);
        let found = find_met(&game.checked, self.puuid.as_deref().unwrap_or_default(), &game.groups);
        // Le champion que chacun jouait alors : son nom.
        let mut met = HashMap::new();
        for (champion_id, (ago, ally, then)) in found {
            met.insert(champion_id, Met { ago, ally, champion: self.champ(base, password, then).await.name });
        }
        let Some(game) = &mut self.game else {
            return;
        };
        game.met = met;
        for player in game.allies.iter_mut().chain(game.enemies.iter_mut()) {
            player.group = game.groups.get(&player.champion_id).copied();
            player.met = game.met.get(&player.champion_id).cloned();
        }
        // Pour vérifier sur une vraie partie : parties lues par joueur, parties jouées ensemble, joueurs croisés.
        let name = |id: i64| game.allies.iter().chain(&game.enemies).find(|p| p.champion_id == id).map_or("?", |p| p.champion.as_str());
        let read: Vec<String> = game.checked.iter().map(|p| format!("{} {}", name(p.champion_id), p.games.len())).collect();
        let mut pairs = Vec::new();
        for (i, a) in game.checked.iter().enumerate() {
            for b in &game.checked[i + 1..] {
                let n = together(&a.games, &b.games);
                if a.ally == b.ally && n > 0 {
                    pairs.push(format!("{} + {} {n}", name(a.champion_id), name(b.champion_id)));
                }
            }
        }
        let met: Vec<String> = game
            .met
            .iter()
            .map(|(id, m)| {
                let relation = if m.ally { "avec toi" } else { "contre toi" };
                format!("{} ({} partie(s) avant, {relation}, en {})", name(*id), m.ago, m.champion)
            })
            .collect();
        let list = |items: Vec<String>| if items.is_empty() { "aucun".to_string() } else { items.join(", ") };
        eprintln!(
            "[groupes] partie {game_id} : parties lues {} ; ensemble dans la même équipe : {} ; déjà croisés : {}",
            read.join(", "),
            list(pairs),
            list(met)
        );
    }

    /// Ton rang (solo/duo et flexible) et tes 20 dernières parties, d'après le client.
    async fn read_profile(&mut self, base: &str, password: &str) -> Option<Profile> {
        let ranked = self.get(&format!("{base}/lol-ranked/v1/current-ranked-stats"), Some(password)).await;
        let url = format!("{base}/lol-match-history/v1/products/lol/current-summoner/matches?begIndex=0&endIndex=19");
        let matches = self.get(&url, Some(password)).await;
        if ranked.is_none() && matches.is_none() {
            return None;
        }
        let games = matches.as_ref().and_then(|m| m["games"]["games"].as_array()).cloned().unwrap_or_default();
        let records = games.iter().filter_map(|g| history::record_from(g, self.puuid.as_deref(), self.summoner_id, 0)).collect();
        let solo = ranked.as_ref().map(|r| rank_in(r, "RANKED_SOLO_5x5"));
        let flex = ranked.as_ref().map(|r| rank_in(r, "RANKED_FLEX_SR"));
        let mut profile = summarize(solo, flex, records);
        for champion in &mut profile.recent_champions {
            champion.name = self.champ(base, password, champion.champion_id).await.name;
        }
        for record in &mut profile.matches {
            record.champion = self.champ(base, password, record.champion_id).await.name;
            // Partie suivie par l'app : on connaît aussi ton adversaire direct, ton poste et ton skin.
            if let Some(known) = self.history.get(record.game_id, &record.puuid) {
                record.position.clone_from(&known.position);
                record.opponent_id = known.opponent_id;
                record.opponent.clone_from(&known.opponent);
                record.lp_change = known.lp_change;
                record.skin_id = known.skin_id;
            }
        }
        Some(profile)
    }

    /// Parties enregistrées avant le rangement par compte : celui qui jouait leur champion, d'après la partie
    /// complète dans le client, est leur compte. Celles introuvables restent sans compte (réessayé au prochain
    /// démarrage du client).
    /// 5 parties par tour ; true quand il n'y en a plus à essayer.
    async fn assign_owners(&mut self, base: &str, password: &str) -> bool {
        // Les premières parties sans compte sont celles déjà essayées en vain (elles gardent leur place).
        let batch: Vec<(i64, i64)> = self.history.unowned().into_iter().skip(self.owners_skip).take(5).collect();
        if batch.is_empty() {
            if self.owners_skip > 0 {
                eprintln!("[historique] {} anciennes parties : compte introuvable pour l'instant", self.owners_skip);
            }
            return true;
        }
        let mut owners = HashMap::new();
        for (game_id, champion_id) in &batch {
            let doc = self.get(&format!("{base}/lol-match-history/v1/games/{game_id}"), Some(password)).await;
            match doc.as_ref().and_then(|doc| history::owner_of(doc, *champion_id)) {
                Some(puuid) => {
                    owners.insert(*game_id, puuid);
                }
                None => self.owners_skip += 1,
            }
        }
        if owners.is_empty() {
            return false;
        }
        match self.history.set_owners(&owners) {
            Ok(()) => {
                eprintln!("[historique] {} anciennes parties rangées sous leur compte", owners.len());
                // Les lignes de l'historique reprennent ce que l'app sait de ces parties (adversaire, LP...).
                self.profile_due = true;
                false
            }
            Err(e) => {
                // Fichier pas réécrit : on réessaiera au prochain démarrage du client.
                eprintln!("[historique] {e}");
                true
            }
        }
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
        if now > finished.give_up || self.history.contains(game.game_id, self.puuid.as_deref().unwrap_or_default()) {
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
        // Classée : LP gagnés ou perdus = tes LP maintenant moins ceux du début de la partie, une fois que
        // ton rang compte la partie (une victoire ou une défaite de plus). D'habitude c'est déjà le cas
        // quand elle arrive dans l'historique ; sinon on l'attend un peu.
        let queue = match record.queue_id {
            420 => Some("RANKED_SOLO_5x5"),
            440 => Some("RANKED_FLEX_SR"),
            _ => None,
        };
        if let (Some(queue), Some(before)) = (queue, game.lp_before.as_ref().filter(|r| ladder(r).is_some())) {
            let stats = self.get(&format!("{base}/lol-ranked/v1/current-ranked-stats"), Some(password)).await;
            let after = stats.map(|stats| rank_in(&stats, queue));
            match after.filter(|a| a.wins + a.losses != before.wins + before.losses) {
                Some(after) => record.lp_change = lp_gap(before, &after),
                None => {
                    if let Some(finished) = &mut self.finished {
                        let until = *finished.lp_until.get_or_insert(now + LP_WAIT);
                        if now < until {
                            finished.next_try = now + RECORD_RETRY;
                            return;
                        }
                    }
                    eprintln!("[historique] partie {} : LP pas encore comptés, enregistrée sans", game.game_id);
                }
            }
        }
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

    /// Comme `get`, pour ce que le client va chercher chez Riot : ça peut prendre plus que les 2 secondes habituelles.
    async fn get_from_riot(&self, url: &str, password: &str) -> Option<Value> {
        let resp = self.http.get(url).basic_auth("riot", Some(password)).timeout(Duration::from_secs(5)).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        resp.json().await.ok()
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
        self.seen_spells = None;
        self.page_set = None;
        self.owners_checked = None;
        self.owners_skip = 0;
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

/// Runes d'une page de runes, triées : pour voir si elle a changé, quel que soit l'ordre donné par le client.
fn sorted_perks(page: &Value) -> Vec<i64> {
    let perks: Vec<i64> = page["selectedPerkIds"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect();
    sorted(&perks)
}

fn sorted(ids: &[i64]) -> Vec<i64> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids
}

/// Sorts d'un joueur de la sélection : [D, F] ; None si le client ne donne pas deux sorts valables.
fn spells_of(cell: &Value) -> Option<[i64; 2]> {
    let spell = |key: &str| cell[key].as_i64().filter(|id| *id > 0);
    Some([spell("spell1Id")?, spell("spell2Id")?])
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

/// Équipe (100 ou 200), champion et début de chacune des dernières parties d'un joueur, sans la partie en cours,
/// d'après la liste du client (qui ne met en général que lui dans chaque partie).
fn past_games(list: &Value, puuid: &str, current: i64) -> HashMap<i64, Past> {
    let games = list["games"]["games"].as_array().into_iter().flatten();
    games
        .filter_map(|g| {
            let game_id = g["gameId"].as_i64().filter(|id| *id != current)?;
            let participants = g["participants"].as_array()?;
            let mut identities = g["participantIdentities"].as_array().into_iter().flatten();
            let number = identities.find(|i| i["player"]["puuid"] == puuid).map(|i| &i["participantId"]);
            let p = match number {
                Some(number) => participants.iter().find(|p| &p["participantId"] == number)?,
                None if participants.len() == 1 => &participants[0],
                None => return None,
            };
            // Sans numéro d'équipe, le résultat les sépare aussi : gagnants d'un côté, perdants de l'autre.
            let team = p["teamId"].as_i64().or_else(|| p["stats"]["win"].as_bool().map(|win| if win { 100 } else { 200 }))?;
            let champion = p["championId"].as_i64().unwrap_or(0);
            Some((game_id, Past { team, champion, start: g["gameCreation"].as_i64().unwrap_or(0) }))
        })
        .collect()
}

/// Parties jouées ensemble dans la même équipe, d'après les dernières parties de chacun.
fn together(a: &HashMap<i64, Past>, b: &HashMap<i64, Past>) -> usize {
    a.iter().filter(|(game, past)| b.get(game).is_some_and(|other| other.team == past.team)).count()
}

/// Joueurs venus ensemble : deux joueurs de la même équipe qui ont joué au moins 2 de leurs 20 dernières parties
/// ensemble, du même côté (une seule peut être un hasard du matchmaking), et ceux liés à eux (groupe de 3 et plus).
/// Numéro de groupe de chacun, par champion : 1, 2… en commençant par ton équipe.
fn find_groups(players: &[Recent]) -> HashMap<i64, u8> {
    // Chacun part dans son propre groupe ; deux joueurs liés mettent leurs groupes en commun.
    let mut label: Vec<usize> = (0..players.len()).collect();
    for (i, a) in players.iter().enumerate() {
        for (j, b) in players.iter().enumerate().skip(i + 1) {
            if a.ally == b.ally && together(&a.games, &b.games) >= 2 {
                let (from, to) = (label[j], label[i]);
                label.iter_mut().filter(|l| **l == from).for_each(|l| *l = to);
            }
        }
    }
    let mut numbers: Vec<usize> = Vec::new();
    let mut groups = HashMap::new();
    let order = (0..players.len()).filter(|i| players[*i].ally).chain((0..players.len()).filter(|i| !players[*i].ally));
    for i in order {
        if label.iter().filter(|l| **l == label[i]).count() < 2 {
            continue;
        }
        let number = match numbers.iter().position(|l| *l == label[i]) {
            Some(k) => k + 1,
            None => {
                numbers.push(label[i]);
                numbers.len()
            }
        };
        groups.insert(players[i].champion_id, number as u8);
    }
    groups
}

/// Joueurs déjà croisés dans tes 3 dernières parties, alliés ou adversaires, sauf ceux venus avec toi. Pour chacun
/// (par champion) : la plus récente de ces parties (1 = ta dernière), s'il était dans ton équipe, son champion.
fn find_met(players: &[Recent], me: &str, groups: &HashMap<i64, u8>) -> HashMap<i64, (u8, bool, i64)> {
    let Some(mine) = players.iter().find(|p| !me.is_empty() && p.puuid == me) else {
        return HashMap::new();
    };
    let mut last: Vec<(&i64, &Past)> = mine.games.iter().collect();
    last.sort_by_key(|(_, past)| std::cmp::Reverse(past.start));
    last.truncate(3);
    let my_group = groups.get(&mine.champion_id);
    players
        .iter()
        .filter(|p| p.puuid != mine.puuid && (my_group.is_none() || groups.get(&p.champion_id) != my_group))
        .filter_map(|p| {
            last.iter().enumerate().find_map(|(k, (game, me_then))| {
                let then = p.games.get(*game)?;
                Some((p.champion_id, (k as u8 + 1, then.team == me_then.team, then.champion)))
            })
        })
        .collect()
}

/// Salon d'un joueur d'après ses rangs et ses 20 dernières parties (sans les parties annulées) : bilan, forme,
/// 3 champions les plus joués (à égalité, dans l'ordre de la liste) et les 10 dernières parties, de la plus
/// récente à la plus ancienne. Les noms des champions restent à remplir.
pub fn summarize(solo: Option<Rank>, flex: Option<Rank>, records: Vec<history::GameRecord>) -> Profile {
    let mut records: Vec<history::GameRecord> = records.into_iter().filter(|r| r.duration >= REMAKE_SECONDS).collect();
    let mut counts: Vec<(i64, u64, u64)> = Vec::new();
    for record in &records {
        match counts.iter_mut().find(|(id, _, _)| *id == record.champion_id) {
            Some((_, played, won)) => {
                *played += 1;
                *won += u64::from(record.win);
            }
            None => counts.push((record.champion_id, 1, u64::from(record.win))),
        }
    }
    counts.sort_by_key(|(_, games, _)| std::cmp::Reverse(*games));
    records.sort_by_key(|r| std::cmp::Reverse(r.date));
    let recent_champions = counts
        .into_iter()
        .take(3)
        .map(|(champion_id, games, wins)| {
            // Nom déjà connu (OP.GG le donne avec chaque partie).
            let name = records.iter().find(|r| r.champion_id == champion_id).map(|r| r.champion.clone()).unwrap_or_default();
            RecentChampion { champion_id, name, games, wins }
        })
        .collect();
    Profile {
        solo,
        flex,
        recent_games: records.len() as u64,
        recent_wins: records.iter().filter(|r| r.win).count() as u64,
        recent_results: records.iter().map(|r| r.win).collect(),
        last_champion_id: records.first().map_or(0, |r| r.champion_id),
        recent_champions,
        matches: records.into_iter().take(MATCHES_SHOWN).collect(),
    }
}

/// Rang d'une file dans une réponse de /lol-ranked (liste « queues », ou table « queueMap »).
/// Pas d'entrée pour cette file, ou tier « NONE » : non classé.
pub fn rank_in(stats: &Value, queue: &str) -> Rank {
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

/// Position d'un rang sur l'échelle classée, en LP depuis Fer IV 0 LP : 400 par rang, 100 par division ;
/// à partir de Maître, les LP s'ajoutent sans division. None hors classement (ou en placement).
fn ladder(rank: &Rank) -> Option<i64> {
    const BELOW_MASTER: [&str; 7] = ["IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND"];
    if matches!(rank.tier.as_str(), "MASTER" | "GRANDMASTER" | "CHALLENGER") {
        return Some(7 * 400 + rank.lp);
    }
    let tier = BELOW_MASTER.iter().position(|t| *t == rank.tier)? as i64;
    let division = ["IV", "III", "II", "I"].iter().position(|d| *d == rank.division)? as i64;
    Some(tier * 400 + division * 100 + rank.lp)
}

/// LP gagnés (ou perdus) d'un rang à l'autre, montées et descentes de division comprises.
/// None si l'un n'est pas classé, ou si l'écart est impossible en une partie (nouvelle saison...).
fn lp_gap(before: &Rank, after: &Rank) -> Option<i64> {
    let gap = ladder(after)? - ladder(before)?;
    (gap.abs() <= 100).then_some(gap)
}

/// Support : l'Atlas en premier objet de départ (OP.GG ne le compte pas dans les départs).
fn with_atlas(mut build: opgg::Build, position: &str) -> opgg::Build {
    if position == "support" && !build.is_empty() && !build.starter.iter().any(|i| i.id == ATLAS) {
        build.starter.insert(0, opgg::Item { id: ATLAS, name: "Atlas".to_string(), count: 1 });
    }
    build
}

/// Panneau en jeu, de haut en bas : départ, 1er objet (et son autre choix), bottes, 2e objet (et son autre choix).
/// Un objet acheté pour de bon (`bought`) n'y est plus ; le 1er et le 2e objet partent aussi si tu as pris
/// leur autre choix. `started` : ton premier achat est fait, la rangée du départ n'a plus lieu d'être.
fn panel(build: &opgg::Build, bought: &HashSet<i64>, started: bool) -> Vec<Vec<PanelItem>> {
    let item = |i: &opgg::Item, size| PanelItem { id: i.id, count: i.count, size };
    // Une étape du build : vide dès qu'un de ses objets est acheté.
    let step = |main: Option<&opgg::Item>, alt: Option<&opgg::Item>, size| -> Vec<PanelItem> {
        if [main, alt].into_iter().flatten().any(|i| bought.contains(&i.id)) {
            return Vec::new();
        }
        main.map(|m| item(m, size)).into_iter().chain(alt.map(|a| item(a, "alt"))).collect()
    };
    let starter = if started { Vec::new() } else { build.starter.iter().map(|i| item(i, "starter")).collect() };
    [
        starter,
        step(build.core.first(), build.first_alt.as_ref(), "main"),
        step(build.boots.first(), None, "boots"),
        step(build.core.get(1), build.second_alt.as_ref(), "main"),
    ]
    .into_iter()
    .filter(|row| !row.is_empty())
    .collect()
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
