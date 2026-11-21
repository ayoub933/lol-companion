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
/// Bans conseillés : au plus 2 champions qui battent le tien, complétés par les plus forts de ton poste.
const BANS_SHOWN: usize = 3;
const BAN_COUNTERS: usize = 2;
/// Classement d'OP.GG (tier list) : relu toutes les 3 heures, ou 5 minutes après un échec.
const META_REFRESH: Duration = Duration::from_secs(3 * 3600);
const META_RETRY: Duration = Duration::from_secs(5 * 60);
/// Après une partie, ses stats arrivent dans l'historique du client avec un peu de retard.
const RECORD_RETRY: Duration = Duration::from_secs(5);
const RECORD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// Partie classée dans l'historique mais pas encore comptée dans ton rang : on attend au plus ça ses LP.
const LP_WAIT: Duration = Duration::from_secs(3 * 60);
/// Objet gardé ce temps-là après son achat : acheté pour de bon, il reste grisé dans le panneau même vendu ou
/// utilisé ensuite (un achat annulé tout de suite n'y compte pas).
const BOUGHT_AFTER: Duration = Duration::from_secs(30);
/// Illustrations de skins gardées en mémoire (les plus récentes).
const SPLASH_CACHE: usize = 6;
/// Objets à Blessures graves connus (si leur description ne le dit pas dans la langue du client) : Appel du
/// bourreau, Rappel mortel, Glaive chimiotech, Orbe d'oubli, Morellonomicon, Gilet épineux, Cotte épineuse.
const GRIEVOUS: [i64; 7] = [3123, 3033, 6609, 3916, 3165, 3076, 3075];
/// Champions aux gros soins : contre eux, un objet à Blessures graves.
const HEALERS: [i64; 20] = [16, 350, 266, 8, 517, 36, 19, 50, 233, 420, 267, 37, 902, 9, 2, 106, 48, 147, 57, 154];
/// Kayn : deux formes (rouge, bleue), chacune ses runes et son build.
const KAYN: i64 = 141;
/// Pour les formes de Kayn sans adversaire connu : son duel contre Lee Sin (jungler très joué) sert de référence.
const LEE_SIN: i64 = 64;
/// Voie de runes Précision (celle du Conquérant) : la page de Kayn rouge.
const PRECISION: i64 = 8000;

/// Forme de Kayn : rouge (Rhaast, le darkin : combattant) ou bleue (l'assassin de l'ombre).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Form {
    Red,
    Blue,
}

impl Form {
    pub fn parse(text: &str) -> Option<Form> {
        match text {
            "red" => Some(Form::Red),
            "blue" => Some(Form::Blue),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Form::Red => "red",
            Form::Blue => "blue",
        }
    }
}

/// Fichiers du client en anglais (objets, runes, sorts), repris par CommunityDragon : réglage « noms en anglais ».
const ENGLISH_DATA: &str = "https://raw.communitydragon.org/latest/plugins/rcp-be-lol-game-data/global/default/v1";
/// Noms anglais pas encore lus (CommunityDragon injoignable) : nouvel essai au plus une fois par minute.
const ENGLISH_RETRY: Duration = Duration::from_secs(60);

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
    /// Sa place dans le classement d'OP.GG, à son poste (absent tant que le classement n'est pas lu).
    pub meta: Option<ChampionMeta>,
}

/// Un champion dans le classement d'OP.GG d'un poste.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChampionMeta {
    /// "top", "jungle", "mid", "adc" ou "support".
    pub position: String,
    /// 1 (le meilleur) à 5.
    pub tier: u8,
    /// Sa place parmi les `ranked` champions classés à ce poste, et au patch précédent (0 : inconnue).
    pub rank: u32,
    pub ranked: u32,
    pub rank_prev_patch: u32,
    pub win_rate: f64,
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
    /// Leurs numéros (pour leurs noms anglais).
    #[serde(skip)]
    pub applied_spells: Vec<i64>,
    /// Pourquoi les sorts n'ont pas été changés.
    pub spells_note: Option<String>,
    /// Aide au choix, tant que ton champion n'est pas verrouillé.
    pub help: Option<PickHelp>,
    /// Bans conseillés, tant que ton ban n'est pas fait.
    pub bans: Option<BanHelp>,
    /// Ton bilan contre ton adversaire direct, d'après l'historique de l'app.
    pub duel_games: u64,
    pub duel_wins: u64,
    /// Skin choisi pour ton champion (numéro du client, ex. 103015) ; 0 si inconnu.
    pub skin_id: i64,
    /// true : champion seulement cliqué (pas verrouillé) ; ses runes et son build sont montrés, rien n'est importé.
    pub preview: bool,
    /// true : champion à deux formes (Kayn), à choisir.
    pub forms: bool,
    /// Sa forme retenue : "red", "blue" ; vide : pas encore choisie ni conseillée (build général, formes mélangées).
    pub form: String,
    /// true : forme choisie d'un clic (sinon conseillée d'après l'équipe d'en face).
    pub form_manual: bool,
    /// Champions de mêlée et à distance déjà choisis en face (ce qui donne les orbes rouges ou bleues).
    pub enemy_melee: usize,
    pub enemy_ranged: usize,
    /// Dégâts de ton équipe (champions choisis ou survolés) et de celle d'en face.
    pub ally_damage: TeamDamage,
    pub enemy_damage: TeamDamage,
}

/// Dégâts d'une équipe en sélection, d'après la fiche de chaque champion dans le client : physiques ou magiques (un
/// champion aux dégâts mixtes compte pour moitié de chaque côté), et ses tanks.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TeamDamage {
    pub physical: f64,
    pub magic: f64,
    pub tanks: u32,
    /// Champions comptés.
    pub champions: u32,
}

impl TeamDamage {
    fn of<'a>(champs: impl IntoIterator<Item = &'a opgg::Champ>) -> Self {
        let mut team = Self::default();
        for champ in champs {
            match champ.damage.as_str() {
                "kPhysical" => team.physical += 1.0,
                "kMagic" => team.magic += 1.0,
                "kMixed" => {
                    team.physical += 0.5;
                    team.magic += 0.5;
                }
                _ => {}
            }
            team.tanks += u32::from(champ.roles.iter().any(|r| r == "tank"));
            team.champions += 1;
        }
        team
    }
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

/// Pendant les bans : qui bannir, d'abord contre ton champion (survolé ou choisi), puis d'après le classement
/// d'OP.GG de ton poste.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BanHelp {
    /// Ton champion ; vide tant que tu n'en as pas survolé.
    pub champion: String,
    /// Ton poste ("top", "jungle", "mid", "adc", "support").
    pub position: String,
    pub bans: Vec<BanPick>,
    /// true tant qu'une réponse d'OP.GG manque encore.
    pub pending: bool,
}

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BanPick {
    pub champion_id: i64,
    pub name: String,
    /// true : il bat ton champion (`win_rate` = ses victoires contre lui) ; false : parmi les plus forts de ton
    /// poste (`win_rate` = ses victoires à ce poste, `tier` son tier).
    pub counter: bool,
    pub win_rate: f64,
    pub games: u64,
    pub tier: u8,
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
    /// Poste deviné (au sens du client : "top", "jungle", "middle", "bottom", "utility") ; vide sans postes.
    pub position: String,
    /// Part de ses parties à ce poste sur OP.GG (0 à 1).
    pub position_rate: f64,
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
    /// Noms anglais des objets, runes et sorts (réglage « noms en anglais »), lus une fois sur CommunityDragon.
    english: Arc<Mutex<English>>,
}

/// Noms anglais par numéro, et quand ils ont été demandés.
#[derive(Default)]
struct English {
    items: HashMap<i64, String>,
    /// Runes et voies de runes.
    perks: HashMap<i64, String>,
    spells: HashMap<i64, String>,
    asked: Option<Instant>,
}

/// Rangs dont le client a un emblème (images/ranked-emblem/emblem-<rang>.png).
const TIERS: [&str; 10] =
    ["iron", "bronze", "silver", "gold", "platinum", "emerald", "diamond", "master", "grandmaster", "challenger"];

/// Illustration d'un skin : (numéro du skin, image).
type Splash = (i64, Vec<u8>);

#[derive(Clone)]
struct ItemData {
    /// Nom dans la langue du client.
    name: String,
    /// Chemin de l'icône dans le client.
    icon: String,
    /// Composants (pour savoir ce qu'il reste à payer quand tu en as déjà).
    from: Vec<i64>,
    /// Prix total dans la boutique (0 pour les balises, données gratuitement).
    price: u64,
    /// Catégories de la boutique : "Boots", "Armor", "SpellBlock", "ArmorPenetration", "MagicPenetration"…
    tags: Vec<String>,
    /// Applique Blessures graves (réduit les soins des ennemis).
    grievous: bool,
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
            english: Arc::default(),
        }
    }

    /// Réglage « noms en anglais » : runes, objets et sorts de la sélection renommés d'après leurs numéros. Les noms
    /// sont demandés à CommunityDragon la première fois, sans attendre : d'ici là (ou s'il ne répond pas), ceux du
    /// client et d'OP.GG restent.
    pub fn in_english(&self, runes: &mut RuneImport) {
        self.fetch_english();
        let english = self.english.lock().unwrap();
        let rename = |names: &HashMap<i64, String>, id: i64, name: &mut String| {
            if let Some(found) = names.get(&id) {
                name.clone_from(found);
            }
        };
        rename(&english.perks, runes.keystone_id, &mut runes.keystone);
        rename(&english.perks, runes.primary_id, &mut runes.primary);
        rename(&english.perks, runes.secondary_id, &mut runes.secondary);
        // Sorts : seulement si tous sont connus en anglais (pas de « Flash + Embrasement »).
        let spells = |ids: &[i64]| -> Option<Vec<String>> { ids.iter().map(|id| english.spells.get(id).cloned()).collect() };
        if let Some(build) = &mut runes.build {
            let items = build.starter.iter_mut().chain(&mut build.core).chain(&mut build.boots).chain(&mut build.options);
            for item in items.chain(&mut build.first_alt).chain(&mut build.second_alt) {
                rename(&english.items, item.id, &mut item.name);
            }
            if let Some(names) = spells(&build.spells) {
                runes.spells = names;
            }
        }
        if let Some(names) = spells(&runes.applied_spells) {
            runes.spells_applied = names;
        }
    }

    /// Demande les noms anglais à CommunityDragon sans attendre la réponse : une fois, ou de nouveau une minute
    /// après un échec.
    fn fetch_english(&self) {
        {
            let mut english = self.english.lock().unwrap();
            let complete = !english.items.is_empty() && !english.perks.is_empty() && !english.spells.is_empty();
            if complete || english.asked.is_some_and(|at| at.elapsed() < ENGLISH_RETRY) {
                return;
            }
            english.asked = Some(Instant::now());
        }
        let assets = self.clone();
        tauri::async_runtime::spawn(async move {
            // Adresse remplaçable pour les tests (faux CommunityDragon).
            let base = std::env::var("ENGLISH_DATA_URL").unwrap_or_else(|_| ENGLISH_DATA.to_string());
            let items = assets.english_file(&base, "items.json").await;
            let mut perks = assets.english_file(&base, "perks.json").await;
            perks.extend(assets.english_file(&base, "perkstyles.json").await);
            let spells = assets.english_file(&base, "summoner-spells.json").await;
            eprintln!("[noms] noms anglais lus : {} objets, {} runes, {} sorts", items.len(), perks.len(), spells.len());
            // Ce qui a été lu sert tout de suite ; une liste manquante sera redemandée.
            let mut guard = assets.english.lock().unwrap();
            let english = &mut *guard;
            for (names, found) in [(&mut english.items, items), (&mut english.perks, perks), (&mut english.spells, spells)] {
                if !found.is_empty() {
                    *names = found;
                }
            }
        });
    }

    /// Numéro → nom, d'un fichier de CommunityDragon (une liste, ou { "styles": [...] } pour les voies de runes) ;
    /// vide s'il ne répond pas.
    async fn english_file(&self, base: &str, file: &str) -> HashMap<i64, String> {
        let doc = match self.web.get(format!("{base}/{file}")).timeout(Duration::from_secs(20)).send().await {
            Ok(resp) if resp.status().is_success() => resp.json::<Value>().await.unwrap_or(Value::Null),
            Ok(resp) => {
                eprintln!("[noms] {file} : réponse {}", resp.status());
                Value::Null
            }
            Err(e) => {
                eprintln!("[noms] {file} injoignable : {e}");
                Value::Null
            }
        };
        let entries = doc.as_array().or(doc["styles"].as_array()).into_iter().flatten();
        // Quelques objets spéciaux ont un nom mis en forme (« <rarityLegendary>… ») : laissés de côté.
        let named = |e: &Value| {
            let name = e["name"].as_str().filter(|n| !n.is_empty() && !n.contains('<'))?;
            Some((e["id"].as_i64()?, name.to_string()))
        };
        entries.filter_map(named).collect()
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
                let id = it["id"].as_i64()?;
                let price = it["priceTotal"].as_u64().unwrap_or(0);
                let tags = it["categories"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
                // Blessures graves : dit dans sa description (client en français ou en anglais), ou objet connu.
                let text = it["description"].as_str().unwrap_or_default().to_lowercase();
                let grievous = GRIEVOUS.contains(&id) || text.contains("blessures graves") || text.contains("grievous wounds");
                let name = it["name"].as_str().unwrap_or_default().to_string();
                let from = it["from"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect();
                Some((id, ItemData { name, icon: it["iconPath"].as_str()?.to_string(), from, price, tags, grievous }))
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

    /// L'objet est dans cette catégorie de la boutique (« Armor », « SpellBlock »…).
    fn has_tag(&self, id: i64, tag: &str) -> bool {
        self.items.lock().unwrap().get(&id).is_some_and(|i| i.tags.iter().any(|t| t == tag))
    }

    /// L'objet applique Blessures graves.
    fn grievous(&self, id: i64) -> bool {
        self.items.lock().unwrap().get(&id).is_some_and(|i| i.grievous)
    }

    /// Bottes améliorées (pas les bottes de base, à 300 pièces d'or).
    fn upgraded_boots(&self, id: i64) -> bool {
        self.has_tag(id, "Boots") && self.price(id) > 300
    }

    /// Nom d'un objet : en anglais si demandé (et déjà lu), sinon celui du client.
    fn item_name(&self, id: i64, english: bool) -> String {
        if english {
            self.fetch_english();
            if let Some(name) = self.english.lock().unwrap().items.get(&id) {
                return name.clone();
            }
        }
        self.items.lock().unwrap().get(&id).map(|i| i.name.clone()).unwrap_or_default()
    }

    /// Ce qu'il reste à payer pour un objet avec ton inventaire (ses composants que tu as, et les leurs, déduits
    /// comme dans la boutique), et ses composants avec, pour chacun, s'il est dans ton inventaire.
    fn recipe(&self, id: i64, inventory: &[i64]) -> (u64, Vec<(i64, bool)>) {
        let items = self.items.lock().unwrap();
        let Some(item) = items.get(&id) else {
            return (0, Vec::new());
        };
        let mut left_over = inventory.to_vec();
        let mut have = 0;
        let mut parts = Vec::new();
        for part in &item.from {
            parts.push((*part, left_over.contains(part)));
            have += owned_value(&items, *part, &mut left_over);
        }
        (item.price.saturating_sub(have), parts)
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
    /// Panneau posé sur le jeu : les objets à acheter (ceux du duel contre ton adversaire de couloir), une rangée
    /// par étape (voir `panel`), ceux déjà achetés grisés. Vide : rien à montrer.
    pub panel: Vec<PanelRow>,
    /// Le même build en détail, pour l'écran de l'app ; None sans build.
    pub build: Option<GameBuild>,
}

/// Ce que l'écran de l'app ajoute au panneau posé sur le jeu.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GameBuild {
    /// Adversaire du duel dont viennent les objets ; vide : ceux de ton champion à ton poste.
    pub versus: String,
    /// Forme de Kayn dont viennent les objets : "red", "blue" ; vide pour les autres champions.
    pub form: String,
    /// Prochain achat ; None quand tout le build est acheté.
    pub next: Option<NextBuy>,
    /// Ordre de montée des compétences, ex. ["Q", "E", "W"].
    pub skills: Vec<String>,
    /// Ce que l'app voit en face, ex. « 4 AD · 1 AP · 2 tanks · soins : Soraka ».
    pub enemy_note: String,
    /// Conseil d'OP.GG contre ton adversaire de couloir (vide sans duel).
    pub tip: DuelTip,
}

/// Conseil d'OP.GG pour un duel : style de jeu ("defensive", "aggressive", "even") et astuce (en anglais : OP.GG ne
/// la traduit pas).
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DuelTip {
    pub versus: String,
    pub play_style: String,
    pub text: String,
}

/// Le prochain achat : les objets de départ, ou un objet avec ce qu'il coûte encore et ses composants.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NextBuy {
    pub items: Vec<PanelItem>,
    /// Prix total dans la boutique.
    pub price: u64,
    /// Ce qu'il reste à payer : ses composants que tu as déjà sont déduits.
    pub left: u64,
    /// Ses composants (un composant que tu as est `owned`).
    pub components: Vec<PanelItem>,
}

/// Une rangée du panneau posé sur le jeu.
#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PanelRow {
    /// « 1. » à « 5. » : le build dans l'ordre d'achat ; « + » : la suite ; « VS » : contre l'équipe d'en face.
    pub label: String,
    pub items: Vec<PanelItem>,
    /// Étape faite (un de ses objets acheté) : grisée.
    pub done: bool,
    /// La prochaine étape à acheter : mise en avant.
    pub next: bool,
}

/// Un objet du panneau posé sur le jeu.
#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PanelItem {
    pub id: i64,
    /// Ex. 2 pour deux potions de départ.
    pub count: u32,
    /// Taille de l'icône : "starter", "main", "boots", "alt" (autre choix possible) ou "option" (suite, et
    /// contre l'équipe d'en face).
    pub size: &'static str,
    /// Dans ton inventaire, ou acheté pendant la partie : grisé, avec une coche.
    pub owned: bool,
    /// Pourquoi il est proposé contre l'équipe d'en face : "soins", "AD", "AP" ou "tanks" ; vide sinon.
    pub reason: &'static str,
    /// Pour l'écran de l'app : nom (dans la langue choisie) et prix dans la boutique.
    pub name: String,
    pub price: u64,
    /// Parties et victoires OP.GG de ce choix (choix suivants et objets contre l'équipe d'en face) ; 0 sinon.
    pub games: u64,
    pub wins: u64,
}

/// Ce que l'équipe d'en face demande, et ton champion pour y répondre.
#[derive(Default)]
struct Needs {
    /// Ce que l'app voit en face (pour l'écran de l'app), ex. « 4 AD · 1 AP · 2 tanks · soins : Soraka ».
    note: String,
    /// Au moins un gros soigneur en face.
    anti_heal: bool,
    /// Dégâts physiques (ou magiques) en large majorité en face.
    armor: bool,
    magic_resist: bool,
    /// Au moins 2 tanks en face : la pénétration qui va avec tes dégâts.
    penetration: Option<&'static str>,
    /// Ton composant à Blessures graves si OP.GG n'en propose pas : selon tes dégâts (ou si tu es tank).
    grievous_component: i64,
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
    /// Riot ID (« Nom#TAG ») et compte, pour ouvrir son profil d'un clic ; vides si le client le cache (mode
    /// streamer) ou ne les donne pas.
    pub riot_id: String,
    pub puuid: String,
    /// Sa série en cours et son expérience sur son champion, d'après ses dernières parties ; None tant qu'elles
    /// ne sont pas lues (ou s'il est en mode streamer).
    pub recent: Option<RecentForm>,
    /// Smurf probable, d'après son rang dans la file de la partie et le niveau de son compte (voir `smurf_like`).
    pub smurf: bool,
    /// Niveau de son compte ; None tant qu'il n'est pas lu.
    pub account_level: Option<u64>,
}

/// Ce que ses dernières parties disent d'un joueur (lues pendant le chargement, comme pour les duos).
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecentForm {
    /// Parties lues (20 au plus, sans celles annulées).
    pub games: u32,
    /// Série en cours : 3 = 3 victoires d'affilée, −4 = 4 défaites d'affilée.
    pub streak: i32,
    /// Ses parties et victoires avec le champion qu'il joue.
    pub champion_games: u32,
    pub champion_wins: u32,
    /// OTP (« one-trick ») : il joue surtout ce champion, au moins 60 % de ses dernières parties (10 lues au moins).
    pub otp: bool,
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
    /// Victoire ; None si le client ne le dit pas.
    win: Option<bool>,
    /// Durée en secondes (une partie annulée dure moins de 5 minutes).
    seconds: u64,
}

/// Un joueur de la partie et ses dernières parties : pour les groupes et les joueurs déjà croisés.
#[derive(Clone, Default)]
struct Recent {
    ally: bool,
    champion_id: i64,
    puuid: String,
    /// Ses 20 dernières parties (sans celle en cours), par numéro de partie.
    games: HashMap<i64, Past>,
    /// Niveau de son compte (pour repérer un smurf).
    level: Option<u64>,
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
    build_for: Option<(i64, String, Option<i64>, Option<Form>)>,
    /// Adversaire du duel dont vient ce build ; vide : build de ton champion à ton poste.
    build_versus: String,
    /// Conseil d'OP.GG contre ton adversaire de couloir.
    tip: DuelTip,
    /// Forme de Kayn retenue en sélection (son build en partie suit).
    form: Option<Form>,
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
    /// Fiche rapide de chaque joueur dont les parties sont lues, par champion.
    forms: HashMap<i64, RecentForm>,
    /// Niveau du compte de chaque joueur lu, par champion.
    levels: HashMap<i64, u64>,
    /// Compte de chaque joueur que le client donne, par champion.
    puuids: HashMap<i64, String>,
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
    last_pick: Option<(i64, String, Option<i64>, Option<Form>)>,
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
    /// Aperçu du champion cliqué mais pas verrouillé (rien d'importé), même forme que `rune_import`.
    rune_preview: Option<(ImportKey, RuneImport)>,
    /// Réponses OP.GG déjà récupérées, pour ne pas redemander à chaque partie.
    cache: HashMap<(i64, String, String), opgg::Analysis>,
    /// Réponses d'OP.GG pour les duels (lues selon la forme de Kayn si besoin).
    matchups: HashMap<(i64, i64, String), Value>,
    /// Part des parties d'un champion à chaque poste (au sens d'OP.GG), selon OP.GG ; vide si OP.GG n'a pas répondu.
    roles: HashMap<i64, Vec<(String, f64)>>,
    /// Noms des champions, lus une fois dans le client.
    champs: HashMap<i64, opgg::Champ>,
    /// Numéro de chaque champion d'après son nom interne en minuscules (« monkeyking » → 62).
    champion_ids: HashMap<String, i64>,
    /// Tous les champions du client : (numéro, nom interne, nom affiché), lus avec `champion_ids`.
    champion_list: Vec<(i64, String, String)>,
    /// Classement d'OP.GG de chaque poste, et quand le relire (None : à lire dès que possible).
    meta: Option<HashMap<String, Vec<opgg::MetaChamp>>>,
    meta_due: Option<Instant>,
    /// Noms des sorts d'invocateur, lus une fois dans le client.
    spell_names: HashMap<i64, String>,
    /// Dernière ligne écrite dans le terminal par sujet, pour ne pas la répéter.
    last_log: HashMap<&'static str, String>,
}

/// Ce qui déclenche un nouvel import : (champion, poste, mode, adversaire direct).
/// (champion, poste, mode, adversaire, forme de Kayn).
type ImportKey = (i64, String, String, Option<i64>, Option<Form>);

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
            rune_preview: None,
            cache: HashMap::new(),
            matchups: HashMap::new(),
            roles: HashMap::new(),
            champs: HashMap::new(),
            champion_ids: HashMap::new(),
            champion_list: Vec::new(),
            meta: None,
            meta_due: None,
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

    /// `manual` = adversaire direct choisi d'un clic dans l'interface ; `form` : forme de Kayn choisie d'un clic.
    pub async fn poll(&mut self, manual: Option<i64>, form: Option<Form>, settings: &Settings) -> Snapshot {
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
        // Classement d'OP.GG pour tes champions du moment, hors sélection et partie (en sélection, il sert aux bans).
        let lobby = !matches!(phase.as_str(), "ChampSelect" | "GameStart" | "InProgress" | "Reconnect");
        if lobby && self.profile.is_some() {
            self.lane_meta(&base, &password).await;
        }
        if phase_changed && phase == "ChampSelect" {
            self.last_pick = None;
        }
        self.last_phase = Some(phase.clone());

        let runes = if phase == "ChampSelect" {
            let mut runes = self.champ_select_runes(&base, &password, manual, form).await;
            // Seulement à l'affichage : un changement du réglage se voit tout de suite.
            if settings.english_names {
                self.assets.in_english(&mut runes);
            }
            Some(runes)
        } else {
            self.rune_import = None;
            self.rune_preview = None;
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
            profile: self.profile.clone().map(|mut profile| {
                self.rate_champions(&mut profile);
                profile
            }),
            runes,
            game,
            history: self.history.overview(self.puuid.as_deref()),
            recording: self.finished.is_some(),
            update: None,
        }
    }

    /// En sélection : dès que ton champion est verrouillé, importe runes et build OP.GG,
    /// puis ceux du duel contre ton adversaire direct dès qu'il est connu.
    /// `picked_form` : forme de Kayn choisie d'un clic.
    async fn champ_select_runes(&mut self, base: &str, password: &str, manual: Option<i64>, picked_form: Option<Form>) -> RuneImport {
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
        // Postes adverses et adversaire direct : seulement quand le client attribue des postes (classée, normale
        // avec postes...).
        let laning = !assigned.is_empty() && !position.is_empty();
        let chosen = manual.and_then(|id| enemies.iter().find(|e| e.id == id)).cloned();
        let (lanes, guessed) = if laning {
            self.enemy_roles(&enemies, position, mode, chosen.as_ref().map(|c| c.id)).await
        } else {
            (Vec::new(), None)
        };
        let (opponent, opponent_manual) = match chosen {
            _ if !laning => (None, false),
            Some(chosen) => (Some(chosen), true),
            None => (guessed, false),
        };

        let current = self.get(&format!("{base}/lol-champ-select/v1/current-champion"), Some(password)).await;
        let champion_id = locked_champion(current.as_ref(), &session);
        // Champion cliqué dans la grille, pas encore verrouillé : on montre ses runes et son build.
        let hovered = allies.iter().find(|a| a.me && !a.locked).map(|a| a.champion_id).filter(|id| *id > 0);
        let shown = if champion_id > 0 { champion_id } else { hovered.unwrap_or(0) };
        // Kayn : sa forme choisie d'un clic, sinon conseillée d'après l'équipe d'en face (les mêlées donnent les
        // orbes rouges, les champions à distance les bleues).
        let ranged = enemies.iter().filter(|e| e.ranged).count();
        let melee = enemies.len() - ranged;
        let suggested = match melee.cmp(&ranged) {
            std::cmp::Ordering::Greater => Some(Form::Red),
            std::cmp::Ordering::Less => Some(Form::Blue),
            std::cmp::Ordering::Equal => None,
        };
        let forms = shown == KAYN;
        let form = if forms { picked_form.or(suggested) } else { None };
        let mut result = if champion_id <= 0 {
            self.log("runes", format!(
                "[runes] en attente du verrouillage (current-champion = {}, session lue = {}, survolé = {})",
                current.map(|v| v.to_string()).unwrap_or_else(|| "erreur/404".into()),
                !session.is_null(),
                hovered.unwrap_or(0)
            ));
            match hovered {
                Some(id) => self.preview(base, password, id, position, mode, opponent.as_ref(), form).await,
                None => RuneImport::default(),
            }
        } else {
            self.log("runes", format!(
                "[runes] champion {champion_id}, poste client \"{assigned}\", file {}/{}, carte {map_id} -> OP.GG poste \"{position}\", mode \"{mode}\"",
                queue["id"], queue["gameMode"]
            ));
            self.locked_import(base, password, champion_id, position, mode, map_id, opponent.as_ref(), form).await
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
        // Phase des bans : qui bannir, tant que ton ban n'est pas fait.
        if laning && my_ban_pending(&session) {
            let mut avoid = taken_champions(&session, &enemies);
            avoid.extend(allies.iter().map(|a| a.champion_id).filter(|id| *id > 0));
            let mine = (shown > 0).then_some(shown);
            result.bans = Some(self.ban_help(base, password, mine, position, mode, &avoid).await);
        }
        // Ton bilan dans ce duel, pour le champion verrouillé ou cliqué.
        if let Some(id) = opponent.as_ref().map(|o| o.id).filter(|_| shown > 0) {
            (result.duel_games, result.duel_wins) = self.history.versus(self.puuid.as_deref(), shown, id);
        }
        if champion_id > 0 {
            self.last_pick = Some((champion_id, position.to_string(), opponent.as_ref().map(|o| o.id), form));
        }
        result.forms = forms;
        result.form = form.map_or("", Form::name).to_string();
        result.form_manual = forms && picked_form.is_some();
        (result.enemy_melee, result.enemy_ranged) = (melee, ranged);
        // Dégâts des deux équipes (les tiens survolés compris : on voit l'effet d'un choix avant de le verrouiller).
        let mut ally_champs = Vec::new();
        for ally in allies.iter().filter(|a| a.champion_id > 0) {
            ally_champs.push(self.champ(base, password, ally.champion_id).await);
        }
        result.ally_damage = TeamDamage::of(&ally_champs);
        result.enemy_damage = TeamDamage::of(&enemies);
        result.laning = laning;
        result.skin_id = my_cell(&session).and_then(|p| p["selectedSkinId"].as_i64()).unwrap_or(0);
        result.allies = allies;
        result.enemies = enemies
            .iter()
            .map(|e| {
                let lane = lanes.iter().find(|(id, _, _)| *id == e.id);
                Enemy {
                    id: e.id,
                    name: e.name.clone(),
                    position: lane.map_or("", |(_, lane, _)| client_lane(lane)).to_string(),
                    position_rate: lane.map_or(0.0, |(_, _, rate)| *rate),
                }
            })
            .collect();
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
        form: Option<Form>,
    ) -> RuneImport {
        let me = self.champ(base, password, champion_id).await;
        let key: ImportKey = (champion_id, position.to_string(), mode.to_string(), opponent.as_ref().map(|o| o.id), form);
        match self.rune_import.clone() {
            // Déjà fait pour ce champion, ce poste, ce mode et cet adversaire.
            Some((done, result)) if done == key && !result.pending => result,
            // Recherche annoncée au tour précédent, ou déjà faite pour l'aperçu : on importe maintenant.
            Some((done, _)) if done == key => {
                let result = self.import(base, password, &me, position, mode, map_id, opponent, form, true).await;
                self.rune_import = Some((key, result.clone()));
                result
            }
            _ if self.rune_preview.as_ref().is_some_and(|(seen, r)| *seen == key && !r.pending) => {
                let result = self.import(base, password, &me, position, mode, map_id, opponent, form, true).await;
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

    /// Champion cliqué mais pas verrouillé : ses runes, son build et le duel, sans rien importer. Même rythme que
    /// l'import : « Recherche… » d'abord, OP.GG au tour suivant (une fois par champion, poste, mode et adversaire).
    #[allow(clippy::too_many_arguments)]
    async fn preview(
        &mut self,
        base: &str,
        password: &str,
        champion_id: i64,
        position: &str,
        mode: &str,
        opponent: Option<&opgg::Champ>,
        form: Option<Form>,
    ) -> RuneImport {
        let me = self.champ(base, password, champion_id).await;
        let key: ImportKey = (champion_id, position.to_string(), mode.to_string(), opponent.map(|o| o.id), form);
        match self.rune_preview.clone() {
            Some((done, result)) if done == key && !result.pending => result,
            Some((done, _)) if done == key => {
                let result = self.import(base, password, &me, position, mode, 0, opponent, form, false).await;
                self.rune_preview = Some((key, result.clone()));
                result
            }
            _ => {
                let pending = RuneImport {
                    champion: me.name.clone(),
                    champion_id,
                    position: position.to_string(),
                    pending: true,
                    preview: true,
                    ..Default::default()
                };
                self.rune_preview = Some((key, pending.clone()));
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

    /// Poste de chaque adversaire, et ton adversaire direct deviné. Postes : la répartition la plus probable d'après
    /// la part des parties de chaque champion à chaque poste sur OP.GG, deux jamais au même poste ; l'adversaire
    /// choisi d'un clic (`chosen`) est à ton poste. Adversaire deviné : celui placé à ton poste, s'il y joue au moins
    /// 15 % de ses parties (ou dès que les 5 sont connus). Une seule question à OP.GG par tour, pour que l'interface
    /// reste réactive.
    async fn enemy_roles(
        &mut self,
        enemies: &[opgg::Champ],
        position: &str,
        mode: &str,
        chosen: Option<i64>,
    ) -> (Vec<(i64, &'static str, f64)>, Option<opgg::Champ>) {
        if let Some(enemy) = enemies.iter().find(|e| !self.roles.contains_key(&e.id)).cloned() {
            let shares = match self.opgg.positions(&enemy, position, mode).await {
                Ok(list) => list,
                Err(e) => {
                    eprintln!("[postes] postes de {} introuvables : {e}", enemy.name);
                    Vec::new()
                }
            };
            let text: Vec<String> = shares.iter().map(|(p, rate)| format!("{p} {:.0} %", rate * 100.0)).collect();
            eprintln!("[postes] {} : {}", enemy.name, text.join(", "));
            self.roles.insert(enemy.id, shares);
        }
        // Seulement ceux dont OP.GG a donné les postes (et celui choisi d'un clic) : les autres attendent sans poste,
        // pour que les adversaires ne changent pas de place à chaque réponse.
        let known = |e: &&opgg::Champ| Some(e.id) == chosen || self.roles.get(&e.id).is_some_and(|shares| !shares.is_empty());
        let ids: Vec<i64> = enemies.iter().filter(known).map(|e| e.id).collect();
        let lanes = assign_roles(&ids, &self.roles, chosen.map(|id| (id, position)));
        let name = |id: i64| enemies.iter().find(|e| e.id == id).map_or("?", |e| e.name.as_str());
        let text: Vec<String> = lanes.iter().map(|(id, lane, _)| format!("{} {lane}", name(*id))).collect();
        self.log("postes", format!("[postes] en face : {}", text.join(", ")));
        let complete = enemies.len() == POSITIONS.len() && enemies.iter().all(|e| self.roles.contains_key(&e.id));
        let guessed = lanes
            .iter()
            .find(|(_, lane, rate)| *lane == position && (*rate >= MIN_ROLE_RATE || complete))
            .and_then(|(id, _, _)| enemies.iter().find(|e| e.id == *id).cloned());
        (lanes, guessed)
    }

    /// Runes, build et set d'objets : ceux du duel si l'adversaire est connu (et les parties assez nombreuses),
    /// sinon les recommandations générales du champion. `form` (Kayn) : ceux de sa forme, pris dans le duel, sinon
    /// dans son duel contre Lee Sin (l'analyse générale d'OP.GG mélange les deux formes).
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
        form: Option<Form>,
        apply: bool,
    ) -> RuneImport {
        let mut result = RuneImport {
            champion: me.name.clone(),
            champion_id: me.id,
            position: position.to_string(),
            preview: !apply,
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
            match self.matchup(me, opponent, position, form).await {
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
        // Kayn sans duel utilisable : sa forme d'après son duel contre Lee Sin.
        if let Some(form) = form.filter(|_| versus.is_empty()) {
            let reference = self.champ(base, password, LEE_SIN).await;
            match self.matchup(me, &reference, position, Some(form)).await {
                Ok(m) if m.analysis.runes.games > 0 && !m.analysis.build.is_empty() => {
                    eprintln!("[forme] {} {} : page {} ({} parties contre Lee Sin)", me.name, form.name(), m.analysis.runes.keystone, m.analysis.runes.games);
                    chosen = m.analysis;
                }
                Ok(_) => eprintln!("[forme] {} {} : rien contre Lee Sin, runes générales gardées", me.name, form.name()),
                Err(e) => eprintln!("[forme] duel de référence introuvable : {e}"),
            }
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
        if apply && self.settings.import_runes {
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
            if apply && self.settings.import_spells {
                match self.apply_spells(base, password, &build.spells).await {
                    Ok(spells) => {
                        result.spells_applied = self.spell_names(base, password, &spells).await;
                        result.applied_spells = spells.to_vec();
                    }
                    Err(note) => {
                        eprintln!("[sorts] {note}");
                        result.spells_note = Some(note);
                    }
                }
            }
            if apply && self.settings.import_item_set {
                // « LoL Companion · Kayn rouge vs Lee Sin » : la forme de Kayn dans le titre.
                let form = match form {
                    Some(Form::Red) => " rouge",
                    Some(Form::Blue) => " bleu",
                    None => "",
                };
                let title = format!("{PAGE_NAME} · {}{form}{versus}", me.name);
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
            return opgg::Champ { id, name: id.to_string(), ..Default::default() };
        };
        let champ = opgg::Champ {
            id,
            name: info["name"].as_str().map_or_else(|| id.to_string(), str::to_string),
            alias: info["alias"].as_str().unwrap_or_default().to_string(),
            damage: info["tacticalInfo"]["damageType"].as_str().unwrap_or_default().to_string(),
            roles: info["roles"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect(),
            // "melee" ou "ranged" (vérifié sur les fichiers du client repris par CommunityDragon).
            ranged: info["tacticalInfo"]["attackType"] == "ranged",
        };
        self.champs.insert(id, champ.clone());
        champ
    }

    /// Liste de tous les champions du client, lue une fois (réessayée au tour suivant en cas d'échec).
    async fn load_champion_list(&mut self, base: &str, password: &str) {
        if !self.champion_list.is_empty() {
            return;
        }
        let url = format!("{base}/lol-game-data/assets/v1/champion-summary.json");
        let Some(Value::Array(list)) = self.get(&url, Some(password)).await else {
            return;
        };
        let text = |c: &Value, key: &str| c[key].as_str().unwrap_or_default().to_string();
        self.champion_list = list
            .iter()
            .filter_map(|c| Some((c["id"].as_i64().filter(|id| *id > 0)?, text(c, "alias"), text(c, "name"))))
            .collect();
        self.champion_ids = self.champion_list.iter().map(|(id, alias, _)| (alias.to_lowercase(), *id)).collect();
    }

    /// Classement d'OP.GG de chaque poste, relu au plus toutes les 3 heures. None tant qu'il n'a pas pu être lu.
    async fn lane_meta(&mut self, base: &str, password: &str) -> Option<&HashMap<String, Vec<opgg::MetaChamp>>> {
        if self.meta_due.is_none_or(|due| Instant::now() >= due) {
            // Sans la liste des champions du client, impossible de retrouver leurs numéros : au tour suivant.
            self.load_champion_list(base, password).await;
            if !self.champion_list.is_empty() {
                self.fetch_meta().await;
            }
        }
        self.meta.as_ref()
    }

    async fn fetch_meta(&mut self) {
        match self.opgg.lane_meta().await {
            Ok(mut meta) => {
                let mut unknown = Vec::new();
                for champ in meta.values_mut().flat_map(|list| list.iter_mut()) {
                    match champion_by_name(&champ.name, &self.champion_list) {
                        Some((id, name)) => (champ.champion_id, champ.name) = (id, name.to_string()),
                        None => unknown.push(champ.name.clone()),
                    }
                }
                unknown.sort();
                unknown.dedup();
                let counts: Vec<String> = meta.iter().map(|(position, list)| format!("{position} {}", list.len())).collect();
                eprintln!(
                    "[méta] classement OP.GG : {} ; noms non retrouvés dans le client : {}",
                    counts.join(", "),
                    if unknown.is_empty() { "aucun".to_string() } else { unknown.join(", ") }
                );
                self.meta = Some(meta);
                self.meta_due = Some(Instant::now() + META_REFRESH);
            }
            Err(e) => {
                eprintln!("[méta] classement : {e}");
                self.meta_due = Some(Instant::now() + META_RETRY);
            }
        }
    }

    /// Tes champions du moment dans le classement d'OP.GG, au poste où tu les joues le plus d'après les parties
    /// suivies par l'app, sinon à leur poste principal (celui où ils sont le plus joués).
    fn rate_champions(&self, profile: &mut Profile) {
        let Some(meta) = &self.meta else {
            return;
        };
        let puuid = self.puuid.as_deref();
        for champion in &mut profile.recent_champions {
            let id = champion.champion_id;
            let at = |position: &str| {
                let list = meta.get(position)?;
                let found = list.iter().find(|c| c.champion_id == id)?;
                Some((position.to_string(), found, list.len()))
            };
            let mine = self.history.main_position(puuid, id).and_then(|position| at(&position));
            let main = || POSITIONS.iter().filter_map(|p| at(p)).max_by(|a, b| a.1.role_rate.total_cmp(&b.1.role_rate));
            champion.meta = mine.or_else(main).map(|(position, c, ranked)| ChampionMeta {
                position,
                tier: c.tier,
                rank: c.rank,
                ranked: ranked as u32,
                rank_prev_patch: c.rank_prev_patch,
                win_rate: c.win_rate,
            });
        }
    }

    /// Bans conseillés : d'abord les champions qui battent le tien à ton poste (s'ils gagnent plus d'une partie
    /// sur deux contre lui), puis les mieux classés de ton poste sur OP.GG. Jamais un champion déjà banni ou pris
    /// (`avoid`), ni un de ton équipe, survolé ou choisi. Une seule question à OP.GG par tour pour les contres.
    async fn ban_help(
        &mut self,
        base: &str,
        password: &str,
        mine: Option<i64>,
        position: &str,
        mode: &str,
        avoid: &[i64],
    ) -> BanHelp {
        let mut help = BanHelp { position: position.to_string(), ..Default::default() };
        let mut asked = false;
        if let Some(id) = mine {
            let champ = self.champ(base, password, id).await;
            help.champion = champ.name.clone();
            match self.help_of(&champ, position, mode, &mut asked).await {
                Some(found) => {
                    let all = self.counters_of(base, password, &champ, &found, position, &mut asked).await;
                    help.pending |= all.is_none();
                    let list = all.as_deref().filter(|all| !all.is_empty()).unwrap_or(&found.counters);
                    let counters = list.iter().filter(|p| p.win_rate > 0.5 && !avoid.contains(&p.champion_id));
                    help.bans.extend(counters.take(BAN_COUNTERS).map(|p| BanPick {
                        champion_id: p.champion_id,
                        name: p.name.clone(),
                        counter: true,
                        win_rate: p.win_rate,
                        games: p.games,
                        tier: 0,
                    }));
                }
                None => help.pending = true,
            }
        }
        let Some(list) = self.lane_meta(base, password).await.and_then(|meta| meta.get(position)) else {
            return help;
        };
        let free: Vec<BanPick> = list
            .iter()
            .filter(|c| c.champion_id > 0 && !avoid.contains(&c.champion_id))
            .filter(|c| !help.bans.iter().any(|b| b.champion_id == c.champion_id))
            .take(BANS_SHOWN.saturating_sub(help.bans.len()))
            .map(|c| BanPick {
                champion_id: c.champion_id,
                name: c.name.clone(),
                counter: false,
                win_rate: c.win_rate,
                games: c.play,
                tier: c.tier,
            })
            .collect();
        help.bans.extend(free);
        help
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

    /// Duel OP.GG contre cet adversaire (réponse gardée en mémoire). `form` (Kayn) : seulement ce qui va avec sa
    /// forme (voir `KaynStyle`).
    async fn matchup(
        &mut self,
        me: &opgg::Champ,
        opponent: &opgg::Champ,
        position: &str,
        form: Option<Form>,
    ) -> Result<opgg::Matchup, String> {
        let key = (me.id, opponent.id, position.to_string());
        if !self.matchups.contains_key(&key) {
            let doc = self.opgg.matchup_doc(me, opponent, position).await?;
            self.matchups.insert(key.clone(), doc);
        }
        let doc = &self.matchups[&key];
        let parsed = match form {
            Some(form) => {
                // Catégories des objets (santé, armure, létalité) : lues dans le client.
                self.assets.load_items().await;
                let kayn = KaynStyle::new(form, doc, &self.assets);
                let (runes, core, item) = (|p: &Value| kayn.runes(p), |c: &[i64]| kayn.core(c), |id| kayn.item(id));
                let style = opgg::Style { runes: &runes, core: &core, item: &item };
                opgg::matchup_from(doc, opponent.id, Some(&style))
            }
            None => opgg::matchup_from(doc, opponent.id, None),
        };
        parsed.ok_or_else(|| "Réponse OP.GG illisible (format changé ?)".to_string())
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
        self.load_champion_list(base, password).await;
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
        let forms = self.game.as_ref().map(|g| &g.forms);
        let levels = self.game.as_ref().map(|g| &g.levels);
        let puuids = self.game.as_ref().map(|g| &g.puuids);
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
                smurf: rank.as_ref().is_some_and(|rank| smurf_like(rank, levels.and_then(|l| l.get(&id)).copied())),
                account_level: levels.and_then(|l| l.get(&id)).copied(),
                rank,
                position: player["position"].as_str().unwrap_or_default().to_string(),
                items_value: self.assets.items_value(&inventory(player)),
                group: groups.and_then(|g| g.get(&id)).copied(),
                met: met.and_then(|m| m.get(&id)).cloned(),
                riot_id: if hidden(id) { String::new() } else { player["riotId"].as_str().unwrap_or_default().to_string() },
                puuid: puuids.and_then(|m| m.get(&id)).cloned().unwrap_or_default(),
                recent: forms.and_then(|f| f.get(&id)).cloned(),
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
        // Objets du panneau : ceux du build, ceux que tu as (ou as achetés pendant la partie) grisés.
        let inventory: Vec<i64> = p["items"].as_array().into_iter().flatten().filter_map(|it| it["itemID"].as_i64()).collect();
        self.track_purchases(&inventory);
        let build = self.game_build(base, password).await;
        let enemy_ids: Vec<i64> = enemies.iter().map(|e| e.champion_id).filter(|id| *id > 0).collect();
        let needs = match &self.game {
            Some(game) if build.is_some() => self.needs(base, password, game.champion_id, &enemy_ids).await,
            _ => Needs::default(),
        };
        let english = self.settings.english_names;
        let (panel, details) = match (&build, &self.game) {
            (Some(build), Some(game)) => {
                let owned: HashSet<i64> = inventory.iter().chain(&game.bought).copied().collect();
                // Départ fait dès ton premier achat (même un autre départ), bottes dès des bottes améliorées.
                let started = owned.iter().any(|id| self.assets.price(*id) > 0);
                let boots_done = owned.iter().any(|id| self.assets.upgraded_boots(*id));
                let rows = panel(build, &owned, started, boots_done, &needs, &self.assets, english);
                let details = GameBuild {
                    versus: game.build_versus.clone(),
                    form: game.form.filter(|_| game.champion_id == KAYN).map_or("", Form::name).to_string(),
                    next: next_buy(&rows, &inventory, &self.assets, english),
                    skills: build.skill_max.clone(),
                    enemy_note: needs.note.clone(),
                    tip: game.tip.clone(),
                };
                (rows, Some(details))
            }
            _ => (Vec::new(), None),
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
            build: details,
        }
    }

    /// Note depuis quand chaque objet est dans ton inventaire : gardé 30 s, il est acheté pour de bon (et reste
    /// grisé dans le panneau même vendu ou utilisé ensuite).
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
            if now.duration_since(*since) >= BOUGHT_AFTER {
                game.bought.insert(*id);
            }
        }
    }

    /// Ce que l'équipe d'en face demande (voir `Needs`), d'après la fiche de chaque champion dans le client :
    /// son type de dégâts, ses rôles, et les gros soigneurs (`HEALERS`).
    async fn needs(&mut self, base: &str, password: &str, me: i64, enemies: &[i64]) -> Needs {
        let mine = self.champ(base, password, me).await;
        let (mut physical, mut magic, mut tanks) = (0.0, 0.0, 0);
        let mut healers = Vec::new();
        for id in enemies {
            let enemy = self.champ(base, password, *id).await;
            if HEALERS.contains(id) {
                healers.push(enemy.name.clone());
            }
            match enemy.damage.as_str() {
                "kPhysical" => physical += 1.0,
                "kMagic" => magic += 1.0,
                "kMixed" => {
                    physical += 0.5;
                    magic += 0.5;
                }
                _ => {}
            }
            tanks += usize::from(enemy.roles.iter().any(|r| r == "tank"));
        }
        self.log("contre", format!("[panneau] en face : {physical} AD, {magic} AP, {tanks} tank(s), {} soigneur(s)", healers.len()));
        // « 3,5 AD · 1,5 AP · 2 tanks · soins : Soraka » (un champion aux dégâts mixtes compte pour moitié).
        let num = |n: f64| n.to_string().replace('.', ",");
        let mut note = vec![format!("{} AD", num(physical)), format!("{} AP", num(magic))];
        if tanks > 0 {
            note.push(format!("{tanks} tank{}", if tanks > 1 { "s" } else { "" }));
        }
        if !healers.is_empty() {
            note.push(format!("soins : {}", healers.join(", ")));
        }
        let magic_damage = mine.damage == "kMagic";
        Needs {
            note: note.join(" · "),
            anti_heal: !healers.is_empty(),
            armor: physical >= 3.5,
            magic_resist: magic >= 3.5,
            penetration: (tanks >= 2).then_some(if magic_damage { "MagicPenetration" } else { "ArmorPenetration" }),
            // Gilet épineux pour un tank, Orbe d'oubli pour des dégâts magiques, sinon Appel du bourreau.
            grievous_component: if mine.roles.iter().any(|r| r == "tank") {
                3076
            } else if magic_damage {
                3916
            } else {
                3123
            },
        }
    }

    /// Objets du panneau en jeu : ceux du duel contre ton adversaire de couloir (si OP.GG a assez de parties),
    /// sinon ceux de ton champion à ton poste. Cherchés une fois par (champion, poste, adversaire).
    async fn game_build(&mut self, base: &str, password: &str) -> Option<opgg::Build> {
        let game = self.game.as_ref()?;
        // La forme retenue en sélection ne vaut que pour Kayn.
        let form = game.form.filter(|_| game.champion_id == KAYN);
        let key = (game.champion_id, game.position.clone(), game.opponent_id, form);
        if game.build_for.as_ref() == Some(&key) {
            return game.build.clone();
        }
        if key.0 <= 0 || key.1.is_empty() {
            return None;
        }
        let mode = game.mode.clone();
        let me = self.champ(base, password, key.0).await;
        let mut build = None;
        let mut versus = String::new();
        let mut tip = DuelTip::default();
        if let Some(id) = key.2 {
            let opponent = self.champ(base, password, id).await;
            match self.matchup(&me, &opponent, &key.1, key.3).await {
                Ok(m) => {
                    // Le conseil vaut même quand le duel a trop peu de parties pour ses objets.
                    tip = DuelTip { versus: opponent.name.clone(), play_style: m.play_style, text: m.tip };
                    if m.analysis.runes.games >= MIN_MATCHUP_GAMES {
                        build = Some(m.analysis.build);
                        versus = opponent.name;
                    } else {
                        eprintln!("[panneau] {} vs {} : trop peu de parties, objets généraux", me.name, opponent.name);
                    }
                }
                Err(e) => eprintln!("[panneau] duel introuvable ({e}), objets généraux"),
            }
        }
        // Kayn sans duel utilisable : le build de sa forme d'après son duel contre Lee Sin.
        if let Some(form) = key.3.filter(|_| build.is_none()) {
            let reference = self.champ(base, password, LEE_SIN).await;
            match self.matchup(&me, &reference, &key.1, Some(form)).await {
                Ok(m) if !m.analysis.build.is_empty() => build = Some(m.analysis.build),
                Ok(_) => eprintln!("[panneau] {} {} : rien contre Lee Sin, objets généraux", me.name, form.name()),
                Err(e) => eprintln!("[panneau] duel de référence introuvable ({e}), objets généraux"),
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
            game.build_versus = versus;
            game.tip = tip;
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
        if let Some((champion_id, position, opponent_id, form)) = self.last_pick.clone() {
            game.champion_id = champion_id;
            game.position = position;
            game.opponent_id = opponent_id;
            game.form = form;
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
                    game.puuids.insert(champion_id, puuid.to_string());
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
                let text = |key: &str| p[key].as_str().unwrap_or_default();
                let riot_id = if text("gameName").is_empty() { String::new() } else { format!("{}#{}", text("gameName"), text("tagLine")) };
                let player = Player {
                    champion,
                    champion_id,
                    me: is_me,
                    rank_hidden: puuid.is_none(),
                    smurf: rank.as_ref().is_some_and(|rank| smurf_like(rank, None)),
                    rank,
                    position: lane.to_string(),
                    riot_id,
                    puuid: puuid.unwrap_or_default().to_string(),
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
            let url = format!("{base}/lol-summoner/v2/summoners/puuid/{}", player.puuid);
            player.level = self.get_from_riot(&url, password).await.and_then(|s| s["summonerLevel"].as_u64());
            read.push(player);
        }
        let Some(game) = &mut self.game else {
            return;
        };
        // Fiche rapide de chacun, dès que ses parties sont lues ; smurf revu avec le niveau de son compte.
        for player in &read {
            let shown = game.allies.iter_mut().chain(game.enemies.iter_mut()).filter(|p| p.champion_id == player.champion_id);
            let form = (!player.games.is_empty()).then(|| recent_form(&player.games, player.champion_id));
            for shown in shown {
                shown.recent.clone_from(&form);
                shown.account_level = player.level;
                shown.smurf = shown.rank.as_ref().is_some_and(|rank| smurf_like(rank, player.level));
            }
            if let Some(form) = form {
                game.forms.insert(player.champion_id, form);
            }
            if let Some(level) = player.level {
                game.levels.insert(player.champion_id, level);
            }
        }
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
        self.champion_list.clear();
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

/// Ton ban n'est pas encore fait : la phase des bans arrive ou est en cours.
fn my_ban_pending(session: &Value) -> bool {
    let me = &session["localPlayerCellId"];
    let actions = session["actions"].as_array().into_iter().flatten().filter_map(Value::as_array).flatten();
    actions.filter(|a| a["type"] == "ban" && &a["actorCellId"] == me).any(|a| a["completed"] != true)
}

/// Numéro et nom dans le client d'un champion d'après son nom anglais sur OP.GG, comparés sans ponctuation, espaces
/// ni majuscules : son nom interne (« Kai'Sa » = Kaisa) ; sinon son nom affiché (« Wukong »), le plus petit numéro
/// s'il y en a plusieurs (le client liste aussi des variantes de modes spéciaux, « Jade_Annie » nommée Annie) ;
/// sinon le champion dont le nom interne est le premier mot de son nom (« Nunu & Willump » = Nunu).
/// Vérifié sur les vraies listes (client anglais ou français) : les 173 noms d'OP.GG sont retrouvés.
fn champion_by_name<'a>(name: &str, champions: &'a [(i64, String, String)]) -> Option<(i64, &'a str)> {
    let key = |text: &str| -> String { text.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect() };
    let wanted = key(name);
    let first = key(name.split_whitespace().next().unwrap_or_default());
    if wanted.is_empty() {
        return None;
    }
    let by_name = || champions.iter().filter(|(_, _, shown)| key(shown) == wanted).min_by_key(|(id, _, _)| *id);
    let by_first = || champions.iter().find(|(_, alias, _)| !first.is_empty() && key(alias) == first);
    champions
        .iter()
        .find(|(_, alias, _)| key(alias) == wanted)
        .or_else(by_name)
        .or_else(by_first)
        .map(|(id, _, shown)| (*id, shown.as_str()))
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
            let (win, seconds) = (p["stats"]["win"].as_bool(), g["gameDuration"].as_u64().unwrap_or(0));
            Some((game_id, Past { team, champion, start: g["gameCreation"].as_i64().unwrap_or(0), win, seconds }))
        })
        .collect()
}

/// Fiche rapide d'un joueur d'après ses dernières parties (sans les parties annulées) : sa série en cours, la plus
/// récente d'abord, et ses parties avec ce champion.
fn recent_form(games: &HashMap<i64, Past>, champion_id: i64) -> RecentForm {
    let mut played: Vec<&Past> = games.values().filter(|p| p.win.is_some() && p.seconds >= REMAKE_SECONDS).collect();
    played.sort_by_key(|p| std::cmp::Reverse(p.start));
    let wins: Vec<bool> = played.iter().map(|p| p.win == Some(true)).collect();
    let run = wins.iter().take_while(|w| Some(*w) == wins.first()).count() as i32;
    let on: Vec<&&Past> = played.iter().filter(|p| p.champion == champion_id).collect();
    RecentForm {
        games: played.len() as u32,
        streak: if wins.first() == Some(&true) { run } else { -run },
        champion_games: on.len() as u32,
        champion_wins: on.iter().filter(|p| p.win == Some(true)).count() as u32,
        otp: played.len() >= 10 && on.len() * 100 >= 60 * played.len(),
    }
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
            RecentChampion { champion_id, name, games, wins, meta: None }
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

/// Niveau de compte en dessous duquel un compte est jeune (les classées s'ouvrent au niveau 30).
const YOUNG_ACCOUNT: u64 = 60;

/// Smurf probable (compte d'un joueur plus fort que son rang), d'après ses parties classées de la saison dans la file
/// de la partie (au moins 10) :
/// - peu de parties (100 au plus), au moins 65 % de victoires, et trop pour être de la chance : victoires − défaites
///   ≥ 2 × √parties (plus de 2 écarts-types au-dessus de 50 %). Ex. 15 V – 5 D : oui ; 14 V – 6 D : non ;
///   130 V – 70 D : non (beaucoup de parties : un bon joueur) ;
/// - ou compte jeune (niveau sous 60), au moins 60 % de victoires et victoires − défaites ≥ √parties.
///   Ex. 13 V – 7 D au niveau 41 : oui (non au niveau 120).
fn smurf_like(rank: &Rank, level: Option<u64>) -> bool {
    let games = rank.wins + rank.losses;
    // % en entiers (pas d'arrondi à la limite) ; l'écart en nombres à virgule, pour la racine.
    let rate_at_least = |percent: u64| rank.wins * 100 >= percent * games;
    let margin = rank.wins as f64 - rank.losses as f64;
    let spread = (games as f64).sqrt();
    let few_games = games <= 100 && rate_at_least(65) && margin >= 2.0 * spread;
    let young = level.is_some_and(|level| level < YOUNG_ACCOUNT) && rate_at_least(60) && margin >= spread;
    games >= 10 && (few_games || young)
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
        build.starter.insert(0, opgg::Item { id: ATLAS, name: "Atlas".to_string(), count: 1, ..Default::default() });
    }
    build
}

/// Panneau en jeu, de haut en bas : départ, 1er objet (et son autre choix), bottes, 2e objet (et son autre choix),
/// 3e objet, puis « + » : les 3 premiers choix suivants d'OP.GG, et « VS » : jusqu'à 3 objets contre l'équipe d'en
/// face (`needs`), pris parmi ces choix avant la suite. Rien ne disparaît : un objet que tu as (`owned`) est grisé, une étape faite aussi, et la
/// prochaine étape est mise en avant. `started` : ton premier achat est fait ; `boots_done` : tu as des bottes
/// améliorées (pas forcément celles conseillées). `english` : noms des objets en anglais (écran de l'app).
fn panel(
    build: &opgg::Build,
    owned: &HashSet<i64>,
    started: bool,
    boots_done: bool,
    needs: &Needs,
    assets: &Assets,
    english: bool,
) -> Vec<PanelRow> {
    // Un objet : grisé si tu l'as ; avec son nom et son prix pour l'écran de l'app.
    let entry = |id: i64, count: u32, size: &'static str| PanelItem {
        id,
        count,
        size,
        owned: owned.contains(&id),
        reason: "",
        name: assets.item_name(id, english),
        price: assets.price(id),
        games: 0,
        wins: 0,
    };
    let item = |i: &opgg::Item, size| entry(i.id, i.count, size);
    // Choix suivant, ou objet contre l'équipe d'en face : avec ses parties et victoires sur OP.GG.
    let option = |i: &opgg::Item| PanelItem { games: i.games, wins: i.wins, ..entry(i.id, i.count, "option") };
    // Une étape du build : faite dès qu'un de ses objets est acheté.
    let step = |main: Option<&opgg::Item>, alt: Option<&opgg::Item>, size| -> (Vec<PanelItem>, bool) {
        let items: Vec<PanelItem> = main.map(|m| item(m, size)).into_iter().chain(alt.map(|a| item(a, "alt"))).collect();
        let done = items.iter().any(|i| i.owned);
        (items, done)
    };
    let (boots, boots_owned) = step(build.boots.first(), None, "boots");
    let path = [
        (build.starter.iter().map(|i| item(i, "starter")).collect(), started),
        step(build.core.first(), build.first_alt.as_ref(), "main"),
        (boots, boots_owned || boots_done),
        step(build.core.get(1), build.second_alt.as_ref(), "main"),
        step(build.core.get(2), None, "main"),
    ];
    let mut rows: Vec<PanelRow> = Vec::new();
    for (items, done) in path.into_iter().filter(|(items, _)| !items.is_empty()) {
        rows.push(PanelRow { label: format!("{}.", rows.len() + 1), items, done, next: false });
    }
    let shown: Vec<i64> = rows.iter().flat_map(|r| r.items.iter().map(|i| i.id)).collect();
    // Contre l'équipe d'en face d'abord : parmi les autres choix d'OP.GG pour ton champion, les plus joués d'abord.
    let candidates: Vec<&opgg::Item> =
        build.options.iter().chain(&build.first_alt).chain(&build.second_alt).filter(|i| !shown.contains(&i.id)).collect();
    // Le premier candidat qui convient, pas déjà proposé.
    let pick = |taken: &[PanelItem], fits: &dyn Fn(i64) -> bool, reason: &'static str| -> Option<PanelItem> {
        let found = candidates.iter().find(|i| fits(i.id) && !taken.iter().any(|t| t.id == i.id))?;
        Some(PanelItem { reason, ..option(found) })
    };
    let mut against: Vec<PanelItem> = Vec::new();
    if needs.anti_heal {
        // Sans objet à Blessures graves parmi ses choix : le composant qui va avec tes dégâts.
        let id = needs.grievous_component;
        let component = (assets.price(id) > 0).then(|| PanelItem { reason: "soins", ..entry(id, 1, "option") });
        let found = pick(&against, &|id| assets.grievous(id), "soins").or(component);
        against.extend(found);
    }
    let wanted = [(needs.armor, "Armor", "AD"), (needs.magic_resist, "SpellBlock", "AP")];
    for (tag, reason) in wanted.into_iter().filter(|(wanted, _, _)| *wanted).map(|(_, tag, reason)| (tag, reason)) {
        let found = pick(&against, &|id| assets.has_tag(id, tag), reason);
        against.extend(found);
    }
    if let Some(tag) = needs.penetration {
        let found = pick(&against, &|id| assets.has_tag(id, tag), "tanks");
        against.extend(found);
    }
    against.truncate(3);
    // La suite : les 3 premiers choix suivants d'OP.GG, hors ceux déjà proposés contre l'équipe d'en face.
    let more: Vec<PanelItem> = build
        .options
        .iter()
        .filter(|i| !shown.contains(&i.id) && !against.iter().any(|a| a.id == i.id))
        .take(3)
        .map(option)
        .collect();
    if !more.is_empty() {
        rows.push(PanelRow { label: "+".to_string(), done: more.iter().all(|i| i.owned), items: more, next: false });
    }
    if !against.is_empty() {
        rows.push(PanelRow { label: "VS".to_string(), items: against, done: false, next: false });
    }
    // La prochaine étape : la première pas encore faite.
    if let Some(row) = rows.iter_mut().find(|r| !r.done && r.label != "VS") {
        row.next = true;
    }
    rows
}

/// Prochain achat, d'après la prochaine étape du panneau : les objets de départ, sinon son premier objet que tu
/// n'as pas (l'objet conseillé avant son autre choix), avec ce qu'il coûte encore et ses composants.
fn next_buy(rows: &[PanelRow], inventory: &[i64], assets: &Assets, english: bool) -> Option<NextBuy> {
    let row = rows.iter().find(|r| r.next)?;
    if row.items.iter().all(|i| i.size == "starter") {
        let price = row.items.iter().map(|i| i.price * u64::from(i.count)).sum();
        return Some(NextBuy { items: row.items.clone(), price, left: price, components: Vec::new() });
    }
    let item = row.items.iter().find(|i| !i.owned)?.clone();
    let (left, parts) = assets.recipe(item.id, inventory);
    let components = parts
        .into_iter()
        .map(|(id, owned)| PanelItem {
            id,
            count: 1,
            size: "option",
            owned,
            reason: "",
            name: assets.item_name(id, english),
            price: assets.price(id),
            games: 0,
            wins: 0,
        })
        .collect();
    Some(NextBuy { price: item.price, left, components, items: vec![item] })
}

/// Ce que tu as déjà pour cet objet, en or : lui-même s'il est dans `owned`, sinon ses composants (et les leurs).
/// Chaque objet de `owned` ne sert qu'une fois : il en est retiré.
fn owned_value(items: &HashMap<i64, ItemData>, id: i64, owned: &mut Vec<i64>) -> u64 {
    if let Some(i) = owned.iter().position(|o| *o == id) {
        owned.swap_remove(i);
        return items.get(&id).map_or(0, |d| d.price);
    }
    let parts = items.get(&id).map(|d| d.from.clone()).unwrap_or_default();
    parts.into_iter().map(|part| owned_value(items, part, owned)).sum()
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
/// Postes au sens d'OP.GG, dans l'ordre des couloirs.
const POSITIONS: [&str; 5] = ["top", "jungle", "mid", "adc", "support"];

/// Poste au sens du client ("middle", "bottom", "utility"…) d'un poste au sens d'OP.GG ("mid", "adc", "support"…).
fn client_lane(position: &str) -> &'static str {
    match position {
        "top" => "top",
        "jungle" => "jungle",
        "mid" => "middle",
        "adc" => "bottom",
        "support" => "utility",
        _ => "",
    }
}

/// Répartition des adversaires aux postes qui donne, au total, la plus forte part de parties à leur poste (d'après
/// `roles`, d'OP.GG), deux adversaires jamais au même poste : (champion, poste au sens d'OP.GG, part de ses parties
/// à ce poste). `fixed` : un adversaire placé d'office à un poste (choisi d'un clic, à ton poste).
fn assign_roles(ids: &[i64], roles: &HashMap<i64, Vec<(String, f64)>>, fixed: Option<(i64, &str)>) -> Vec<(i64, &'static str, f64)> {
    let rate = |id: i64, lane: &str| {
        let shares = roles.get(&id).into_iter().flatten();
        shares.filter(|(p, _)| p == lane).map(|(_, rate)| *rate).next().unwrap_or(0.0)
    };
    let ids = &ids[..ids.len().min(POSITIONS.len())];
    let mut best: Option<(f64, Vec<usize>)> = None;
    // 120 façons de placer 5 adversaires : on les essaie toutes (à égalité, la première gagne).
    for order in permutations(POSITIONS.len()) {
        let lanes = &order[..ids.len()];
        let placed = |(id, lane): (&i64, &usize)| fixed.is_none_or(|(chosen, at)| *id != chosen || POSITIONS[*lane] == at);
        if !ids.iter().zip(lanes).all(placed) {
            continue;
        }
        let score: f64 = ids.iter().zip(lanes).map(|(id, lane)| rate(*id, POSITIONS[*lane])).sum();
        if best.as_ref().is_none_or(|(top, _)| score > *top) {
            best = Some((score, lanes.to_vec()));
        }
    }
    let lanes = best.map(|(_, lanes)| lanes).unwrap_or_default();
    ids.iter().zip(lanes).map(|(id, lane)| (*id, POSITIONS[lane], rate(*id, POSITIONS[lane]))).collect()
}

/// Kayn : ce qui va avec sa forme dans un duel OP.GG. Runes : voie principale Précision (Conquérant) pour le rouge,
/// une autre (Moisson noire, Grêle de lames…) pour le bleu. Combinaison d'objets principaux : rouge si elle compte
/// plus d'objets de combattant (santé ou armure) que d'objets de létalité (sans santé ni armure), bleue s'il y en a
/// moins. Autre objet : s'il apparaît dans des combinaisons, il va si les parties de celles de sa forme font au
/// moins le quart de celles de l'autre (Cyclosabre voltaïque va aux deux, Danse de la mort au rouge seulement) ;
/// sinon, s'il n'est pas un objet de l'autre forme (combattant pour le bleu, létalité seule pour le rouge).
struct KaynStyle {
    form: Form,
    /// Pour chaque objet du duel : +1 combattant, −1 létalité seule, 0 ni l'un ni l'autre.
    sides: HashMap<i64, i32>,
    /// Parties des combinaisons de sa forme, et de celles de l'autre, où se trouve chaque objet.
    mine: HashMap<i64, u64>,
    theirs: HashMap<i64, u64>,
}

impl KaynStyle {
    fn new(form: Form, doc: &Value, assets: &Assets) -> Self {
        let ids = |e: &Value| -> Vec<i64> { e["ids"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect() };
        let d = &doc["data"];
        let entries: Vec<&Value> = d["core_items"].as_array().into_iter().flatten().collect();
        let combos: Vec<Vec<i64>> = entries.iter().map(|e| ids(e)).collect();
        let singles = d["single_items"].as_array().into_iter().flatten().flat_map(|s| s["items"].as_array().into_iter().flatten());
        let all: Vec<i64> = combos.iter().flatten().copied().chain(singles.flat_map(ids)).collect();
        let side = |id: i64| match (assets.has_tag(id, "Health") || assets.has_tag(id, "Armor"), assets.has_tag(id, "ArmorPenetration")) {
            (true, _) => 1,
            (false, true) => -1,
            (false, false) => 0,
        };
        let sides: HashMap<i64, i32> = all.into_iter().map(|id| (id, side(id))).collect();
        let (mut mine, mut theirs) = (HashMap::new(), HashMap::new());
        for (entry, combo) in entries.iter().zip(&combos) {
            let plays = entry["play"].as_u64().unwrap_or(0);
            let counts = match combo_form(combo, &sides) {
                Some(f) if f == form => &mut mine,
                Some(_) => &mut theirs,
                None => continue,
            };
            for id in combo {
                *counts.entry(*id).or_insert(0) += plays;
            }
        }
        Self { form, sides, mine, theirs }
    }

    fn runes(&self, page: &Value) -> bool {
        (page["primary_page_id"] == PRECISION) == (self.form == Form::Red)
    }

    fn core(&self, combo: &[i64]) -> bool {
        combo_form(combo, &self.sides) == Some(self.form)
    }

    fn item(&self, id: i64) -> bool {
        let (mine, theirs) = (self.mine.get(&id).copied().unwrap_or(0), self.theirs.get(&id).copied().unwrap_or(0));
        if mine + theirs > 0 {
            return mine * 4 >= theirs;
        }
        let side = self.sides.get(&id).copied().unwrap_or(0);
        match self.form {
            Form::Red => side >= 0,
            Form::Blue => side <= 0,
        }
    }
}

/// Forme d'une combinaison d'objets de Kayn (voir `KaynStyle`) ; None à égalité.
fn combo_form(combo: &[i64], sides: &HashMap<i64, i32>) -> Option<Form> {
    let total: i32 = combo.iter().map(|id| sides.get(id).copied().unwrap_or(0)).sum();
    match total.cmp(&0) {
        std::cmp::Ordering::Greater => Some(Form::Red),
        std::cmp::Ordering::Less => Some(Form::Blue),
        std::cmp::Ordering::Equal => None,
    }
}

/// Toutes les façons d'ordonner 0, 1… n − 1.
fn permutations(n: usize) -> Vec<Vec<usize>> {
    let mut all: Vec<Vec<usize>> = vec![Vec::new()];
    for _ in 0..n {
        let mut longer = Vec::new();
        for start in &all {
            for i in (0..n).filter(|i| !start.contains(i)) {
                let mut next = start.clone();
                next.push(i);
                longer.push(next);
            }
        }
        all = longer;
    }
    all
}

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
