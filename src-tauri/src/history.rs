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
    /// En classée, LP gagnés (positif) ou perdus (négatif) : tes LP après la partie moins ceux d'avant.
    /// Absent si l'app ne les a pas vus (autre file, partie jouée sans l'app...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lp_change: Option<i64>,
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

    /// Poste où ce compte joue le plus ce champion sur la Faille, d'après les parties suivies par l'app
    /// (à égalité, le premier dans l'ordre alphabétique).
    pub fn main_position(&self, puuid: Option<&str>, champion_id: i64) -> Option<String> {
        let mut counts: HashMap<&str, u32> = HashMap::new();
        for g in self.of(puuid).filter(|g| g.champion_id == champion_id && g.map_id == 11 && !g.position.is_empty()) {
            *counts.entry(g.position.as_str()).or_default() += 1;
        }
        counts.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0))).map(|(position, _)| position.to_string())
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
    /// Pour lire son rang, et ouvrir son profil d'un clic ; vide sur OP.GG.
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
    /// Score « carry » de 0 à 100 (voir carry_scores) et place dans la partie selon ce score (1 = le meilleur).
    pub score: f64,
    pub place: u32,
}

/// Objets de support : l'Atlas du monde, ses étapes (Boussole runique, Butin des mondes) et ses 5 versions
/// finales (les 5 derniers : quête de support finie). Qui en a un joue support.
const SUPPORT_ITEMS: [i64; 8] = [3865, 3866, 3867, 3869, 3870, 3871, 3876, 3877];
/// Sort d'invocateur Châtiment : qui l'a joue jungle.
const SMITE: i64 = 11;
/// Objet lié au rôle (quêtes de rôle), donné par le client pour chaque joueur (« roleBoundItem ») ; le bas a
/// ses bottes à la place. Vu sur les deux équipes d'une partie classée de novembre 2026.
const ROLE_ITEMS: [(i64, Role); 4] = [(1220, Role::Top), (1209, Role::Jungle), (1206, Role::Mid), (1208, Role::Support)];

/// Rôle d'un joueur, pour son score carry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Top,
    Jungle,
    Mid,
    Bot,
    Support,
    /// Hors de la Faille (ARAM...) ou rôle introuvable.
    Other,
}

/// Barème du score carry pour un rôle, calé sur les notes de dpm.lol (voir carry_scores).
struct Scale {
    /// Points par kill, par mort (retirés) et par assistance.
    kill: f64,
    death: f64,
    assist: f64,
    /// CS par minute : (pente, référence à 0 point) ; aucun pour le support.
    cs: Option<(f64, f64)>,
    /// Or par minute : (pente, référence à 0 point).
    gold: (f64, f64),
    /// Dégâts aux champions et vision par minute : (pente, décalage).
    damage: (f64, f64),
    vision: (f64, f64),
    /// Par point de participation aux kills au-delà de 25 %, de part des dégâts et des dégâts encaissés
    /// de l'équipe.
    kill_share: f64,
    damage_share: f64,
    taken_share: f64,
    /// Dégâts aux objectifs par minute : (pente, décalage).
    objectives: (f64, f64),
    /// Par tour détruite (haut, milieu, bas).
    turret: f64,
    /// Points de rôle que le client ne donne pas de quoi calculer (plaques, solo kills, crabes, buffs volés,
    /// alliés sauvés...) : leur moyenne sur dpm.lol.
    missing: f64,
}

impl Role {
    fn scale(self) -> Scale {
        match self {
            Role::Top => Scale {
                kill: 0.8,
                death: 1.5,
                assist: 0.8,
                cs: Some((2.0, 5.3)),
                gold: (0.0483, 339.0),
                damage: (0.0067, 0.44),
                vision: (6.62, -2.97),
                kill_share: 0.14,
                damage_share: 0.09,
                taken_share: 0.07,
                objectives: (0.0028, 1.17),
                turret: 0.85,
                missing: 8.1,
            },
            Role::Jungle => Scale {
                kill: 0.75,
                death: 1.5,
                assist: 0.75,
                cs: Some((1.53, 5.9)),
                gold: (0.0449, 342.0),
                damage: (0.0069, 0.47),
                vision: (5.98, -3.68),
                kill_share: 0.125,
                damage_share: 0.05,
                taken_share: 0.05,
                objectives: (0.0012, 0.2),
                turret: 0.0,
                missing: 13.4,
            },
            Role::Mid => Scale {
                kill: 0.75,
                death: 1.5,
                assist: 0.75,
                cs: Some((1.71, 5.5)),
                gold: (0.0419, 337.0),
                damage: (0.0062, 0.8),
                vision: (7.51, -4.0),
                kill_share: 0.13,
                damage_share: 0.07,
                taken_share: 0.05,
                objectives: (0.006, 0.15),
                turret: 0.75,
                missing: 9.7,
            },
            Role::Bot => Scale {
                kill: 0.75,
                death: 1.5,
                assist: 0.75,
                cs: Some((1.87, 6.0)),
                gold: (0.0388, 343.0),
                damage: (0.0065, 0.32),
                vision: (6.26, -3.26),
                kill_share: 0.12,
                damage_share: 0.1,
                taken_share: 0.03,
                objectives: (0.0029, 0.18),
                turret: 0.75,
                missing: 7.2,
            },
            Role::Support => Scale {
                kill: 0.85,
                death: 1.25,
                assist: 0.9,
                cs: None,
                gold: (0.054, 263.0),
                damage: (0.0043, 0.18),
                vision: (3.5, -3.38),
                kill_share: 0.14,
                damage_share: 0.06,
                taken_share: 0.06,
                objectives: (0.0, 0.0),
                turret: 0.0,
                missing: 5.2,
            },
            // ARAM ou rôle introuvable : comme le milieu, sans les points de couloir ni de rôle.
            Role::Other => Scale { turret: 0.0, missing: 0.0, ..Role::Mid.scale() },
        }
    }
}

/// Rôle de chaque participant (même ordre), sur la Faille de l'invocateur ; ailleurs (ARAM...), aucun.
/// Rempli équipe par équipe, un rôle par joueur :
/// - l'objet lié au rôle donné par le client (quêtes de rôle), s'il donne des rôles différents ;
/// - puis, pour les rôles encore libres : Châtiment → jungle (s'ils sont plusieurs, celui qui a tué le plus
///   de monstres) ; objet de support → support (sinon celui qui a le moins de sbires, s'il en a moins de 3
///   par minute) ; haut, milieu, bas selon le couloir annoncé par le client, une fois chacun ; le dernier
///   rôle libre au dernier joueur.
///
/// Le couloir annoncé vient d'une ancienne estimation de Riot (la position en début de partie) et se trompe
/// parfois (ex. un top annoncé en jungle) : la jungle et le support ne s'y fient donc jamais.
fn roles(doc: &Value, participants: &[Value]) -> Vec<Role> {
    let mut roles = vec![Role::Other; participants.len()];
    if doc["mapId"] != 11 {
        return roles;
    }
    let minutes = (doc["gameDuration"].as_f64().unwrap_or(0.0) / 60.0).max(1.0);
    let stat = |i: usize, key: &str| participants[i]["stats"][key].as_f64().unwrap_or(0.0);
    let smite = |i: usize| [&participants[i]["spell1Id"], &participants[i]["spell2Id"]].iter().any(|s| s.as_i64() == Some(SMITE));
    let support_item = |i: usize| {
        (0..7).any(|k| participants[i]["stats"][format!("item{k}")].as_i64().is_some_and(|item| SUPPORT_ITEMS.contains(&item)))
    };
    for team in [100, 200] {
        let members: Vec<usize> = (0..participants.len()).filter(|i| participants[*i]["teamId"] == team).collect();
        let mut free = members.clone();
        let bound: Vec<(usize, Role)> = free
            .iter()
            .filter_map(|i| {
                let item = participants[*i]["stats"]["roleBoundItem"].as_i64()?;
                ROLE_ITEMS.iter().find(|(id, _)| *id == item).map(|(_, role)| (*i, *role))
            })
            .collect();
        let distinct = bound.iter().enumerate().all(|(k, (_, role))| bound[..k].iter().all(|(_, other)| other != role));
        if distinct {
            for (i, role) in &bound {
                roles[*i] = *role;
            }
            free.retain(|i| bound.iter().all(|(j, _)| j != i));
        }
        let taken = |roles: &[Role], role: Role| members.iter().any(|i| roles[*i] == role);
        if !taken(&roles, Role::Jungle) {
            let jungler = free
                .iter()
                .copied()
                .filter(|i| smite(*i))
                .max_by(|a, b| stat(*a, "neutralMinionsKilled").total_cmp(&stat(*b, "neutralMinionsKilled")));
            if let Some(i) = jungler {
                roles[i] = Role::Jungle;
                free.retain(|j| *j != i);
            }
        }
        if !taken(&roles, Role::Support) {
            let minions = |i: &usize| stat(*i, "totalMinionsKilled");
            let support = free
                .iter()
                .copied()
                .filter(|i| support_item(*i))
                .min_by(|a, b| minions(a).total_cmp(&minions(b)))
                .or_else(|| {
                    free.iter().copied().min_by(|a, b| minions(a).total_cmp(&minions(b))).filter(|i| minions(i) < 3.0 * minutes)
                });
            if let Some(i) = support {
                roles[i] = Role::Support;
                free.retain(|j| *j != i);
            }
        }
        let mut open: Vec<Role> = [Role::Top, Role::Mid, Role::Bot].into_iter().filter(|role| !taken(&roles, *role)).collect();
        let mut unknown = Vec::new();
        for i in free {
            let lane = match participants[i]["timeline"]["lane"].as_str().unwrap_or_default() {
                "TOP" => Some(Role::Top),
                "MIDDLE" | "MID" => Some(Role::Mid),
                "BOTTOM" | "BOT" => Some(Role::Bot),
                _ => None,
            };
            match lane.filter(|lane| open.contains(lane)) {
                Some(lane) => {
                    roles[i] = lane;
                    open.retain(|role| *role != lane);
                }
                None => unknown.push(i),
            }
        }
        if let ([i], [role]) = (unknown.as_slice(), open.as_slice()) {
            roles[*i] = *role;
        }
    }
    roles
}

/// Score carry de chaque participant (même ordre), de 0 à 100, calé sur la note de dpm.lol : 15 points de
/// base, plus (barème de chaque rôle dans Role::scale) :
/// - global : kills, morts, assistances, premier sang (5) ; or, dégâts, CS et vision par minute (de -4 à 10
///   chacun) ;
/// - couloir : écart d'or et de CS par minute avec le vis-à-vis (de -5 à 10) ;
/// - objectifs de l'équipe (2 par dragon et nashor, 3 par héraut, 0,5 par larve) et dégâts aux objectifs ;
/// - équipe : participation aux kills, part des dégâts et des dégâts encaissés ;
/// - résultat : +3 ou -3 ;
/// - rôle : première tour (5) et tours détruites ; balises et quête du support ; la moyenne de dpm.lol pour
///   ce que le client ne donne pas (plaques, solo kills, crabes...).
fn carry_scores(doc: &Value, participants: &[Value], roles: &[Role]) -> Vec<f64> {
    let minutes = (doc["gameDuration"].as_f64().unwrap_or(0.0) / 60.0).max(1.0);
    let stat = |p: &Value, key: &str| p["stats"][key].as_f64().unwrap_or(0.0);
    let flag = |p: &Value, key: &str| p["stats"][key].as_bool().unwrap_or(false);
    let team_total = |team: &Value, key: &str| -> f64 {
        participants.iter().filter(|p| &p["teamId"] == team).map(|p| stat(p, key)).sum()
    };
    let cs = |p: &Value| stat(p, "totalMinionsKilled") + stat(p, "neutralMinionsKilled");
    let team_objectives = |team: &Value| -> f64 {
        let teams = doc["teams"].as_array().into_iter().flatten();
        teams
            .filter(|t| &t["teamId"] == team)
            .map(|t| {
                let n = |key: &str| t[key].as_f64().unwrap_or(0.0);
                2.0 * n("dragonKills") + 3.0 * n("riftHeraldKills") + 2.0 * n("baronKills") + 0.5 * n("hordeKills")
            })
            .sum()
    };
    let team_towers = |team: &Value| -> f64 {
        let teams = doc["teams"].as_array().into_iter().flatten();
        teams.filter(|t| &t["teamId"] == team).map(|t| t["towerKills"].as_f64().unwrap_or(0.0)).sum()
    };
    let first_tower = |p: &Value| flag(p, "firstTowerKill") || flag(p, "firstTowerAssist");
    let final_support_item = |p: &Value| {
        (0..7).any(|k| p["stats"][format!("item{k}")].as_i64().is_some_and(|item| SUPPORT_ITEMS[3..].contains(&item)))
    };
    participants
        .iter()
        .zip(roles)
        .map(|(p, role)| {
            let s = role.scale();
            let team = &p["teamId"];
            let per_min = |x: f64| x / minutes;
            let share = |key: &str| 100.0 * stat(p, key) / team_total(team, key).max(1.0);
            let mut points = 15.0;
            // Global.
            points += s.kill * stat(p, "kills") + s.assist * stat(p, "assists") - s.death * stat(p, "deaths");
            if flag(p, "firstBloodKill") {
                points += 5.0;
            }
            if let Some((slope, reference)) = s.cs {
                points += (slope * (per_min(cs(p)) - reference)).clamp(-4.0, 10.0);
            }
            points += (s.gold.0 * (per_min(stat(p, "goldEarned")) - s.gold.1)).clamp(-4.0, 10.0);
            points += (s.damage.0 * per_min(stat(p, "totalDamageDealtToChampions")) + s.damage.1).clamp(0.0, 10.0);
            points += (s.vision.0 * per_min(stat(p, "visionScore")) + s.vision.1).clamp(-4.0, 10.0);
            // Couloir : écart avec le joueur du même rôle en face.
            let opponent = participants
                .iter()
                .zip(roles)
                .find(|(q, r)| *r == role && *role != Role::Other && &q["teamId"] != team)
                .map(|(q, _)| q);
            if let Some(opp) = opponent {
                let gap = per_min(stat(p, "goldEarned") - stat(opp, "goldEarned")) + 10.0 * per_min(cs(p) - cs(opp));
                points += (0.0311 * gap + 1.92).clamp(-5.0, 10.0);
            }
            // Objectifs.
            points += team_objectives(team);
            points += (s.objectives.0 * per_min(stat(p, "damageDealtToObjectives")) + s.objectives.1).clamp(0.0, 4.0);
            // Équipe.
            let kill_part = 100.0 * (stat(p, "kills") + stat(p, "assists")) / team_total(team, "kills").max(1.0);
            points += (s.kill_share * (kill_part - 25.0)).clamp(-4.0, 10.0);
            points += s.damage_share * share("totalDamageDealtToChampions") + s.taken_share * share("totalDamageTaken");
            // Résultat.
            points += if flag(p, "win") { 3.0 } else { -3.0 };
            // Rôle.
            points += s.missing + s.turret * stat(p, "turretKills");
            // Première tour de son couloir : +5 au joueur dont l'équipe l'a prise. Le client ne dit que qui a pris
            // la première tour de la partie ; dans les autres couloirs, c'est l'équipe gagnante (vrai pour 30
            // couloirs sur 36 sur dpm.lol), si elle a pris au moins une tour.
            if matches!(role, Role::Top | Role::Mid | Role::Bot) {
                let lane_first = first_tower(p)
                    || (!opponent.is_some_and(&first_tower) && flag(p, "win") && team_towers(team) > 0.0);
                if lane_first {
                    points += 5.0;
                }
            }
            if *role == Role::Support {
                points += (0.44 * stat(p, "visionWardsBoughtInGame") + 0.27).clamp(-3.0, 3.5);
                points += (0.21 * stat(p, "wardsKilled") + 1.0).clamp(0.0, 3.5);
                points += (0.011 * stat(p, "wardsPlaced") + 3.16).min(3.5);
                points += if final_support_item(p) { 1.5 } else { -3.0 };
            }
            points.clamp(0.0, 100.0)
        })
        .collect()
}

/// Les deux équipes d'une partie complète du client ; « toi » = le joueur de ton champion.
pub fn match_from(doc: &Value, champion_id: i64) -> Option<MatchDetails> {
    let participants = doc["participants"].as_array()?;
    let identities = doc["participantIdentities"].as_array();
    let my_team = &participants.iter().find(|p| p["championId"] == champion_id)?["teamId"];
    // Score « carry » et place de chaque participant (par son numéro dans la partie).
    let roles = roles(doc, participants);
    // Rôles trouvés, à côté du couloir annoncé par le client (pour vérifier sur une vraie partie).
    let name_of = |p: &Value| {
        identities
            .into_iter()
            .flatten()
            .find(|identity| identity["participantId"] == p["participantId"])
            .and_then(|identity| identity["player"]["gameName"].as_str())
            .unwrap_or("?")
    };
    let found: Vec<String> = participants
        .iter()
        .zip(&roles)
        .map(|(p, role)| {
            let lane = |key: &str| p["timeline"][key].as_str().unwrap_or("-").to_string();
            format!("{} {:?} (client {}/{})", name_of(p), role, lane("lane"), lane("role"))
        })
        .collect();
    eprintln!("[détail] partie {} : {}", doc["gameId"], found.join(", "));
    let scores = carry_scores(doc, participants, &roles);
    let mut order: Vec<usize> = (0..participants.len()).collect();
    order.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]));
    let carry = |p: &Value| -> (f64, u32) {
        let i = participants.iter().position(|q| q["participantId"] == p["participantId"]).unwrap_or(0);
        let place = order.iter().position(|j| *j == i).unwrap_or(0) as u32 + 1;
        (scores.get(i).copied().unwrap_or(0.0).round(), place)
    };
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
        let (score, place) = carry(p);
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
            score,
            place,
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
