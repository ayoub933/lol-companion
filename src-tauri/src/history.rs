//! Historique local de tes parties : une ligne JSON par partie (historique.jsonl), chacune rangée sous
//! son compte (plusieurs comptes peuvent jouer sur le même PC), et la dernière partie telle que le client
//! l'a donnée (derniere-partie.json).

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
    /// Compte qui a joué la partie (identifiant Riot « puuid ») ; vide pour les parties enregistrées avant
    /// que l'historique ne soit rangé par compte, le temps de retrouver leur compte.
    pub puuid: String,
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

    pub fn contains(&self, game_id: i64, puuid: &str) -> bool {
        self.get(game_id, puuid).is_some()
    }

    /// Cette partie, jouée par ce compte.
    pub fn get(&self, game_id: i64, puuid: &str) -> Option<&GameRecord> {
        self.games.iter().find(|g| g.game_id == game_id && g.puuid == puuid)
    }

    /// Parties de ce compte. Compte pas encore connu (client fermé) : celui de la partie la plus récente.
    fn of(&self, puuid: Option<&str>) -> impl Iterator<Item = &GameRecord> {
        let owner = match puuid {
            Some(puuid) => puuid.to_string(),
            None => self.games.iter().max_by_key(|g| g.date).map(|g| g.puuid.clone()).unwrap_or_default(),
        };
        self.games.iter().filter(move |g| g.puuid == owner)
    }

    /// Parties enregistrées sans leur compte : (partie, champion joué).
    pub fn unowned(&self) -> Vec<(i64, i64)> {
        self.games.iter().filter(|g| g.puuid.is_empty()).map(|g| (g.game_id, g.champion_id)).collect()
    }

    /// Range sous leur compte des parties qui n'en avaient pas (partie → compte), et réécrit le fichier.
    pub fn set_owners(&mut self, owners: &HashMap<i64, String>) -> Result<(), String> {
        for g in self.games.iter_mut().filter(|g| g.puuid.is_empty()) {
            if let Some(puuid) = owners.get(&g.game_id) {
                g.puuid.clone_from(puuid);
            }
        }
        let lines: Vec<String> = self.games.iter().filter_map(|g| serde_json::to_string(g).ok()).collect();
        let text = lines.iter().map(|line| format!("{line}\n")).collect::<String>();
        // Écrit à côté puis remplace : l'ancien fichier reste entier si l'écriture échoue.
        let (file, temp) = (self.dir.join(FILE), self.dir.join(format!("{FILE}.tmp")));
        fs::write(&temp, text)
            .and_then(|()| fs::rename(&temp, &file))
            .map_err(|e| format!("historique non réécrit : {e}"))
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

    /// (parties, victoires) de ce champion contre cet adversaire, sur ce compte.
    pub fn versus(&self, puuid: Option<&str>, champion_id: i64, opponent_id: i64) -> (u64, u64) {
        self.of(puuid)
            .filter(|g| g.champion_id == champion_id && g.opponent_id == opponent_id)
            .fold((0, 0), |(games, wins), g| (games + 1, wins + u64::from(g.win)))
    }

    /// Dernière partie et stats de ce compte (`None` : compte pas encore connu, voir `of`).
    pub fn overview(&self, puuid: Option<&str>) -> Option<Overview> {
        let last = self.of(puuid).max_by_key(|g| g.date)?.clone();
        let mut champions: HashMap<(i64, bool), Totals> = HashMap::new();
        let mut duels: HashMap<(i64, i64), Totals> = HashMap::new();
        // Faille de l'invocateur et ARAM seulement : les autres modes (Arena...) ne se comparent pas.
        for g in self.of(puuid).filter(|g| matches!(g.map_id, 11 | 12)) {
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

/// Une partie en détail, pour le salon : les deux équipes, la tienne d'abord.
#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MatchDetails {
    pub game_id: i64,
    pub teams: [MatchTeam; 2],
}

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MatchTeam {
    pub win: bool,
    /// Dans l'ordre du client (en classée : haut, jungle, milieu, bas, support).
    pub players: Vec<MatchPlayer>,
}

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MatchPlayer {
    /// Pour lire son rang ; pas envoyé à l'interface.
    #[serde(skip)]
    pub puuid: String,
    /// Riot ID : nom, et tag sans le « # ».
    pub name: String,
    pub tag: String,
    pub me: bool,
    pub champion_id: i64,
    pub level: u64,
    /// Sorts d'invocateur : D puis F.
    pub spells: Vec<i64>,
    /// Rune principale et voie secondaire.
    pub keystone: i64,
    pub sub_style: i64,
    pub kills: u64,
    pub deaths: u64,
    pub assists: u64,
    /// Dégâts infligés aux champions.
    pub damage: u64,
    pub gold: u64,
    pub cs: u64,
    /// Balises posées.
    pub wards: u64,
    /// Cases d'objets dans l'ordre (balise comprise) ; 0 = case vide.
    pub items: Vec<i64>,
    /// Rang solo/duo actuel ; None s'il n'a pas pu être lu.
    pub rank: Option<crate::lol::Rank>,
}

/// Les deux équipes d'une partie complète du client ; « toi » = le joueur de ton champion.
pub fn match_from(doc: &Value, champion_id: i64) -> Option<MatchDetails> {
    let participants = doc["participants"].as_array()?;
    let identities = doc["participantIdentities"].as_array();
    let my_team = &participants.iter().find(|p| p["championId"] == champion_id)?["teamId"];
    let player = |p: &Value| {
        let who = identities
            .into_iter()
            .flatten()
            .find(|identity| identity["participantId"] == p["participantId"])
            .map(|identity| &identity["player"]);
        let text = |key: &str| who.and_then(|w| w[key].as_str()).unwrap_or_default().to_string();
        let s = &p["stats"];
        let n = |key: &str| s[key].as_u64().unwrap_or(0);
        let id = |value: &Value| value.as_i64().unwrap_or(0);
        let name = text("gameName");
        MatchPlayer {
            puuid: text("puuid"),
            name: if name.is_empty() { text("summonerName") } else { name },
            tag: text("tagLine"),
            me: p["championId"] == champion_id,
            champion_id: id(&p["championId"]),
            level: n("champLevel"),
            spells: vec![id(&p["spell1Id"]), id(&p["spell2Id"])],
            keystone: id(&s["perk0"]),
            sub_style: id(&s["perkSubStyle"]),
            kills: n("kills"),
            deaths: n("deaths"),
            assists: n("assists"),
            damage: n("totalDamageDealtToChampions"),
            gold: n("goldEarned"),
            cs: n("totalMinionsKilled") + n("neutralMinionsKilled"),
            wards: n("wardsPlaced"),
            // item0 à item6 (et item7 si le client en donne un de plus).
            items: (0..8).map_while(|i| s.get(format!("item{i}")).map(id)).collect(),
            rank: None,
        }
    };
    let team = |ours: bool| {
        let players: Vec<&Value> = participants.iter().filter(|p| (&p["teamId"] == my_team) == ours).collect();
        MatchTeam {
            win: players.first().is_some_and(|p| p["stats"]["win"] == true),
            players: players.into_iter().map(player).collect(),
        }
    };
    Some(MatchDetails { game_id: doc["gameId"].as_i64()?, teams: [team(true), team(false)] })
}

/// Compte qui jouait ce champion dans cette partie (document complet du client, les 10 joueurs).
pub fn owner_of(doc: &Value, champion_id: i64) -> Option<String> {
    let participant = doc["participants"].as_array()?.iter().find(|p| p["championId"] == champion_id)?;
    let identity = doc["participantIdentities"]
        .as_array()?
        .iter()
        .find(|identity| identity["participantId"] == participant["participantId"])?;
    identity["player"]["puuid"].as_str().filter(|puuid| !puuid.is_empty()).map(str::to_string)
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
        puuid: puuid.unwrap_or_default().to_string(),
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
