//! Salon d'un autre joueur, cherché par son Riot ID et sa région. Dans la région de ton client, l'app le lit dans
//! le client (comme ton salon, avec le détail des parties et le score carry) ; dans les autres régions, ou si le
//! client ne le trouve pas, sur OP.GG (rangs, 20 dernières parties, détail des parties avec la note OP.GG).

use crate::{history, lol, opgg};
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashMap, sync::Mutex};

/// Le salon d'un joueur cherché.
#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Searched {
    /// Riot ID tel que la source l'écrit : « Nom#TAG ».
    pub riot_id: String,
    pub level: Option<u64>,
    pub region: String,
    /// "client" (ton client LoL) ou "opgg".
    pub source: String,
    pub profile: lol::Profile,
}

/// Une partie lue sur OP.GG : ce qu'il faut pour en demander le détail.
struct OpggGame {
    region: String,
    id: String,
    created_at: String,
    riot_id: String,
}

pub struct Search {
    opgg: tokio::sync::Mutex<opgg::Client>,
    /// Parties OP.GG des joueurs cherchés, par numéro donné par l'app (voir `game_number`).
    games: Mutex<HashMap<i64, OpggGame>>,
    /// Celles déjà ouvertes en détail.
    details: Mutex<HashMap<i64, history::MatchDetails>>,
}

impl Search {
    pub fn new() -> Self {
        Self { opgg: tokio::sync::Mutex::new(opgg::Client::new()), games: Mutex::default(), details: Mutex::default() }
    }

    pub async fn find(&self, assets: &lol::Assets, riot_id: &str, region: &str) -> Result<Searched, String> {
        let (name, tag) = riot_id
            .trim()
            .rsplit_once('#')
            .map(|(name, tag)| (name.trim(), tag.trim()))
            .filter(|(name, tag)| !name.is_empty() && !tag.is_empty())
            .ok_or("Écris le Riot ID en entier : Nom#TAG")?;
        let region = region.trim().to_uppercase();
        if assets.client_region().await.as_deref() == Some(region.as_str()) {
            if let Some(found) = from_client(assets, name, tag, &region).await {
                eprintln!("[recherche] {name}#{tag} ({region}) : lu dans le client, {} parties", found.profile.matches.len());
                return Ok(found);
            }
            eprintln!("[recherche] {name}#{tag} ({region}) : introuvable dans le client, essai sur OP.GG");
        }
        let mut client = self.opgg.lock().await;
        let profile = client.summoner_profile(name, tag, &region).await.map_err(|e| explain(&e, &region))?;
        let matches = client.summoner_matches(name, tag, &region).await.unwrap_or_else(|e| {
            eprintln!("[recherche] {name}#{tag} : parties OP.GG illisibles ({e})");
            Value::Null
        });
        drop(client);
        let (found, games) = from_opgg(&profile, &matches, &region, &format!("{name}#{tag}"));
        eprintln!("[recherche] {} ({region}) : lu sur OP.GG, {} parties", found.riot_id, found.profile.matches.len());
        self.games.lock().unwrap().extend(games);
        Ok(found)
    }

    /// Une partie lue sur OP.GG, en détail : les 10 joueurs et leur note OP.GG.
    pub async fn match_details(&self, game_id: i64) -> Result<history::MatchDetails, String> {
        if let Some(details) = self.details.lock().unwrap().get(&game_id) {
            return Ok(details.clone());
        }
        let game = self.games.lock().unwrap().get(&game_id).map(|g| {
            (g.region.clone(), g.id.clone(), g.created_at.clone(), g.riot_id.clone())
        });
        let (region, id, created_at, riot_id) = game.ok_or("Partie inconnue : refais la recherche.")?;
        let doc = self.opgg.lock().await.game_detail(&region, &id, &created_at, &riot_id).await?;
        let details = details_from(&doc, game_id, &riot_id).ok_or("OP.GG n'a pas donné le détail de cette partie.")?;
        self.details.lock().unwrap().insert(game_id, details.clone());
        Ok(details)
    }
}

/// Le joueur dans le client (sa région seulement) ; None s'il est introuvable ou si le client est fermé.
async fn from_client(assets: &lol::Assets, name: &str, tag: &str, region: &str) -> Option<Searched> {
    // Le client va chercher ce joueur, son rang et ses parties chez Riot.
    let summoner = assets.client_json(&format!("/lol-summoner/v1/summoners?name={}", encode(&format!("{name}#{tag}"))), true).await?;
    let puuid = summoner["puuid"].as_str().filter(|p| !p.is_empty())?.to_string();
    let ranked = assets.client_json(&format!("/lol-ranked/v1/ranked-stats/{puuid}"), true).await;
    let path = format!("/lol-match-history/v1/products/lol/{puuid}/matches?begIndex=0&endIndex=19");
    let list = assets.client_json(&path, true).await;
    let games = list.as_ref().and_then(|l| l["games"]["games"].as_array()).cloned().unwrap_or_default();
    let summoner_id = summoner["summonerId"].as_u64();
    let records = games.iter().filter_map(|g| history::record_from(g, Some(&puuid), summoner_id, 0)).collect();
    let solo = ranked.as_ref().map(|r| lol::rank_in(r, "RANKED_SOLO_5x5"));
    let flex = ranked.as_ref().map(|r| lol::rank_in(r, "RANKED_FLEX_SR"));
    let mut profile = lol::summarize(solo, flex, records);
    for champion in &mut profile.recent_champions {
        champion.name = assets.champion_name(champion.champion_id).await;
    }
    for record in &mut profile.matches {
        record.champion = assets.champion_name(record.champion_id).await;
    }
    let text = |key: &str| summoner[key].as_str().unwrap_or_default().to_string();
    let riot_id = match (text("gameName"), text("tagLine")) {
        (game_name, tag_line) if !game_name.is_empty() => format!("{game_name}#{tag_line}"),
        _ => format!("{name}#{tag}"),
    };
    let source = "client".to_string();
    Some(Searched { riot_id, level: summoner["summonerLevel"].as_u64(), region: region.to_string(), source, profile })
}

/// Erreur d'OP.GG, dite simplement.
fn explain(error: &str, region: &str) -> String {
    if error.to_lowercase().contains("not found") {
        format!("Joueur introuvable en {region} : vérifie le Riot ID (Nom#TAG) et la région.")
    } else {
        error.to_string()
    }
}

/// Profil et parties d'OP.GG en salon, et de quoi demander le détail de chaque partie (par son numéro).
fn from_opgg(profile: &Value, matches: &Value, region: &str, asked: &str) -> (Searched, Vec<(i64, OpggGame)>) {
    let summoner = &profile["data"]["summoner"];
    let rank = |game_type: &str| {
        let stats = summoner["league_stats"].as_array().into_iter().flatten();
        Some(stats.into_iter().find(|s| s["game_type"] == game_type).map(rank_of).unwrap_or_default())
    };
    let riot_id = match (summoner["game_name"].as_str(), summoner["tagline"].as_str()) {
        (Some(name), Some(tag)) if !name.is_empty() => format!("{name}#{tag}"),
        _ => asked.to_string(),
    };
    let name = riot_id.rsplit_once('#').map_or(riot_id.as_str(), |(name, _)| name).to_lowercase();
    let mut records = Vec::new();
    let mut games = Vec::new();
    for game in matches["data"]["game_history"].as_array().into_iter().flatten() {
        let Some(id) = game["id"].as_str() else {
            continue;
        };
        let players: Vec<&Value> = game["participants"].as_array().into_iter().flatten().collect();
        // OP.GG ne met que lui dans chaque partie ; sinon, celui qui porte son nom.
        let me = players
            .iter()
            .find(|p| p["summoner"]["game_name"].as_str().is_some_and(|n| n.to_lowercase() == name))
            .or_else(|| players.first().filter(|_| players.len() == 1));
        let Some(me) = me else {
            continue;
        };
        let stats = &me["stats"];
        let n = |key: &str| stats[key].as_u64().unwrap_or(0);
        let number = game_number(id);
        let created_at = game["created_at"].as_str().unwrap_or_default();
        records.push(history::GameRecord {
            game_id: number,
            date: iso_millis(created_at).unwrap_or(0),
            queue_id: queue_of(game["game_type"].as_str().unwrap_or_default()),
            map_id: match game["game_map"].as_str() {
                Some("SUMMONERS_RIFT") => 11,
                Some("HOWLING_ABYSS") => 12,
                _ => 0,
            },
            champion_id: me["champion_id"].as_i64().unwrap_or(0),
            champion: me["champion_name"].as_str().unwrap_or_default().to_string(),
            position: position_of(me["position"].as_str().unwrap_or_default()).to_string(),
            win: stats["result"] == "WIN",
            duration: game["game_length_second"].as_u64().unwrap_or(0),
            kills: n("kill"),
            deaths: n("death"),
            assists: n("assist"),
            cs: n("minion_kill") + n("neutral_minion_kill"),
            gold: n("gold_earned"),
            damage: n("total_damage_dealt_to_champions"),
            ..Default::default()
        });
        let game = OpggGame {
            region: region.to_string(),
            id: id.to_string(),
            created_at: created_at.to_string(),
            riot_id: riot_id.clone(),
        };
        games.push((number, game));
    }
    let found = Searched {
        riot_id,
        level: summoner["level"].as_u64(),
        region: region.to_string(),
        source: "opgg".to_string(),
        profile: lol::summarize(rank("SOLORANKED"), rank("FLEXRANKED"), records),
    };
    (found, games)
}

/// Rang d'une file dans le profil OP.GG (« DIAMOND », division 4, 11 LP) ; non classé sans rang.
fn rank_of(stats: &Value) -> lol::Rank {
    let info = &stats["tier_info"];
    let tier = info["tier"].as_str().unwrap_or_default().to_uppercase();
    if tier.is_empty() {
        return lol::Rank::default();
    }
    let division = match info["division"].as_i64() {
        _ if matches!(tier.as_str(), "MASTER" | "GRANDMASTER" | "CHALLENGER") => "",
        Some(1) => "I",
        Some(2) => "II",
        Some(3) => "III",
        Some(4) => "IV",
        _ => "",
    };
    lol::Rank {
        tier,
        division: division.to_string(),
        lp: info["lp"].as_i64().unwrap_or(0),
        wins: stats["win"].as_u64().unwrap_or(0),
        losses: stats["lose"].as_u64().unwrap_or(0),
    }
}

/// File de la partie, avec les numéros du client (voir QUEUES dans main.js) ; -1 si inconnue.
fn queue_of(game_type: &str) -> i64 {
    match game_type {
        "SOLORANKED" => 420,
        "FLEXRANKED" => 440,
        "NORMAL" => 400,
        "QUICKPLAY" | "SWIFTPLAY" => 480,
        "ARAM" => 450,
        "CLASH" => 700,
        "URF" | "ARURF" => 900,
        "ARENA" => 1700,
        _ => -1,
    }
}

/// Poste d'OP.GG avec les noms de l'app ; vide si inconnu.
fn position_of(position: &str) -> &'static str {
    match position {
        "TOP" => "top",
        "JUNGLE" => "jungle",
        "MID" => "mid",
        "ADC" => "adc",
        "SUPPORT" => "support",
        _ => "",
    }
}

/// Postes dans l'ordre du client, pour ranger chaque équipe.
const ORDER: [&str; 5] = ["TOP", "JUNGLE", "MID", "ADC", "SUPPORT"];

/// Les deux équipes d'une partie OP.GG, celle du joueur cherché d'abord ; note OP.GG (0 à 10) × 10 en guise de
/// score carry, et sa place d'après OP.GG.
fn details_from(doc: &Value, game_id: i64, riot_id: &str) -> Option<history::MatchDetails> {
    let teams = doc["data"]["game_detail"]["teams"].as_array()?;
    let mut sides: Vec<history::MatchTeam> = teams
        .iter()
        .map(|team| {
            let mut players: Vec<&Value> = team["participants"].as_array().into_iter().flatten().collect();
            players.sort_by_key(|p| ORDER.iter().position(|o| p["position"] == *o).unwrap_or(ORDER.len()));
            history::MatchTeam { win: team["game_stat"]["is_win"] == true, players: players.into_iter().map(player_from).collect() }
        })
        .collect();
    if sides.len() != 2 {
        return None;
    }
    // Joueur cherché pas marqué par OP.GG : celui qui porte son nom.
    if !sides.iter().flat_map(|s| &s.players).any(|p| p.me) {
        let name = riot_id.rsplit_once('#').map_or(riot_id, |(name, _)| name).to_lowercase();
        if let Some(player) = sides.iter_mut().flat_map(|s| s.players.iter_mut()).find(|p| p.name.to_lowercase() == name) {
            player.me = true;
        }
    }
    if sides[1].players.iter().any(|p| p.me) {
        sides.swap(0, 1);
    }
    let [first, second]: [history::MatchTeam; 2] = sides.try_into().ok()?;
    Some(history::MatchDetails { game_id, teams: [first, second] })
}

fn player_from(p: &Value) -> history::MatchPlayer {
    let stats = &p["stats"];
    let n = |key: &str| stats[key].as_u64().unwrap_or(0);
    let ids = |list: &Value| -> Vec<i64> { list.as_array().into_iter().flatten().filter_map(Value::as_i64).collect() };
    let text = |value: &Value| value.as_str().unwrap_or_default().to_string();
    // 7 cases comme dans le client (OP.GG ne donne que les objets présents).
    let mut items = ids(&p["items"]);
    items.resize(items.len().max(7), 0);
    history::MatchPlayer {
        name: text(&p["summoner"]["game_name"]),
        tag: text(&p["summoner"]["tagline"]),
        me: p["is_target"] == true,
        champion_id: p["champion_id"].as_i64().unwrap_or(0),
        level: n("champion_level"),
        spells: ids(&p["spells"]),
        keystone: p["rune"]["primary_rune_id"].as_i64().unwrap_or(0),
        sub_style: p["rune"]["secondary_page_id"].as_i64().unwrap_or(0),
        kills: n("kill"),
        deaths: n("death"),
        assists: n("assist"),
        damage: n("total_damage_dealt_to_champions"),
        gold: n("gold_earned"),
        cs: n("minion_kill") + n("neutral_minion_kill"),
        wards: n("ward_place"),
        items,
        score: (stats["op_score"].as_f64().unwrap_or(0.0) * 10.0).round(),
        place: u32::try_from(stats["op_score_rank"].as_u64().unwrap_or(10)).unwrap_or(10),
        ..Default::default()
    }
}

/// Numéro donné par l'app à une partie OP.GG (dont l'identifiant est un texte) : sous 2^53, pour arriver intact
/// dans l'interface (JavaScript) et en revenir.
fn game_number(id: &str) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    i64::try_from(hash >> 11).unwrap_or(0)
}

/// « 2026-11-17T03:19:24+09:00 » (ISO-8601, comme OP.GG l'écrit) en millisecondes depuis 1970.
fn iso_millis(text: &str) -> Option<i64> {
    let num = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let seconds = num(11..13)? * 3600 + num(14..16)? * 60 + num(17..19)?;
    // Décalage horaire à la fin : « Z », « +09:00 » ou « -05:00 » (après d'éventuelles fractions de seconde).
    let offset = match text.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit()) {
        "" | "Z" => 0,
        zone => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            sign * (zone.get(1..3)?.parse::<i64>().ok()? * 3600 + zone.get(4..6)?.parse::<i64>().ok()? * 60)
        }
    };
    // Jours depuis le 1er janvier 1970 (calcul « days from civil »).
    let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + (153 * m + 2) / 5 + day - 1;
    let days = era * 146_097 + day_of_era - 719_468;
    Some((days * 86_400 + seconds - offset) * 1000)
}

/// Pour une adresse : tout sauf les lettres, les chiffres et - _ . ~ en %XX (octets UTF-8).
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { char::from(b).to_string() } else { format!("%{b:02X}") })
        .collect()
}
