//! Runes et build recommandés par OP.GG, via leur service public « MCP » (prévu pour les assistants IA).
//! Les données OP.GG sont réservées à un usage personnel.

use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::HashMap, time::Duration};

const URL: &str = "https://mcp-api.op.gg/mcp";
const LANG: &str = "fr_FR";
const APP: &str = concat!("LoL Companion/", env!("CARGO_PKG_VERSION"));
/// Tranche de rangs des stats. None = tous les rangs ; sinon par ex. Some("emerald_plus").
const TIER: Option<&str> = None;
const UNREADABLE: &str = "Réponse OP.GG illisible (format changé ?)";
/// Objets d'un duel : on prend le meilleur % de victoires parmi les choix joués au moins autant de fois
/// (en dessous, un % élevé peut n'être que du hasard), sinon le plus joué.
const MIN_PLAY: u64 = 100;
/// Pages de runes proposées pour un duel : la plus jouée, puis 2 autres (au moins 50 parties chacune, pour que
/// leur % de victoires veuille dire quelque chose).
const PAGES_SHOWN: usize = 3;
const MIN_PAGE_GAMES: u64 = 50;
/// Sets d'objets (les 3 objets principaux) proposés pour un duel : le plus joué, puis 2 autres (au moins 50 parties
/// chacun). Sans duel, OP.GG n'en donne qu'un.
const SETS_SHOWN: usize = 3;
const MIN_SET_GAMES: u64 = 50;
static NULL: Value = Value::Null;
/// Champs demandés à OP.GG, recopiés de la liste fermée donnée par leur outil.
const FIELDS: [&str; 11] = [
    "data.summary.{id,is_rip,is_rotation,roles}",
    "data.runes.{id,pick_rate,play,primary_page_id,primary_page_name,primary_rune_ids[],primary_rune_names[],secondary_page_id,secondary_page_name,secondary_rune_ids[],secondary_rune_names[],stat_mod_ids[],stat_mod_names[],win}",
    "data.starter_items.{ids[],ids_names[],pick_rate,play,win}",
    "data.core_items.{ids[],ids_names[],pick_rate,play,win}",
    "data.boots.{ids[],ids_names[],pick_rate,play,win}",
    "data.fourth_items[].{ids[],ids_names[],pick_rate,play,win}",
    "data.fifth_items[].{ids[],ids_names[],pick_rate,play,win}",
    "data.sixth_items[].{ids[],ids_names[],pick_rate,play,win}",
    "data.summoner_spells.{ids[],ids_names[],pick_rate,play,win}",
    "data.skills.{order[],pick_rate,play,win}",
    "data.skill_masteries.{ids[],pick_rate,play,win}",
];
/// Champs demandés pour le salon d'un joueur cherché (recopiés de la liste fermée donnée par leurs outils) :
/// son profil, ses dernières parties (lui seul dans chaque partie) et le détail d'une partie (les 10 joueurs).
const PROFILE_FIELDS: [&str; 3] = [
    "data.summoner.{game_name,tagline,level}",
    "data.summoner.league_stats[].{game_type,win,lose}",
    "data.summoner.league_stats[].tier_info.{tier,division,lp}",
];
/// Sa saison en classée (solo et flexible ensemble) : ses parties et victoires, et ses champions les plus joués
/// (10 au plus) avec leurs parties, victoires, éliminations, morts et assistances.
const SEASON_FIELDS: [&str; 2] = [
    "data.summoner.most_champions.{game_type,play,win,lose}",
    "data.summoner.most_champions.champion_stats[].{id,champion_name,play,win,kill,death,assist}",
];
const MATCH_FIELDS: [&str; 4] = [
    "data.game_history[].{id,created_at,game_length_second,game_map,game_type}",
    "data.game_history[].participants[].{champion_id,champion_name,position}",
    "data.game_history[].participants[].summoner.{game_name,tagline}",
    "data.game_history[].participants[].stats.{kill,death,assist,minion_kill,neutral_minion_kill,gold_earned,total_damage_dealt_to_champions,result}",
];
const DETAIL_FIELDS: [&str; 7] = [
    "data.game_detail.{id,created_at,game_length_second,game_map,game_type}",
    "data.game_detail.teams[].{key}",
    "data.game_detail.teams[].game_stat.{is_win}",
    "data.game_detail.teams[].participants[].{champion_id,champion_name,position,team_key,items[],spells[],is_target}",
    "data.game_detail.teams[].participants[].summoner.{game_name,tagline}",
    "data.game_detail.teams[].participants[].rune.{primary_rune_id,secondary_page_id}",
    "data.game_detail.teams[].participants[].stats.{kill,death,assist,champion_level,minion_kill,neutral_minion_kill,gold_earned,total_damage_dealt_to_champions,ward_place,op_score,op_score_rank,result}",
];
/// Champs demandés pour le classement des champions de chaque poste (recopiés de la liste fermée de l'outil).
const META_FIELDS: [&str; 5] = [
    "data.positions.top[].{champion,is_rip,tier,rank,rank_prev_patch,win,win_rate,pick_rate,ban_rate,kda,kill,play,role_rate}",
    "data.positions.jungle[].{champion,is_rip,tier,rank,rank_prev_patch,win,win_rate,pick_rate,ban_rate,kda,kill,play,role_rate}",
    "data.positions.mid[].{champion,is_rip,tier,rank,rank_prev_patch,win,win_rate,pick_rate,ban_rate,kda,kill,play,role_rate}",
    "data.positions.adc[].{champion,is_rip,tier,rank,rank_prev_patch,win,win_rate,pick_rate,ban_rate,kda,kill,play,role_rate}",
    "data.positions.support[].{champion,is_rip,tier,rank,rank_prev_patch,win,win_rate,pick_rate,ban_rate,kda,kill,play,role_rate}",
];
/// Le classement ne donne que le nom des champions : en anglais, il suit leur nom interne (« Master Yi »,
/// « MasterYi »), quelle que soit la langue du client.
const META_LANG: &str = "en_US";
/// Postes du classement, au sens d'OP.GG.
const META_POSITIONS: [&str; 5] = ["top", "jungle", "mid", "adc", "support"];
/// Page d'un champion (recherche) : en plus de son build (FIELDS), ses stats à chaque poste, ses meilleurs et pires
/// duels, ses meilleurs partenaires (OP.GG les range par poste du partenaire, sans le sien).
const PAGE_FIELDS: [&str; 10] = [
    "data.summary.positions[].name",
    "data.summary.positions[].stats.{play,win_rate,pick_rate,role_rate,ban_rate,kda}",
    "data.summary.positions[].stats.tier_data.{tier,rank}",
    "data.strong_counters[].{champion_id,champion_name,play,win}",
    "data.weak_counters[].{champion_id,champion_name,play,win}",
    "data.synergies.top[].{synergy_champion_id,synergy_champion_name,play,win}",
    "data.synergies.jungle[].{synergy_champion_id,synergy_champion_name,play,win}",
    "data.synergies.mid[].{synergy_champion_id,synergy_champion_name,play,win}",
    "data.synergies.adc[].{synergy_champion_id,synergy_champion_name,play,win}",
    "data.synergies.support[].{synergy_champion_id,synergy_champion_name,play,win}",
];
/// Joueurs d'un champion (les plus de parties avec lui dans une région) : leur rang en solo/duo et leurs stats.
const LEADERBOARD_FIELDS: [&str; 5] = [
    "leaderboard[].rank",
    "leaderboard[].summoner.{game_name,tagline}",
    "leaderboard[].summoner.league_stats[].game_type",
    "leaderboard[].summoner.league_stats[].tier_info.{tier,division,lp}",
    "leaderboard[].most_champion_stat.{play,win,kill,death,assist}",
];

/// Champs demandés pour savoir à quels postes un champion est joué.
const POSITION_FIELDS: [&str; 3] = [
    "data.summary.{id,is_rip,is_rotation,roles}",
    "data.summary.positions[].name",
    "data.summary.positions[].stats.{ban_rate,kda,pick_rate,play,role_rate,win_rate}",
];

/// Un champion, tel que le client le décrit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Champ {
    pub id: i64,
    /// Nom affiché (« Wukong »).
    pub name: String,
    /// Nom interne (« MonkeyKing »).
    pub alias: String,
    /// Type de dégâts d'après le client : "kPhysical", "kMagic" ou "kMixed" (vide si inconnu).
    pub damage: String,
    /// Rôles d'après le client : "tank", "fighter", "mage", "assassin", "marksman", "support".
    pub roles: Vec<String>,
    /// Champion à distance d'après le client (false : mêlée, ou inconnu).
    pub ranged: bool,
}

/// Un style de jeu à part pour un même champion (Kayn rouge ou bleu) : ce qui lui va dans les listes d'OP.GG.
pub struct Style<'a> {
    /// Une page de runes d'OP.GG (entrée de la liste « runes ») lui va.
    pub runes: &'a dyn Fn(&Value) -> bool,
    /// Une combinaison d'objets principaux lui va.
    pub core: &'a dyn Fn(&[i64]) -> bool,
    /// Un objet seul (autre choix, choix suivants) lui va.
    pub item: &'a dyn Fn(i64) -> bool,
}

/// Ce qu'OP.GG recommande pour un champion à un poste : runes et build.
#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    pub runes: Runes,
    pub build: Build,
}

/// Objets, sorts d'invocateur et compétences les plus joués.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Build {
    pub starter: Vec<Item>,
    /// Les 3 objets principaux, dans l'ordre d'achat.
    pub core: Vec<Item>,
    pub boots: Vec<Item>,
    /// Autre choix pour le 1er et le 2e objet principal (duel seulement).
    pub first_alt: Option<Item>,
    pub second_alt: Option<Item>,
    /// Choix les plus joués en 4e, 5e et 6e objet, sans ceux déjà au-dessus.
    pub options: Vec<Item>,
    /// Identifiants des sorts d'invocateur (4 = Saut éclair...).
    pub spells: Vec<i64>,
    /// Ordre de montée des compétences, ex. ["Q", "W", "E"].
    pub skill_max: Vec<String>,
    /// Compétence prise à chaque niveau, ex. ["W", "Q", "E", "Q"...].
    pub skill_order: Vec<String>,
    /// Sets d'objets (3 objets principaux) les plus joués, le classique d'abord (voir SETS_SHOWN).
    pub sets: Vec<CoreSet>,
}

/// Un set d'objets : les 3 objets principaux, avec ses parties et victoires sur OP.GG.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
pub struct CoreSet {
    pub items: Vec<Item>,
    pub games: u64,
    pub wins: u64,
}

pub(crate) fn core_set(entry: &Value) -> CoreSet {
    CoreSet {
        items: items_of(entry),
        games: entry["play"].as_u64().unwrap_or(0),
        wins: entry["win"].as_u64().unwrap_or(0),
    }
}

impl Build {
    pub fn is_empty(&self) -> bool {
        self.starter.is_empty() && self.core.is_empty()
    }
}

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
pub struct Item {
    pub id: i64,
    pub name: String,
    /// Ex. 2 pour deux potions de départ.
    pub count: u32,
    /// Parties et victoires de ce choix sur OP.GG (pour un objet seul, comme les choix suivants ; pour un objet
    /// des objets principaux, celles des trois ensemble).
    pub games: u64,
    pub wins: u64,
}

/// Page de runes recommandée par OP.GG, prête à envoyer au client.
#[derive(Clone, Debug, PartialEq)]
pub struct Runes {
    /// Champion renvoyé par OP.GG (0 si inconnu), pour vérifier qu'il n'y a pas d'erreur.
    pub champion_id: i64,
    pub primary_style: i64,
    pub sub_style: i64,
    /// 4 runes principales, 2 secondaires, 3 fragments.
    pub perks: Vec<i64>,
    pub keystone: String,
    pub primary_name: String,
    pub secondary_name: String,
    /// Noms des 4 runes principales et des 2 secondaires (dans l'ordre de `perks`).
    pub rune_names: Vec<String>,
    pub games: u64,
    pub wins: u64,
}

/// Duel contre ton adversaire direct : ce qu'OP.GG recommande pour ce duel précis.
#[derive(Clone, Debug, PartialEq)]
pub struct Matchup {
    pub analysis: Analysis,
    /// Parties et victoires de ton champion contre celui-ci, à ce poste.
    pub games: u64,
    pub wins: u64,
    /// Champion avantagé en phase de couloir.
    pub advantage: String,
    /// Style de jeu conseillé : "defensive", "aggressive"...
    pub play_style: String,
    /// Conseil pour ce duel (OP.GG l'envoie en anglais, même en demandant le français).
    pub tip: String,
    /// Pages de runes proposées : celle de `analysis` d'abord, puis jusqu'à 2 autres parmi les plus jouées qui
    /// changent au moins une rune (pas seulement les fragments), du même style pour Kayn.
    pub pages: Vec<Runes>,
}

/// Un champion conseillé, avec son % de victoires.
#[derive(Serialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    pub champion_id: i64,
    pub name: String,
    pub win_rate: f64,
    pub games: u64,
}

/// Saison d'un joueur en classée (solo et flexible ensemble), d'après OP.GG : ses parties et victoires, ses
/// éliminations, morts et assistances (sur ses champions les plus joués), et ces champions (le plus joué d'abord).
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Season {
    pub games: u64,
    pub wins: u64,
    pub kills: u64,
    pub deaths: u64,
    pub assists: u64,
    pub champions: Vec<SeasonChampion>,
}

/// Un de ses champions cette saison.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SeasonChampion {
    pub champion_id: i64,
    pub name: String,
    pub games: u64,
    pub wins: u64,
    pub kills: u64,
    pub deaths: u64,
    pub assists: u64,
}

/// Sa saison d'après le profil OP.GG (champs SEASON_FIELDS).
fn season_from(doc: &Value) -> Season {
    let most = &doc["data"]["summoner"]["most_champions"];
    let count = |v: &Value, key: &str| v[key].as_u64().unwrap_or(0);
    let mut champions: Vec<SeasonChampion> = most["champion_stats"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(SeasonChampion {
                champion_id: c["id"].as_i64().filter(|id| *id > 0)?,
                name: c["champion_name"].as_str().unwrap_or_default().to_string(),
                games: count(c, "play"),
                wins: count(c, "win"),
                kills: count(c, "kill"),
                deaths: count(c, "death"),
                assists: count(c, "assist"),
            })
        })
        .collect();
    champions.sort_by_key(|c| std::cmp::Reverse(c.games));
    let total = |stat: fn(&SeasonChampion) -> u64| champions.iter().map(stat).sum();
    Season {
        games: count(most, "play"),
        wins: count(most, "win"),
        kills: total(|c| c.kills),
        deaths: total(|c| c.deaths),
        assists: total(|c| c.assists),
        champions,
    }
}

/// Un joueur du classement d'un champion (ses parties avec lui, son rang en solo/duo).
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TopPlayer {
    pub rank: u32,
    /// « Pseudo#TAG ».
    pub riot_id: String,
    /// Rang en solo/duo : "DIAMOND", 2, 73 LP ; vide si non classé.
    pub tier: String,
    pub division: u32,
    pub lp: u32,
    pub games: u64,
    pub wins: u64,
    pub kills: u64,
    pub deaths: u64,
    pub assists: u64,
}

/// Un champion dans le classement d'OP.GG d'un poste (sa « tier list »).
#[derive(Clone, Default, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaChamp {
    /// Numéro du champion dans le client ; 0 tant qu'il n'est pas retrouvé d'après son nom.
    pub champion_id: i64,
    /// Nom anglais (« Master Yi ») ; une fois retrouvé, celui du client.
    pub name: String,
    /// 1 (le meilleur) à 5.
    pub tier: u8,
    /// Place au classement de ce poste (1 = premier), et au patch précédent (0 : inconnue).
    pub rank: u32,
    pub rank_prev_patch: u32,
    /// Calculé sur ses parties et ses victoires (exact).
    pub win_rate: f64,
    /// Arrondis par OP.GG au centième (0,11), sans les nombres qui permettraient de les calculer.
    pub pick_rate: f64,
    pub ban_rate: f64,
    /// KDA moyen ((éliminations + assistances) / morts) et éliminations moyennes par partie.
    pub kda: f64,
    pub kills: f64,
    pub play: u64,
    /// Part de ses parties jouées à ce poste (0 à 1).
    pub role_rate: f64,
}

/// Pour aider à choisir : qui bat ce champion, et avec qui il s'entend le mieux.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Help {
    /// Champions qui gagnent contre lui à son poste, avec leur % de victoires contre lui.
    pub counters: Vec<Pick>,
    /// Ses meilleurs partenaires, par poste ("top", "jungle", "mid", "adc", "support"), avec le % du duo.
    pub synergies: HashMap<String, Vec<Pick>>,
}

pub struct Client {
    http: reqwest::Client,
    url: String,
    session: Option<String>,
    ready: bool,
    next_id: u64,
    /// Nom accepté par OP.GG pour chaque champion déjà demandé.
    names: HashMap<i64, String>,
}

impl Client {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .user_agent(APP)
            .build()
            .expect("impossible de créer le client HTTP");
        // Adresse remplaçable pour les tests (faux serveur OP.GG).
        let url = std::env::var("OPGG_MCP_URL").unwrap_or_else(|_| URL.to_string());
        Self { http, url, session: None, ready: false, next_id: 1, names: HashMap::new() }
    }

    /// Runes et build les plus joués pour ce champion à ce poste.
    pub async fn find(&mut self, champ: &Champ, position: &str, mode: &str) -> Result<Analysis, String> {
        let complete = |doc: &Value| analysis_from(doc).is_some_and(|a| !a.build.is_empty());
        let doc = self.champion_doc(champ, position, mode, &FIELDS, complete).await?;
        analysis_from(&doc).ok_or_else(|| UNREADABLE.to_string())
    }

    /// Part des parties jouées à chaque poste par ce champion, ex. [("top", 0.49), ("mid", 0.49)].
    pub async fn positions(&mut self, champ: &Champ, position: &str, mode: &str) -> Result<Vec<(String, f64)>, String> {
        let complete = |doc: &Value| doc["data"]["summary"]["positions"].is_array();
        let doc = self.champion_doc(champ, position, mode, &POSITION_FIELDS, complete).await?;
        Ok(doc["data"]["summary"]["positions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some((p["name"].as_str()?.to_lowercase(), p["stats"]["role_rate"].as_f64()?)))
            .collect())
    }

    /// Contres et partenaires d'un champion à un poste. Réponse complète (petite) : le filtre d'OP.GG
    /// ne propose pas les partenaires de tous les postes.
    pub async fn help(&mut self, champ: &Champ, position: &str, mode: &str) -> Result<Help, String> {
        let doc = self.champion_doc(champ, position, mode, &[], |_| true).await?;
        Ok(help_from(&doc))
    }

    /// Tous les champions qui battent ce champion à ce poste, du meilleur au moins bon, avec leur % de victoires
    /// contre lui. C'est la liste complète de ses duels, donnée avec son guide de couloir (contre `against`,
    /// n'importe quel adversaire courant) ; l'analyse générale n'en donne que 3.
    pub async fn counters(&mut self, champ: &Champ, against: &Champ, position: &str) -> Result<Vec<Pick>, String> {
        let args = json!({
            "position": position,
            "my_champion": self.name_of(champ),
            "opponent_champion": self.name_of(against),
            "lang": LANG,
            "client_app": APP,
        });
        let text = self.tool_text("lol_get_lane_matchup_guide", args).await?;
        let doc = to_json_doc(&text).ok_or_else(|| UNREADABLE.to_string())?;
        let id = doc["data"]["summary"]["id"].as_i64().unwrap_or(0);
        if id != 0 && id != champ.id {
            return Err(format!("OP.GG a renvoyé les duels d'un autre champion ({id})"));
        }
        Ok(counters_from(&doc))
    }

    /// Tout ce qu'OP.GG sait d'un champion à un poste, pour sa page : build, stats à chaque poste, duels, partenaires.
    pub async fn champion_page_doc(&mut self, champ: &Champ, position: &str) -> Result<Value, String> {
        let fields: Vec<&str> = FIELDS.iter().chain(&PAGE_FIELDS).copied().collect();
        let complete = |doc: &Value| analysis_from(doc).is_some() && doc["data"]["summary"]["positions"].is_array();
        self.champion_doc(champ, position, "ranked", &fields, complete).await
    }

    /// Tous les champions (numéro, nom dans la langue de l'app, nom interne), quand le client est fermé.
    pub async fn champion_list(&mut self) -> Result<Vec<(i64, String, String)>, String> {
        let args = json!({ "lang": LANG, "client_app": APP, "desired_output_fields": ["data.champions[].{champion_id,key,name}"] });
        let text = self.tool_text("lol_list_champions", args).await?;
        let doc = to_json_doc(&text).ok_or_else(|| UNREADABLE.to_string())?;
        let list: Vec<(i64, String, String)> = doc["data"]["champions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| Some((c["champion_id"].as_i64()?, c["name"].as_str()?.to_string(), c["key"].as_str()?.to_string())))
            // Sans les formes d'un mode de jeu (« Jade_Annie »… à partir de 60 000).
            .filter(|(id, _, _)| (1..10_000).contains(id))
            .collect();
        if list.is_empty() {
            return Err(UNREADABLE.to_string());
        }
        Ok(list)
    }

    /// Les joueurs qui ont le plus de parties avec ce champion dans cette région (« euw »…), du premier au dernier.
    pub async fn leaderboard(&mut self, champ: &Champ, region: &str) -> Result<Vec<TopPlayer>, String> {
        let args = json!({
            "region": region.to_lowercase(), "champion": self.name_of(champ), "client_app": APP,
            "desired_output_fields": LEADERBOARD_FIELDS,
        });
        let text = self.tool_text("lol_list_champion_leaderboard", args).await?;
        let doc = to_json_doc(&text).ok_or_else(|| UNREADABLE.to_string())?;
        let number = |v: &Value| v.as_u64().unwrap_or(0);
        Ok(doc["leaderboard"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let who = &entry["summoner"];
                let name = who["game_name"].as_str().filter(|n| !n.is_empty())?;
                let solo = who["league_stats"].as_array().into_iter().flatten().find(|l| l["game_type"] == "SOLORANKED");
                let info = solo.map(|l| &l["tier_info"]);
                let stat = &entry["most_champion_stat"];
                Some(TopPlayer {
                    rank: number(&entry["rank"]) as u32,
                    riot_id: format!("{name}#{}", who["tagline"].as_str().unwrap_or_default()),
                    tier: info.and_then(|i| i["tier"].as_str()).unwrap_or_default().to_string(),
                    division: info.map_or(0, |i| number(&i["division"]) as u32),
                    lp: info.map_or(0, |i| number(&i["lp"]) as u32),
                    games: number(&stat["play"]),
                    wins: number(&stat["win"]),
                    kills: number(&stat["kill"]),
                    deaths: number(&stat["death"]),
                    assists: number(&stat["assist"]),
                })
            })
            .collect())
    }

    /// Classement d'OP.GG de chaque poste ("top", "jungle", "mid", "adc", "support"), du premier au dernier :
    /// une seule petite réponse (15 ko environ) pour tous les champions.
    pub async fn lane_meta(&mut self) -> Result<HashMap<String, Vec<MetaChamp>>, String> {
        let args = json!({
            "position": "all", "lang": META_LANG, "client_app": APP, "desired_output_fields": META_FIELDS,
        });
        let text = self.tool_text("lol_list_lane_meta_champions", args).await?;
        let meta = to_json_doc(&text).map(|doc| meta_from(&doc)).unwrap_or_default();
        if meta.values().all(Vec::is_empty) {
            return Err(UNREADABLE.to_string());
        }
        Ok(meta)
    }

    /// Runes, build et conseils d'OP.GG pour ton champion contre ton adversaire direct.
    /// Réponse d'OP.GG pour le duel, à lire avec `matchup_from` (une fois par style si besoin).
    pub async fn matchup_doc(&mut self, me: &Champ, opponent: &Champ, position: &str) -> Result<Value, String> {
        let args = json!({
            "position": position,
            "my_champion": self.name_of(me),
            "opponent_champion": self.name_of(opponent),
            "lang": LANG,
            "client_app": APP,
        });
        let text = self.tool_text("lol_get_lane_matchup_guide", args).await?;
        let doc = to_json_doc(&text).ok_or_else(|| UNREADABLE.to_string())?;
        let id = doc["data"]["summary"]["id"].as_i64().unwrap_or(0);
        if id != 0 && id != me.id {
            return Err(format!("OP.GG a renvoyé le duel d'un autre champion ({id})"));
        }
        matchup_from(&doc, opponent.id, None).ok_or_else(|| UNREADABLE.to_string())?;
        Ok(doc)
    }

    /// Analyse d'un champion, en essayant les noms possibles ; retient celui qu'OP.GG accepte.
    async fn champion_doc(
        &mut self,
        champ: &Champ,
        position: &str,
        mode: &str,
        fields: &[&str],
        complete: fn(&Value) -> bool,
    ) -> Result<Value, String> {
        let mut last_error = "Champion introuvable sur OP.GG".to_string();
        for candidate in self.candidates(champ) {
            match self.analysis_doc(&candidate, position, mode, fields, complete).await {
                Ok(doc) => {
                    let id = doc["data"]["summary"]["id"].as_i64().unwrap_or(0);
                    if id == 0 || id == champ.id {
                        eprintln!("[opgg] champion=\"{candidate}\" : trouvé");
                        self.names.insert(champ.id, candidate);
                        return Ok(doc);
                    }
                    eprintln!("[opgg] champion=\"{candidate}\" : OP.GG a renvoyé le champion {id}");
                    last_error = "OP.GG a renvoyé un autre champion".to_string();
                }
                Err(e) => {
                    eprintln!("[opgg] champion=\"{candidate}\" : {e}");
                    last_error = e;
                }
            }
        }
        Err(last_error)
    }

    /// Noms à essayer : celui déjà accepté, puis le nom interne, le nom affiché et le numéro.
    fn candidates(&self, champ: &Champ) -> Vec<String> {
        let mut list: Vec<String> = self.names.get(&champ.id).cloned().into_iter().collect();
        for name in [opgg_name(&champ.alias), opgg_name(&champ.name), champ.id.to_string()] {
            if !name.is_empty() && !list.contains(&name) {
                list.push(name);
            }
        }
        list
    }

    /// Nom pour l'outil des duels, qui attend un nom en majuscules (pas un numéro).
    fn name_of(&self, champ: &Champ) -> String {
        match self.names.get(&champ.id) {
            Some(name) if !name.chars().all(|c| c.is_ascii_digit()) => name.clone(),
            _ if !champ.alias.is_empty() => opgg_name(&champ.alias),
            _ => opgg_name(&champ.name),
        }
    }

    /// Si la réponse filtrée ne contient pas ce qu'il faut, on redemande la réponse complète, au format connu.
    /// Sans champs (`fields` vide), on demande directement la réponse complète.
    async fn analysis_doc(
        &mut self,
        champion: &str,
        position: &str,
        mode: &str,
        fields: &[&str],
        complete: fn(&Value) -> bool,
    ) -> Result<Value, String> {
        let mut args = json!({
            "champion": champion,
            "position": position,
            "game_mode": mode,
            "lang": LANG,
            "client_app": APP,
        });
        if let Some(tier) = TIER {
            args["tier"] = json!(tier);
        }
        if fields.is_empty() {
            return self.analysis(args).await;
        }
        let mut filtered_args = args.clone();
        filtered_args["desired_output_fields"] = json!(fields);
        let filtered = self.analysis(filtered_args).await;
        match &filtered {
            Ok(doc) if complete(doc) => return filtered,
            Err(e) if e != UNREADABLE => return filtered,
            _ => {}
        }
        eprintln!("[opgg] réponse filtrée incomplète, nouvelle demande sans filtre");
        match self.analysis(args).await {
            Ok(doc) => Ok(doc),
            // On garde au moins la première réponse, si elle était lisible.
            Err(e) => filtered.or(Err(e)),
        }
    }

    async fn analysis(&mut self, args: Value) -> Result<Value, String> {
        let text = self.tool_text("lol_get_champion_analysis", args).await?;
        to_json_doc(&text).ok_or_else(|| UNREADABLE.to_string())
    }

    /// Saison d'un joueur en classée (voir `Season`), ou l'erreur d'OP.GG (« Summoner not found »…).
    pub async fn season(&mut self, name: &str, tag: &str, region: &str) -> Result<Season, String> {
        let args = json!({
            "game_name": name, "tag_line": tag, "region": region, "lang": LANG, "client_app": APP,
            "desired_output_fields": SEASON_FIELDS,
        });
        let text = self.tool_text("lol_get_summoner_profile", args).await?;
        let doc = to_json_doc(&text).ok_or_else(|| UNREADABLE.to_string())?;
        Ok(season_from(&doc))
    }

    /// Profil d'un joueur (niveau, rangs), ou l'erreur d'OP.GG (« Summoner not found »…).
    pub async fn summoner_profile(&mut self, name: &str, tag: &str, region: &str) -> Result<Value, String> {
        let args = json!({
            "game_name": name, "tag_line": tag, "region": region, "lang": LANG, "client_app": APP,
            "desired_output_fields": PROFILE_FIELDS,
        });
        let text = self.tool_text("lol_get_summoner_profile", args).await?;
        to_json_doc(&text).ok_or_else(|| UNREADABLE.to_string())
    }

    /// Ses 20 dernières parties.
    pub async fn summoner_matches(&mut self, name: &str, tag: &str, region: &str) -> Result<Value, String> {
        let args = json!({
            "game_name": name, "tag_line": tag, "region": region, "limit": 20, "lang": LANG, "client_app": APP,
            "desired_output_fields": MATCH_FIELDS,
        });
        let text = self.tool_text("lol_list_summoner_matches", args).await?;
        to_json_doc(&text).ok_or_else(|| UNREADABLE.to_string())
    }

    /// Une de ses parties en détail ; `riot_id` (Nom#TAG) : le joueur cherché, qu'OP.GG marque « is_target ».
    pub async fn game_detail(&mut self, region: &str, id: &str, created_at: &str, riot_id: &str) -> Result<Value, String> {
        let args = json!({
            "region": region, "game_id": id, "created_at": created_at, "focus_riot_id": riot_id,
            "lang": LANG, "client_app": APP, "desired_output_fields": DETAIL_FIELDS,
        });
        let text = self.tool_text("lol_get_summoner_game_detail", args).await?;
        to_json_doc(&text).ok_or_else(|| UNREADABLE.to_string())
    }

    /// Texte renvoyé par un outil OP.GG, ou son message d'erreur.
    async fn tool_text(&mut self, tool: &str, args: Value) -> Result<String, String> {
        let result = self.call_tool(tool, args).await?;
        let text = result["content"][0]["text"].as_str().unwrap_or_default().to_string();
        if result["isError"] == true {
            return Err(format!("OP.GG : {}", text.chars().take(120).collect::<String>()));
        }
        Ok(text)
    }

    async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let mut last_error = String::new();
        // Deux essais : si la session a expiré, on refait la poignée de main.
        for _ in 0..2 {
            if !self.ready {
                self.initialize().await?;
            }
            let id = self.take_id();
            let request = json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": name, "arguments": arguments },
            });
            match self.post(&request).await {
                Ok((Some(msg), _)) => {
                    if let Some(error) = msg.get("error") {
                        return Err(format!("OP.GG : {}", error["message"].as_str().unwrap_or("erreur")));
                    }
                    return Ok(msg["result"].clone());
                }
                Ok((None, _)) => last_error = "Réponse OP.GG vide".to_string(),
                Err(e) => last_error = e,
            }
            self.ready = false;
            self.session = None;
        }
        Err(last_error)
    }

    /// Poignée de main du protocole MCP : initialize, puis notifications/initialized.
    async fn initialize(&mut self) -> Result<(), String> {
        let id = self.take_id();
        let init = json!({
            "jsonrpc": "2.0", "id": id, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "lol-companion", "version": env!("CARGO_PKG_VERSION") },
            },
        });
        let (_, session) = self.post(&init).await?;
        self.session = session;
        self.post(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await?;
        self.ready = true;
        Ok(())
    }

    /// Envoie un message JSON-RPC. Renvoie la réponse (JSON ou flux SSE) et l'identifiant de session.
    async fn post(&self, body: &Value) -> Result<(Option<Value>, Option<String>), String> {
        let mut req = self
            .http
            .post(&self.url)
            .header("Accept", "application/json, text/event-stream")
            .json(body);
        if let Some(session) = &self.session {
            req = req.header("Mcp-Session-Id", session);
        }
        let resp = req.send().await.map_err(|_| "OP.GG injoignable (connexion internet ?)".to_string())?;
        let status = resp.status();
        let session = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .or_else(|| self.session.clone());
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(format!("OP.GG a répondu HTTP {}", status.as_u16()));
        }
        Ok((decode_message(&text), session))
    }

    fn take_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

/// Réponse JSON directe, ou flux SSE avec des lignes « data: {...} ».
fn decode_message(body: &str) -> Option<Value> {
    let body = body.trim();
    if body.starts_with('{') {
        return serde_json::from_str(body).ok();
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
        .find(|msg| msg.get("id").is_some())
}

/// Nom attendu par OP.GG : majuscules, mots séparés par « _ », sans ponctuation
/// (ex. « Dr. Mundo » ou « DrMundo » donnent DR_MUNDO, « Kai'Sa » donne KAISA).
fn opgg_name(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in raw.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower {
                out.push('_');
            }
            out.extend(c.to_uppercase());
            prev_lower = c.is_lowercase();
        } else {
            if (c.is_whitespace() || c == '_') && !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            prev_lower = false;
        }
    }
    out.trim_end_matches('_').to_string()
}

// ---------- Lecture de la réponse ----------
// OP.GG renvoie un texte de la forme :
//   class Data: summary,damage_type,...
//   class Runes: id,primary_page_id,...
//   (ligne vide)
//   LolGetChampionAnalysis("Ahri","MID",Data(Summary(103,...),"AP",...,Runes(8112,8100,...),...))
// Les lignes « class » donnent les noms des champs de chaque objet, dans l'ordre :
// on s'en sert pour transformer le tout en JSON ordinaire.
// L'outil des duels, lui, renvoie directement du JSON.

pub(crate) fn analysis_from(doc: &Value) -> Option<Analysis> {
    let data = &doc["data"];
    let runes = build_runes(data["summary"]["id"].as_i64().unwrap_or(0), &data["runes"])?;
    Some(Analysis { runes, build: build_build(data) })
}

/// Réponse de l'outil des duels : chaque liste est triée de la plus jouée à la moins jouée.
/// `style` (Kayn rouge ou bleu) : seulement les pages de runes, objets principaux et autres objets qui lui vont
/// (s'il n'y en a aucun dans une liste, toute la liste).
pub fn matchup_from(doc: &Value, opponent_id: i64, style: Option<&Style>) -> Option<Matchup> {
    let d = &doc["data"];
    let list = |key: &str| d[key].as_array().map(Vec::as_slice).unwrap_or_default();
    let page = style.and_then(|s| list("runes").iter().find(|p| (s.runes)(p))).or(list("runes").first())?;
    let champion_id = d["summary"]["id"].as_i64().unwrap_or(0);
    let runes = build_runes(champion_id, page)?;
    // Autres pages : les plus jouées ensuite qui changent au moins une rune (les 6 premières de `perks`).
    let mut pages = vec![runes.clone()];
    let others = list("runes").iter().filter(|p| style.is_none_or(|s| (s.runes)(p))).filter_map(|p| build_runes(champion_id, p));
    for other in others.filter(|r| r.games >= MIN_PAGE_GAMES) {
        if pages.len() < PAGES_SHOWN && !pages.iter().any(|p| p.perks[..6] == other.perks[..6]) {
            pages.push(other);
        }
    }
    let fits = |e: &Value| e["ids"][0].as_i64().is_some_and(|id| style.is_none_or(|s| (s.item)(id)));
    // 4e, 5e et 6e objets : les 3 plus achetés à chaque étape (au moins 10 parties).
    let step = |depth: i64| -> Vec<Value> {
        d["single_items"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|s| s["depth"] == depth)
            .and_then(|s| s["items"].as_array())
            .map(|items| items.iter().filter(|i| i["play"].as_u64().unwrap_or(0) >= 10 && fits(i)).take(3).cloned().collect())
            .unwrap_or_default()
    };
    let pick = |key: &str| best(list(key)).unwrap_or(&NULL);
    let ids = |e: &Value| -> Vec<i64> { e["ids"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect() };
    let cores: Vec<&Value> = list("core_items").iter().filter(|c| style.is_none_or(|s| (s.core)(&ids(c)))).collect();
    let core = if cores.is_empty() { pick("core_items") } else { best(cores.iter().copied()).unwrap_or(&NULL) };
    // Même forme que l'analyse générale, pour la lire de la même façon.
    let shaped = json!({
        "starter_items": pick("starter_items"),
        "core_items": core,
        "boots": pick("boots"),
        "fourth_items": step(4),
        "fifth_items": step(5),
        "sixth_items": step(6),
        "summoner_spells": d["summoner_spells"][0],
        "skills": d["skills"][0],
        "skill_masteries": d["skill_masteries"][0],
    });
    let mut build = build_build(&shaped);
    // Les sets d'objets du duel : les plus joués (OP.GG les donne dans cet ordre), le classique d'abord.
    let all_cores = list("core_items");
    let set_list: Vec<&Value> = if cores.is_empty() { all_cores.iter().collect() } else { cores };
    let sets: Vec<CoreSet> = set_list
        .into_iter()
        .filter(|c| c["play"].as_u64().unwrap_or(0) >= MIN_SET_GAMES)
        .take(SETS_SHOWN)
        .map(core_set)
        .filter(|set| !set.items.is_empty())
        .collect();
    if !sets.is_empty() {
        build.sets = sets;
    }
    // Autre choix pour le 1er (puis le 2e) objet : le meilleur des objets achetés à ce rang, hors ceux déjà retenus.
    let other = |depth: i64, taken: &[i64]| -> Option<Item> {
        let rank = d["single_items"].as_array()?.iter().find(|s| s["depth"] == depth)?;
        let free = rank["items"].as_array()?.iter().filter(|e| fits(e) && e["ids"][0].as_i64().is_some_and(|id| !taken.contains(&id)));
        items_of(best(free)?).into_iter().next()
    };
    // Jamais un des 3 objets principaux (le 3e n'est pas un autre choix du 2e).
    let mut taken: Vec<i64> = build.core.iter().map(|i| i.id).collect();
    build.first_alt = other(1, &taken);
    taken.extend(build.first_alt.as_ref().map(|i| i.id));
    build.second_alt = other(2, &taken);
    let versus = d["counters"].as_array().into_iter().flatten().find(|c| c["champion_id"] == opponent_id);
    let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
    Some(Matchup {
        analysis: Analysis { runes, build },
        games: versus.and_then(|c| c["play"].as_u64()).unwrap_or(0),
        wins: versus.and_then(|c| c["win"].as_u64()).unwrap_or(0),
        advantage: text(&d["lane_advantage_champion"]),
        play_style: text(&d["recommended_play_style"]),
        tip: text(&d["opponent_champion_tip"]),
        pages,
    })
}

/// Ses duels (« counters » : ses parties et ses victoires contre chaque adversaire), vus de l'autre côté :
/// le % de victoires de chaque adversaire contre lui, du plus haut au plus bas. Seuls comptent les duels
/// assez joués (au moins 0,5 % de ses parties) : sur trop peu de parties, le % ne veut rien dire.
fn counters_from(doc: &Value) -> Vec<Pick> {
    let list = doc["data"]["counters"].as_array().map(Vec::as_slice).unwrap_or_default();
    let total: u64 = list.iter().filter_map(|c| c["play"].as_u64()).sum();
    let mut picks: Vec<Pick> = list
        .iter()
        .filter_map(|c| {
            let games = c["play"].as_u64().filter(|&games| games > 0 && games * 200 >= total)?;
            let wins = c["win"].as_u64()?.min(games);
            Some(Pick {
                champion_id: c["champion_id"].as_i64()?,
                name: c["champion_name"].as_str().unwrap_or_default().to_string(),
                win_rate: (games - wins) as f64 / games as f64,
                games,
            })
        })
        .collect();
    picks.sort_by(|a, b| b.win_rate.total_cmp(&a.win_rate).then(b.games.cmp(&a.games)));
    picks
}

/// Réponse du classement : pour chaque poste, ses champions du premier au dernier (sans ceux retirés du jeu).
fn meta_from(doc: &Value) -> HashMap<String, Vec<MetaChamp>> {
    let positions = &doc["data"]["positions"];
    let entry = |c: &Value| -> Option<MetaChamp> {
        if c["is_rip"] == true {
            return None;
        }
        Some(MetaChamp {
            champion_id: 0,
            name: c["champion"].as_str().filter(|name| !name.is_empty())?.to_string(),
            tier: c["tier"].as_u64().filter(|tier| (1..=5).contains(tier))? as u8,
            rank: c["rank"].as_u64().filter(|rank| *rank > 0)? as u32,
            rank_prev_patch: c["rank_prev_patch"].as_u64().unwrap_or(0) as u32,
            win_rate: exact_win_rate(c).unwrap_or_else(|| c["win_rate"].as_f64().unwrap_or(0.0)),
            pick_rate: c["pick_rate"].as_f64().unwrap_or(0.0),
            ban_rate: c["ban_rate"].as_f64().unwrap_or(0.0),
            kda: c["kda"].as_f64().unwrap_or(0.0),
            kills: c["kill"].as_f64().unwrap_or(0.0),
            play: c["play"].as_u64().unwrap_or(0),
            role_rate: c["role_rate"].as_f64().unwrap_or(0.0),
        })
    };
    META_POSITIONS
        .iter()
        .map(|position| {
            let mut list: Vec<MetaChamp> = positions[position].as_array().into_iter().flatten().filter_map(entry).collect();
            list.sort_by_key(|c| c.rank);
            (position.to_string(), list)
        })
        .collect()
}

/// « weak_counters » : les champions qui le battent ; « synergies » : ses meilleurs duos, par poste du partenaire.
/// OP.GG les donne déjà triés, du meilleur au moins bon.
fn help_from(doc: &Value) -> Help {
    let d = &doc["data"];
    // `win` : les victoires du champion analysé, contre ce contre ou avec ce partenaire.
    let picks = |list: &Value, id: &str, name: &str, counter: bool| -> Vec<Pick> {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let win_rate = match exact_win_rate(c) {
                    Some(rate) if counter => 1.0 - rate,
                    Some(rate) => rate,
                    None => c[if counter { "counter_win_rate" } else { "win_rate" }].as_f64().unwrap_or(0.0),
                };
                Some(Pick {
                    champion_id: c[id].as_i64()?,
                    name: c[name].as_str().unwrap_or_default().to_string(),
                    win_rate,
                    games: c["play"].as_u64().unwrap_or(0),
                })
            })
            .collect()
    };
    let synergies = d["synergies"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(position, list)| {
            (position.clone(), picks(list, "synergy_champion_id", "synergy_champion_name", false))
        })
        .collect();
    Help { counters: picks(&d["weak_counters"], "champion_id", "champion_name", true), synergies }
}

/// % de victoires d'après ses parties et ses victoires (`play`, `win`) : OP.GG arrondit les siens au centième
/// (0,51 pour 51,3 %).
fn exact_win_rate(entry: &Value) -> Option<f64> {
    let games = entry["play"].as_u64().filter(|&games| games > 0)?;
    Some(entry["win"].as_u64()?.min(games) as f64 / games as f64)
}

pub(crate) fn to_json_doc(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    // Si un jour OP.GG renvoie du vrai JSON, on le lit directement.
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed).ok();
    }
    let mut classes: HashMap<String, Vec<String>> = HashMap::new();
    let mut body = String::new();
    for line in text.lines() {
        match line.strip_prefix("class ").and_then(|rest| rest.split_once(':')) {
            Some((name, fields)) => {
                classes.insert(name.trim().to_string(), fields.split(',').map(|f| f.trim().to_string()).collect());
            }
            None => {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    let tree = Parser { b: body.as_bytes(), i: 0 }.value()?;
    Some(to_json(&tree, &classes))
}

fn build_runes(champion_id: i64, r: &Value) -> Option<Runes> {
    let ids = |k: &str| -> Vec<i64> { r[k].as_array().into_iter().flatten().filter_map(Value::as_i64).collect() };
    let mut perks = ids("primary_rune_ids");
    perks.extend(ids("secondary_rune_ids"));
    perks.extend(ids("stat_mod_ids"));
    if perks.len() != 9 {
        return None;
    }
    let text = |k: &str| r[k].as_str().unwrap_or_default().to_string();
    let names = |k: &str| -> Vec<String> { r[k].as_array().into_iter().flatten().map(|n| n.as_str().unwrap_or_default().to_string()).collect() };
    let mut rune_names = names("primary_rune_names");
    rune_names.extend(names("secondary_rune_names"));
    Some(Runes {
        champion_id,
        primary_style: r["primary_page_id"].as_i64()?,
        sub_style: r["secondary_page_id"].as_i64()?,
        perks,
        keystone: r["primary_rune_names"][0].as_str().unwrap_or_default().to_string(),
        primary_name: text("primary_page_name"),
        secondary_name: text("secondary_page_name"),
        rune_names,
        games: r["play"].as_u64().unwrap_or(0),
        wins: r["win"].as_u64().unwrap_or(0),
    })
}

/// Le choix retenu dans une liste OP.GG (triée de la plus jouée à la moins jouée) :
/// le meilleur % de victoires parmi les choix joués au moins `MIN_PLAY` fois, sinon le plus joué.
fn best<'a>(entries: impl IntoIterator<Item = &'a Value>) -> Option<&'a Value> {
    let entries: Vec<&Value> = entries.into_iter().collect();
    let rate = |e: &Value| e["win"].as_f64().unwrap_or(0.0) / e["play"].as_f64().unwrap_or(0.0).max(1.0);
    let mut chosen: Option<&Value> = None;
    for &e in entries.iter().filter(|e| e["play"].as_u64().unwrap_or(0) >= MIN_PLAY) {
        // À égalité, on garde le plus joué (le premier).
        if chosen.is_none_or(|c| rate(e) > rate(c)) {
            chosen = Some(e);
        }
    }
    chosen.or_else(|| entries.first().copied())
}

/// Objets d'une entrée « CoreItems » ; les doublons sont regroupés (2 potions = une ligne ×2).
pub(crate) fn items_of(entry: &Value) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    let ids = entry["ids"].as_array().into_iter().flatten().enumerate();
    for (i, id) in ids.filter_map(|(i, v)| Some((i, v.as_i64()?))) {
        match out.iter_mut().find(|it| it.id == id) {
            Some(it) => it.count += 1,
            None => {
                let name = entry["ids_names"][i].as_str().map_or_else(|| id.to_string(), str::to_string);
                let (games, wins) = (entry["play"].as_u64().unwrap_or(0), entry["win"].as_u64().unwrap_or(0));
                out.push(Item { id, name, count: 1, games, wins });
            }
        }
    }
    out
}

fn build_build(data: &Value) -> Build {
    let items = items_of;
    let core = items(&data["core_items"]);
    let boots = items(&data["boots"]);
    let mut options: Vec<Item> = Vec::new();
    for slot in ["fourth_items", "fifth_items", "sixth_items"] {
        for entry in data[slot].as_array().into_iter().flatten() {
            for item in items(entry) {
                if !core.iter().chain(&boots).chain(&options).any(|it| it.id == item.id) {
                    options.push(item);
                }
            }
        }
    }
    let texts = |v: &Value| -> Vec<String> {
        v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
    };
    let sets = if core.is_empty() { Vec::new() } else { vec![core_set(&data["core_items"])] };
    Build {
        starter: items(&data["starter_items"]),
        core,
        boots,
        sets,
        first_alt: None,
        second_alt: None,
        options,
        spells: data["summoner_spells"]["ids"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect(),
        skill_max: texts(&data["skill_masteries"]["ids"]),
        skill_order: texts(&data["skills"]["order"]),
    }
}

/// Valeur du texte OP.GG : nombre, texte, liste, ou objet « Nom(arguments) ».
#[derive(Debug)]
enum V {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    List(Vec<V>),
    Obj(String, Vec<V>),
}

/// Un objet « Nom(a, b) » devient {"champ1": a, "champ2": b} grâce à sa ligne « class Nom: champ1,champ2 ».
fn to_json(v: &V, classes: &HashMap<String, Vec<String>>) -> Value {
    match v {
        V::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => json!(*n as i64),
        V::Num(n) => json!(n),
        V::Str(s) => json!(s),
        V::Bool(b) => json!(b),
        V::Null => Value::Null,
        V::List(items) => Value::Array(items.iter().map(|x| to_json(x, classes)).collect()),
        V::Obj(name, args) => {
            let names = classes.get(name).map(Vec::as_slice).unwrap_or_default();
            Value::Object(names.iter().zip(args).map(|(k, x)| (k.clone(), to_json(x, classes))).collect())
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&mut self) -> Option<u8> {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
        self.b.get(self.i).copied()
    }

    fn value(&mut self) -> Option<V> {
        match self.peek()? {
            b'[' => self.seq(b']').map(V::List),
            b'"' => self.string().map(V::Str),
            c if c == b'-' || c.is_ascii_digit() => self.number(),
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = self.i;
                while self.i < self.b.len() && (self.b[self.i].is_ascii_alphanumeric() || self.b[self.i] == b'_') {
                    self.i += 1;
                }
                let ident = std::str::from_utf8(&self.b[start..self.i]).ok()?.to_string();
                if self.peek() == Some(b'(') {
                    return self.seq(b')').map(|args| V::Obj(ident, args));
                }
                Some(match ident.as_str() {
                    "true" | "True" => V::Bool(true),
                    "false" | "False" => V::Bool(false),
                    "null" | "None" => V::Null,
                    _ => V::Str(ident),
                })
            }
            _ => None,
        }
    }

    /// Liste « [a,b] » ou arguments « (a,b) » : on est sur le caractère ouvrant.
    fn seq(&mut self, close: u8) -> Option<Vec<V>> {
        self.i += 1;
        let mut items = Vec::new();
        if self.peek()? == close {
            self.i += 1;
            return Some(items);
        }
        loop {
            items.push(self.value()?);
            match self.peek()? {
                b',' => self.i += 1,
                c if c == close => {
                    self.i += 1;
                    return Some(items);
                }
                _ => return None,
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        self.i += 1;
        let mut out = Vec::new();
        while let Some(&c) = self.b.get(self.i) {
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let esc = *self.b.get(self.i)?;
                    self.i += 1;
                    out.push(match esc {
                        b'n' => b'\n',
                        b't' => b'\t',
                        other => other,
                    });
                }
                _ => out.push(c),
            }
        }
        None
    }

    fn number(&mut self) -> Option<V> {
        let start = self.i;
        while self.i < self.b.len() && matches!(self.b[self.i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
            self.i += 1;
        }
        std::str::from_utf8(&self.b[start..self.i]).ok()?.parse().ok().map(V::Num)
    }
}
