//! Salon d'un autre joueur, cherché par son Riot ID et sa région. Dans la région de ton client, l'app le lit dans
//! le client (comme ton salon, avec le détail des parties et le score carry) ; dans les autres régions, ou si le
//! client ne le trouve pas, sur OP.GG (rangs, 20 dernières parties, détail des parties avec la note OP.GG).
//!
//! Page d'un champion, cherché par son nom : ses stats à chaque poste, son build, ses meilleurs et pires duels, ses
//! meilleurs partenaires et ses joueurs, d'après OP.GG (comme sur DPM).

use crate::{history, lol, opgg};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

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

/// Un champion pour les suggestions de la barre de recherche.
#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChampionEntry {
    pub id: i64,
    /// Nom affiché (« Maître Yi ») et nom interne (« MasterYi ») : la recherche marche avec les deux.
    pub name: String,
    pub alias: String,
    /// Rôles d'après le client (« fighter »…), pour deviner son poste ; vides sans le client.
    #[serde(skip)]
    pub roles: Vec<String>,
}

/// Page d'un champion à un poste (comme sur DPM), d'après OP.GG.
#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChampionPage {
    pub id: i64,
    pub name: String,
    /// Poste affiché ("top", "jungle", "mid", "adc", "support"), et les postes où il est joué, le plus joué d'abord.
    pub position: String,
    pub positions: Vec<PagePosition>,
    pub runes: Option<lol::RunePage>,
    pub spells: Vec<i64>,
    /// Leurs noms d'après le client (vides sans lui).
    pub spell_names: Vec<String>,
    pub spell_games: u64,
    pub spell_wins: u64,
    pub skill_max: Vec<String>,
    pub skill_order: Vec<String>,
    pub starter: Vec<opgg::Item>,
    pub boots: Vec<opgg::Item>,
    pub core: Option<opgg::CoreSet>,
    /// Les plus achetés en 4e, 5e et 6e objet, avec leurs parties et victoires.
    pub fourth: Vec<opgg::Item>,
    pub fifth: Vec<opgg::Item>,
    pub sixth: Vec<opgg::Item>,
    /// Ses meilleurs et ses pires duels à ce poste (son % de victoires contre chacun), 5 au plus.
    pub good: Vec<PageDuel>,
    pub bad: Vec<PageDuel>,
    /// Ses meilleurs partenaires, 2 par poste du partenaire.
    pub synergies: Vec<PageDuel>,
    /// Les joueurs qui ont le plus de parties avec lui dans la région de ton client (5 au plus).
    pub players: Vec<opgg::TopPlayer>,
    pub region: String,
}

/// Stats d'un champion à un poste.
#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PagePosition {
    pub position: String,
    pub role_rate: f64,
    pub games: u64,
    pub win_rate: f64,
    pub pick_rate: f64,
    pub ban_rate: f64,
    pub kda: f64,
    /// Tier OP.GG (1 = le meilleur, 5) et rang dans le classement du poste (0 si inconnus).
    pub tier: u8,
    pub rank: u32,
}

/// Un autre champion face à lui (ou avec lui) : parties, et son % de victoires à lui. `position` : poste du
/// partenaire (partenaires seulement).
#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PageDuel {
    pub champion_id: i64,
    pub name: String,
    pub games: u64,
    pub win_rate: f64,
    pub position: String,
}

/// Postes d'OP.GG, dans l'ordre de l'app.
const PAGE_POSITIONS: [&str; 5] = ["top", "jungle", "mid", "adc", "support"];

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
    /// Tous les champions (client, sinon OP.GG), lus une fois.
    champions: Mutex<Vec<ChampionEntry>>,
    /// Pages de champion déjà ouvertes : (champion, poste demandé, vide = son poste principal).
    pages: Mutex<HashMap<(i64, String), ChampionPage>>,
    /// Classement d'OP.GG de chaque poste (pages des postes), et quand il a été lu.
    meta: Mutex<Option<(Instant, LaneMeta)>>,
}

/// Classement d'OP.GG par poste ("top"…), du premier au dernier.
type LaneMeta = HashMap<String, Vec<opgg::MetaChamp>>;

/// Page d'un poste : son classement d'après OP.GG, du premier au dernier (champions « RIP » à ce poste exclus).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleMeta {
    pub position: String,
    /// Minutes depuis la lecture du classement chez OP.GG.
    pub age_minutes: u64,
    pub champions: Vec<opgg::MetaChamp>,
}

impl Search {
    pub fn new() -> Self {
        Self {
            opgg: tokio::sync::Mutex::new(opgg::Client::new()),
            games: Mutex::default(),
            details: Mutex::default(),
            champions: Mutex::default(),
            pages: Mutex::default(),
            meta: Mutex::default(),
        }
    }

    /// Classement d'OP.GG d'un poste ("top", "jungle", "mid", "adc", "support"), relu au plus toutes les 3 heures
    /// (une seule petite réponse pour les 5 postes), avec le numéro et le nom de chaque champion dans le client.
    pub async fn role_meta(&self, assets: &lol::Assets, position: &str) -> Result<RoleMeta, String> {
        const KEEP: Duration = Duration::from_secs(3 * 60 * 60);
        let fresh = self.meta.lock().unwrap().as_ref().filter(|(at, _)| at.elapsed() < KEEP).map(|(at, meta)| (*at, meta.get(position).cloned()));
        let (at, list) = match fresh {
            Some(found) => found,
            None => {
                let known: Vec<(i64, String, String)> =
                    self.champions(assets).await?.into_iter().map(|c| (c.id, c.alias, c.name)).collect();
                let mut meta = self.opgg.lock().await.lane_meta().await?;
                for champ in meta.values_mut().flat_map(|list| list.iter_mut()) {
                    if let Some((id, name)) = lol::champion_by_name(&champ.name, &known) {
                        (champ.champion_id, champ.name) = (id, name.to_string());
                    }
                }
                let at = Instant::now();
                let list = meta.get(position).cloned();
                *self.meta.lock().unwrap() = Some((at, meta));
                (at, list)
            }
        };
        let champions = list.ok_or("Poste inconnu")?.into_iter().filter(|c| c.champion_id > 0).collect();
        Ok(RoleMeta { position: position.to_string(), age_minutes: at.elapsed().as_secs() / 60, champions })
    }

    /// Tous les champions, pour les suggestions de la barre : ceux du client, sinon ceux d'OP.GG.
    pub async fn champions(&self, assets: &lol::Assets) -> Result<Vec<ChampionEntry>, String> {
        if !self.champions.lock().unwrap().is_empty() {
            return Ok(self.champions.lock().unwrap().clone());
        }
        let mut list: Vec<ChampionEntry> = assets
            .champion_entries()
            .await
            .into_iter()
            .map(|(id, name, alias, roles)| ChampionEntry { id, name, alias, roles })
            .collect();
        if list.is_empty() {
            let from_opgg = self.opgg.lock().await.champion_list().await?;
            list = from_opgg.into_iter().map(|(id, name, alias)| ChampionEntry { id, name, alias, roles: Vec::new() }).collect();
        }
        list.sort_by(|a, b| a.name.cmp(&b.name));
        eprintln!("[champions] {} champions pour la recherche", list.len());
        *self.champions.lock().unwrap() = list.clone();
        Ok(list)
    }

    /// Page d'un champion : à `position` (« top »…), sinon à son poste le plus joué. Gardée en mémoire.
    pub async fn champion_page(&self, assets: &lol::Assets, id: i64, position: Option<String>) -> Result<ChampionPage, String> {
        let wanted = position.clone().unwrap_or_default();
        if let Some(page) = self.pages.lock().unwrap().get(&(id, wanted.clone())) {
            return Ok(page.clone());
        }
        let champions = self.champions(assets).await?;
        let entry = champions.iter().find(|c| c.id == id).cloned().ok_or("Champion inconnu")?;
        let champ = opgg::Champ { id, name: entry.name.clone(), alias: entry.alias.clone(), ..Default::default() };
        let mut opgg = self.opgg.lock().await;
        // Sans poste demandé : deviné d'après ses rôles, puis son poste le plus joué si ce n'était pas lui. Son poste
        // le plus joué aussi si OP.GG ne le voit pas joué au poste demandé (partenaire à un poste rare).
        let first = position.clone().unwrap_or_else(|| guess_position(&entry.roles).to_string());
        let mut doc = opgg.champion_page_doc(&champ, &first).await?;
        let mut shown = first.clone();
        let positions = page_positions(&doc);
        if position.is_none() || !positions.iter().any(|p| p.position == first) {
            let main = positions.into_iter().next().map(|p| p.position);
            if let Some(main) = main.filter(|main| *main != first) {
                doc = opgg.champion_page_doc(&champ, &main).await?;
                shown = main;
            }
        }
        let mut page = page_from(&doc, &entry, &shown);
        // Ses 5 meilleurs et 5 pires duels : la liste complète vient avec un guide de couloir (contre un de ses
        // adversaires courants) ; sinon les 3 de chaque que donne l'analyse.
        let rival = page.bad.first().or(page.good.first()).map(|d| d.champion_id);
        let rival = rival.and_then(|rid| champions.iter().find(|c| c.id == rid));
        if let Some(rival) = rival {
            let against = opgg::Champ { id: rival.id, name: rival.name.clone(), alias: rival.alias.clone(), ..Default::default() };
            match opgg.counters(&champ, &against, &shown).await {
                Ok(mut list) if list.len() >= 6 => {
                    // Sans lui-même (le duel miroir).
                    list.retain(|p| p.champion_id != id);
                    let duel = |p: &opgg::Pick| PageDuel {
                        champion_id: p.champion_id,
                        name: p.name.clone(),
                        games: p.games,
                        win_rate: 1.0 - p.win_rate,
                        position: String::new(),
                    };
                    page.bad = list.iter().take(5).map(duel).collect();
                    page.good = list.iter().rev().take(5).map(duel).collect();
                }
                Ok(_) => {}
                Err(e) => eprintln!("[champion] duels de {} : {e}", entry.name),
            }
        }
        // Ses joueurs, dans la région de ton client.
        let region = assets.client_region().await.unwrap_or_else(|| "EUW".to_string());
        match opgg.leaderboard(&champ, &region).await {
            Ok(mut players) => {
                players.truncate(5);
                page.players = players;
            }
            Err(e) => eprintln!("[champion] joueurs de {} : {e}", entry.name),
        }
        page.region = region;
        drop(opgg);
        let spells = assets.client_json("/lol-game-data/assets/v1/summoner-spells.json", false).await;
        let name = |id: &i64| -> Option<String> {
            let list = spells.as_ref()?.as_array()?;
            let spell = list.iter().find(|s| s["id"].as_i64() == Some(*id))?;
            spell["name"].as_str().map(str::to_string)
        };
        page.spell_names = page.spells.iter().map(|id| name(id).unwrap_or_default()).collect();
        eprintln!("[champion] {} ({}) : page prête", entry.name, page.position);
        // Gardée pour le poste demandé et pour celui affiché (son poste principal, demandé ensuite d'un clic).
        let mut pages = self.pages.lock().unwrap();
        pages.insert((id, page.position.clone()), page.clone());
        pages.insert((id, wanted), page.clone());
        Ok(page)
    }

    /// `region` vide : celle de ton client. `puuid` : son compte quand il est connu (un joueur d'une de tes parties),
    /// lu directement dans le client.
    pub async fn find(&self, assets: &lol::Assets, riot_id: &str, region: &str, puuid: Option<&str>) -> Result<Searched, String> {
        let client_region = assets.client_region().await;
        let region = match region.trim() {
            "" => client_region.clone().ok_or("Région inconnue : choisis-la à droite de la barre de recherche.")?,
            region => region.to_uppercase(),
        };
        let in_client = client_region.as_deref() == Some(region.as_str());
        let riot_id = riot_id.trim();
        let by_account = match puuid.filter(|p| in_client && !p.is_empty()) {
            Some(puuid) => from_client(assets, Who::Account(puuid), riot_id, &region).await,
            None => None,
        };
        if let Some(found) = by_account {
            eprintln!("[recherche] {} ({region}) : lu dans le client, {} parties", found.riot_id, found.profile.matches.len());
            return Ok(self.with_records(found).await);
        }
        let (name, tag) = riot_id
            .rsplit_once('#')
            .map(|(name, tag)| (name.trim(), tag.trim()))
            .filter(|(name, tag)| !name.is_empty() && !tag.is_empty())
            .ok_or("Écris le Riot ID en entier : Nom#TAG")?;
        if in_client {
            if let Some(found) = from_client(assets, Who::Name(name, tag), riot_id, &region).await {
                eprintln!("[recherche] {name}#{tag} ({region}) : lu dans le client, {} parties", found.profile.matches.len());
                return Ok(self.with_records(found).await);
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

    /// Le client met à 0 les défaites des autres joueurs : leurs victoires et défaites prises alors sur OP.GG.
    async fn with_records(&self, mut found: Searched) -> Searched {
        if !found.profile.solo.iter().chain(&found.profile.flex).any(|r| r.losses_hidden) {
            return found;
        }
        let riot_id = found.riot_id.clone();
        let Some((name, tag)) = riot_id.rsplit_once('#') else {
            return found;
        };
        let doc = self.opgg.lock().await.summoner_profile(name, tag, &found.region).await;
        match doc {
            Ok(doc) => fill_records(&mut found.profile, &doc),
            Err(e) => eprintln!("[recherche] {riot_id} : défaites cachées par le client ; {e}"),
        }
        found
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

/// Qui chercher dans le client : son compte s'il est connu, sinon son Riot ID.
#[derive(Clone, Copy)]
enum Who<'a> {
    Account(&'a str),
    Name(&'a str, &'a str),
}

/// Le joueur dans le client (sa région seulement) ; None s'il est introuvable, si le client est fermé ou n'a rien
/// donné. `asked` : le Riot ID demandé, gardé si le client ne le redonne pas.
async fn from_client(assets: &lol::Assets, who: Who<'_>, asked: &str, region: &str) -> Option<Searched> {
    // Le client va chercher ce joueur, son rang et ses parties chez Riot.
    let (summoner, puuid) = match who {
        Who::Account(puuid) => {
            let summoner = assets.client_json(&format!("/lol-summoner/v2/summoners/puuid/{puuid}"), true).await;
            (summoner.unwrap_or(Value::Null), puuid.to_string())
        }
        Who::Name(name, tag) => {
            let path = format!("/lol-summoner/v1/summoners?name={}", encode(&format!("{name}#{tag}")));
            let summoner = assets.client_json(&path, true).await?;
            let puuid = summoner["puuid"].as_str().filter(|p| !p.is_empty())?.to_string();
            (summoner, puuid)
        }
    };
    let ranked = assets.client_json(&format!("/lol-ranked/v1/ranked-stats/{puuid}"), true).await;
    let path = format!("/lol-match-history/v1/products/lol/{puuid}/matches?begIndex=0&endIndex=19");
    let list = assets.client_json(&path, true).await;
    if ranked.is_none() && list.is_none() {
        return None;
    }
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
        _ if !asked.is_empty() => asked.to_string(),
        _ => "?".to_string(),
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

/// Victoires et défaites d'OP.GG pour les files dont le client cache les défaites (il les met à 0 pour les autres
/// joueurs) : ses chiffres à lui, parfois en retard de quelques parties ; rang et LP restent ceux du client.
pub fn fill_records(profile: &mut lol::Profile, doc: &Value) {
    let stats: Vec<&Value> = doc["data"]["summoner"]["league_stats"].as_array().into_iter().flatten().collect();
    for (rank, game_type) in [(&mut profile.solo, "SOLORANKED"), (&mut profile.flex, "FLEXRANKED")] {
        let Some(rank) = rank.as_mut().filter(|rank| rank.losses_hidden) else {
            continue;
        };
        let found = stats.iter().find(|s| s["game_type"] == game_type);
        let record = found.and_then(|s| Some((s["win"].as_u64()?, s["lose"].as_u64()?)));
        if let Some((wins, losses)) = record.filter(|(wins, losses)| wins + losses > 0) {
            (rank.wins, rank.losses) = (wins, losses);
            rank.losses_hidden = false;
            rank.record_from_opgg = true;
        }
    }
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
        ..Default::default()
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


/// Poste probable d'après ses rôles dans le client (avant de connaître celui qu'il joue le plus).
fn guess_position(roles: &[String]) -> &'static str {
    let has = |role: &str| roles.iter().any(|r| r == role);
    if has("marksman") {
        "adc"
    } else if has("support") {
        "support"
    } else if has("mage") || has("assassin") {
        "mid"
    } else {
        "top"
    }
}

/// Ses postes, du plus joué au moins joué (OP.GG : "TOP", "JUNGLE", "MID", "ADC", "SUPPORT").
fn page_positions(doc: &Value) -> Vec<PagePosition> {
    let mut list: Vec<PagePosition> = doc["data"]["summary"]["positions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let position = p["name"].as_str()?.to_lowercase();
            let stats = &p["stats"];
            let number = |key: &str| stats[key].as_f64().unwrap_or(0.0);
            PAGE_POSITIONS.contains(&position.as_str()).then(|| PagePosition {
                role_rate: number("role_rate"),
                games: stats["play"].as_u64().unwrap_or(0),
                win_rate: number("win_rate"),
                pick_rate: number("pick_rate"),
                ban_rate: number("ban_rate"),
                kda: number("kda"),
                tier: stats["tier_data"]["tier"].as_u64().unwrap_or(0) as u8,
                rank: stats["tier_data"]["rank"].as_u64().unwrap_or(0) as u32,
                position,
            })
        })
        .collect();
    list.sort_by(|a, b| b.role_rate.total_cmp(&a.role_rate));
    list
}

/// La page d'après l'analyse d'OP.GG à ce poste (ses duels : les 3 de chaque que donne l'analyse).
fn page_from(doc: &Value, entry: &ChampionEntry, position: &str) -> ChampionPage {
    let data = &doc["data"];
    let analysis = opgg::analysis_from(doc);
    let slot = |key: &str| -> Vec<opgg::Item> { data[key].as_array().into_iter().flatten().flat_map(opgg::items_of).collect() };
    let rate = |c: &Value| c["win"].as_f64().unwrap_or(0.0) / c["play"].as_f64().unwrap_or(0.0).max(1.0);
    let duels = |list: &Value, id: &str, name: &str, position: &str| -> Vec<PageDuel> {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                Some(PageDuel {
                    champion_id: c[id].as_i64()?,
                    name: c[name].as_str().unwrap_or_default().to_string(),
                    games: c["play"].as_u64().unwrap_or(0),
                    win_rate: rate(c),
                    position: position.to_string(),
                })
            })
            .collect()
    };
    let synergies = PAGE_POSITIONS
        .iter()
        .flat_map(|p| duels(&data["synergies"][*p], "synergy_champion_id", "synergy_champion_name", p).into_iter().take(2))
        .collect();
    let core = &data["core_items"];
    let spells = &data["summoner_spells"];
    ChampionPage {
        id: entry.id,
        name: entry.name.clone(),
        position: position.to_string(),
        positions: page_positions(doc),
        runes: analysis.as_ref().map(|a| lol::RunePage::of(&a.runes)),
        spells: spells["ids"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect(),
        spell_names: Vec::new(),
        spell_games: spells["play"].as_u64().unwrap_or(0),
        spell_wins: spells["win"].as_u64().unwrap_or(0),
        skill_max: analysis.as_ref().map(|a| a.build.skill_max.clone()).unwrap_or_default(),
        skill_order: analysis.as_ref().map(|a| a.build.skill_order.clone()).unwrap_or_default(),
        starter: opgg::items_of(&data["starter_items"]),
        boots: opgg::items_of(&data["boots"]),
        core: Some(opgg::core_set(core)).filter(|set| !set.items.is_empty()),
        fourth: slot("fourth_items"),
        fifth: slot("fifth_items"),
        sixth: slot("sixth_items"),
        good: duels(&data["strong_counters"], "champion_id", "champion_name", ""),
        bad: duels(&data["weak_counters"], "champion_id", "champion_name", ""),
        synergies,
        players: Vec::new(),
        region: String::new(),
    }
}
