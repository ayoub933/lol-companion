//! Historique local de tes parties : une ligne JSON par partie (historique.jsonl),
//! et la dernière partie telle que le client l'a donnée (derniere-partie.json).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, fs, io::Write, path::PathBuf};

const FILE: &str = "historique.jsonl";
const RAW_FILE: &str = "derniere-partie.json";

/// Une partie terminée, avec tes stats exactes (celles de l'historique du client).
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct GameRecord {
    pub game_id: i64,
    /// Début de la partie, en millisecondes depuis 1970.
    pub date: i64,
    pub queue_id: i64,
    /// 11 = Faille de l'invocateur, 12 = Abîme hurlant.
    pub map_id: i64,
    pub champion_id: i64,
    pub champion: String,
    /// "top", "jungle", "mid", "adc" ou "support" ; vide si inconnu.
    pub position: String,
    /// Adversaire direct ; 0 si inconnu.
    pub opponent_id: i64,
    pub opponent: String,
    pub win: bool,
    /// Durée en secondes.
    pub duration: u64,
    pub kills: u64,
    pub deaths: u64,
    pub assists: u64,
    /// Sbires et monstres tués : le compte exact (en partie, l'API ne le donne que par paliers de 10).
    pub cs: u64,
    pub gold: u64,
    /// Dégâts infligés aux champions.
    pub damage: u64,
    pub vision: u64,
    /// Skin joué (numéro du client, ex. 267002) ; 0 si inconnu.
    pub skin_id: i64,
}

/// Totaux d'un champion (ou d'un duel) : l'interface en tire les moyennes.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub champion_id: i64,
    pub champion: String,
    /// Duels seulement.
    pub opponent_id: i64,
    pub opponent: String,
    /// true = parties en ARAM, comptées à part (CS et KDA n'y sont pas comparables).
    pub aram: bool,
    pub games: u64,
    pub wins: u64,
    pub kills: u64,
    pub deaths: u64,
    pub assists: u64,
    pub cs: u64,
    /// Temps de jeu total, en secondes.
    pub duration: u64,
}

/// Ce que le salon affiche : la dernière partie, tes champions et tes duels les plus joués.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub last: GameRecord,
    pub champions: Vec<Totals>,
    pub duels: Vec<Totals>,
}

pub struct History {
    dir: PathBuf,
    games: Vec<GameRecord>,
}

impl History {
    /// Lit l'historique ; une ligne illisible est ignorée, le reste est gardé.
    pub fn load(dir: PathBuf) -> Self {
        let games = fs::read_to_string(dir.join(FILE))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        Self { dir, games }
    }

    pub fn contains(&self, game_id: i64) -> bool {
        self.get(game_id).is_some()
    }

    pub fn get(&self, game_id: i64) -> Option<&GameRecord> {
        self.games.iter().find(|g| g.game_id == game_id)
    }

    /// Ajoute la partie au fichier, et garde à côté le document complet du client.
    pub fn add(&mut self, record: GameRecord, raw: &Value) -> Result<(), String> {
        let write = || -> std::io::Result<()> {
            fs::create_dir_all(&self.dir)?;
            let mut file = fs::OpenOptions::new().create(true).append(true).open(self.dir.join(FILE))?;
            writeln!(file, "{}", serde_json::to_string(&record)?)?;
            fs::write(self.dir.join(RAW_FILE), serde_json::to_string_pretty(raw)?)
        };
        write().map_err(|e| format!("historique non enregistré : {e}"))?;
        self.games.push(record);
        Ok(())
    }

    /// (parties, victoires) de ce champion contre cet adversaire.
    pub fn versus(&self, champion_id: i64, opponent_id: i64) -> (u64, u64) {
        self.games
            .iter()
            .filter(|g| g.champion_id == champion_id && g.opponent_id == opponent_id)
            .fold((0, 0), |(games, wins), g| (games + 1, wins + u64::from(g.win)))
    }

    pub fn overview(&self) -> Option<Overview> {
        let last = self.games.iter().max_by_key(|g| g.date)?.clone();
        let mut champions: HashMap<(i64, bool), Totals> = HashMap::new();
        let mut duels: HashMap<(i64, i64), Totals> = HashMap::new();
        // Faille de l'invocateur et ARAM seulement : les autres modes (Arena...) ne se comparent pas.
        for g in self.games.iter().filter(|g| matches!(g.map_id, 11 | 12)) {
            let aram = g.map_id == 12;
            let new = || Totals { champion_id: g.champion_id, champion: g.champion.clone(), aram, ..Default::default() };
            champions.entry((g.champion_id, aram)).or_insert_with(new).add(g);
            if !aram && g.opponent_id > 0 {
                let new_duel = || Totals { opponent_id: g.opponent_id, opponent: g.opponent.clone(), ..new() };
                duels.entry((g.champion_id, g.opponent_id)).or_insert_with(new_duel).add(g);
            }
        }
        Some(Overview {
            last,
            champions: most_played(champions.into_values(), 5),
            duels: most_played(duels.into_values(), 3),
        })
    }
}

impl Totals {
    fn add(&mut self, g: &GameRecord) {
        self.games += 1;
        self.wins += u64::from(g.win);
        self.kills += g.kills;
        self.deaths += g.deaths;
        self.assists += g.assists;
        self.cs += g.cs;
        self.duration += g.duration;
    }
}

/// Les plus joués d'abord (à égalité : le plus de victoires, puis l'ordre alphabétique).
fn most_played(list: impl Iterator<Item = Totals>, count: usize) -> Vec<Totals> {
    let mut list: Vec<Totals> = list.collect();
    list.sort_by(|a, b| {
        b.games
            .cmp(&a.games)
            .then(b.wins.cmp(&a.wins))
            .then(a.champion.cmp(&b.champion))
            .then(a.opponent.cmp(&b.opponent))
    });
    list.truncate(count);
    list
}

/// Ta ligne dans une partie de l'historique du client (/lol-match-history/v1/games/{id},
/// à l'ancien format « match-v4 »). `None` tant que la partie n'y est pas complète.
pub fn record_from(doc: &Value, puuid: Option<&str>, summoner_id: Option<u64>, champion_id: i64) -> Option<GameRecord> {
    let participants = doc["participants"].as_array()?;
    // Toi : d'après ton identité si le client la donne, sinon d'après ton champion.
    let my_id = doc["participantIdentities"].as_array().into_iter().flatten().find_map(|identity| {
        let player = &identity["player"];
        let same_puuid = puuid.is_some_and(|me| player["puuid"] == me);
        let same_summoner = summoner_id.is_some_and(|me| player["summonerId"] == me);
        (same_puuid || same_summoner).then(|| identity["participantId"].clone())
    });
    // (Dans la liste de tes dernières parties, le client ne met que toi.)
    let me = participants
        .iter()
        .find(|p| my_id.as_ref().is_some_and(|id| p["participantId"] == *id))
        .or_else(|| participants.iter().find(|p| champion_id > 0 && p["championId"] == champion_id))
        .or_else(|| participants.first().filter(|_| participants.len() == 1))?;
    let s = &me["stats"];
    let n = |key: &str| s[key].as_u64().unwrap_or(0);
    Some(GameRecord {
        game_id: doc["gameId"].as_i64()?,
        date: doc["gameCreation"].as_i64().unwrap_or(0),
        queue_id: doc["queueId"].as_i64().unwrap_or(0),
        map_id: doc["mapId"].as_i64().unwrap_or(0),
        champion_id: me["championId"].as_i64().unwrap_or(champion_id),
        win: s["win"].as_bool()?,
        duration: doc["gameDuration"].as_u64().unwrap_or(0),
        kills: s["kills"].as_u64()?,
        deaths: n("deaths"),
        assists: n("assists"),
        cs: n("totalMinionsKilled") + n("neutralMinionsKilled"),
        gold: n("goldEarned"),
        damage: n("totalDamageDealtToChampions"),
        vision: n("visionScore"),
        ..Default::default()
    })
}
