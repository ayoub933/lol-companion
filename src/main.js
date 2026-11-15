const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Phases renvoyées par le client (/lol-gameflow/v1/gameflow-phase).
const PHASES = {
  None: "Accueil",
  Lobby: "Salon",
  Matchmaking: "Recherche de partie",
  CheckedIntoTournament: "Inscrit au tournoi",
  ReadyCheck: "Partie trouvée",
  ChampSelect: "Sélection des champions",
  GameStart: "Lancement de la partie",
  FailedToLaunch: "Échec du lancement",
  InProgress: "En partie",
  Reconnect: "Reconnexion",
  WaitingForStats: "Attente des stats",
  PreEndOfGame: "Fin de partie",
  EndOfGame: "Fin de partie",
  TerminatedInError: "Partie interrompue",
};

const POSITIONS = { top: "haut", jungle: "jungle", mid: "milieu", adc: "bas", support: "support" };
// Postes tels que le client les nomme.
const CLIENT_POSITIONS = { top: "haut", jungle: "jungle", middle: "milieu", bottom: "bas", utility: "support" };
// Style de jeu conseillé par OP.GG ("even" : rien à ajouter, le duel est déjà dit équilibré).
const PLAY_STYLES = { defensive: "joue prudemment", aggressive: "joue agressif", even: "" };
const TIERS = {
  IRON: "Fer",
  BRONZE: "Bronze",
  SILVER: "Argent",
  GOLD: "Or",
  PLATINUM: "Platine",
  EMERALD: "Émeraude",
  DIAMOND: "Diamant",
  MASTER: "Maître",
  GRANDMASTER: "Grand Maître",
  CHALLENGER: "Challenger",
};
// À partir de Maître : pas de division, seulement des LP.
const APEX = ["MASTER", "GRANDMASTER", "CHALLENGER"];
// Files d'attente (queues.json de Riot).
const QUEUES = {
  0: "Personnalisée",
  400: "Normale (draft)",
  420: "Classée solo/duo",
  430: "Normale (aveugle)",
  440: "Classée flexible",
  450: "ARAM",
  480: "Swiftplay",
  490: "Partie rapide",
  700: "Clash",
  900: "URF",
  1700: "Arena",
  1710: "Arena",
  1900: "URF",
};

const $ = (id) => document.getElementById(id);
// Icône d'un champion, servie par l'app depuis le client (lcu://localhost/<numéro>).
const iconUrl = (id) => convertFileSrc(String(id), "lcu");

// Met l'icône d'un champion dans une image (rien à faire si c'est déjà la bonne).
// Si le client était trop occupé pour la donner, on réessaie une fois, 2 secondes plus tard.
function setIcon(img, id) {
  if (img.dataset.champion === String(id)) return;
  img.dataset.champion = String(id);
  img.onerror = () => {
    img.onerror = null;
    setTimeout(() => {
      if (img.dataset.champion === String(id)) img.src = `${iconUrl(id)}?retry`;
    }, 2000);
  };
  img.src = iconUrl(id);
}
const number = (value, digits = 1) => value.toLocaleString("fr-FR", { maximumFractionDigits: digits });
const percent = (wins, games) => number((wins / games) * 100);
// « milieu » ; rien dans les modes sans poste (ARAM...).
const lane = (runes) => (runes.laning && POSITIONS[runes.position] ? ` ${POSITIONS[runes.position]}` : "");
// « vs Sett » quand runes et build viennent du duel.
const versus = (runes) => (runes.matchup?.applied ? ` vs ${runes.matchup.opponent}` : "");
const kda = (k, d, a) => number((k + a) / Math.max(d, 1));
const record = (games, wins) => `${wins} V – ${games - wins} D`;

// « Émeraude II », « Maître · 245 LP », « Non classé ».
function rankLabel(rank) {
  if (!rank) return "";
  if (!rank.tier) return "Non classé";
  const name = TIERS[rank.tier] ?? rank.tier;
  return APEX.includes(rank.tier) ? `${name} · ${rank.lp} LP` : `${name} ${rank.division}`.trim();
}

// « Émeraude II · 45 LP · 56 % sur 120 parties »
function rankLine(rank) {
  if (!rank?.tier) return rankLabel(rank) || "—";
  const parts = [rankLabel(rank)];
  if (!APEX.includes(rank.tier)) parts.push(`${rank.lp} LP`);
  const games = rank.wins + rank.losses;
  if (games) parts.push(`${percent(rank.wins, games)} % sur ${games} parties`);
  return parts.join(" · ");
}

function renderProfile(state) {
  const profile = state.profile;
  // Seulement hors sélection et hors partie : la place y sert à autre chose.
  $("profile").hidden = !profile || Boolean(state.runes || state.game);
  if ($("profile").hidden) return;
  const rows = [
    ["Solo/duo", rankLine(profile.solo)],
    ["Flexible", rankLine(profile.flex)],
  ];
  $("ranks").replaceChildren(
    ...rows.flatMap(([label, value]) => {
      const dt = document.createElement("dt");
      const dd = document.createElement("dd");
      dt.textContent = label;
      dd.textContent = value;
      return [dt, dd];
    }),
  );
  const games = profile.recentGames;
  $("recent").textContent = games
    ? `${games} dernières parties : ${record(games, profile.recentWins)} (${percent(profile.recentWins, games)} %)`
    : "";
  $("recent-champions").replaceChildren(
    ...profile.recentChampions.map((c) =>
      championRow({ championId: c.championId, name: c.name, detail: `${c.games} parties · ${record(c.games, c.wins)}` }),
    ),
  );
}

function renderRunes(runes) {
  $("runes").hidden = !runes;
  if (!runes) return;
  $("runes-icon").hidden = !runes.championId;
  if (runes.championId) setIcon($("runes-icon"), runes.championId);
  let title, detail, stats = "";
  if (!runes.champion) {
    title = "En attente";
    detail = "Verrouille ton champion pour importer ses runes.";
  } else if (runes.pending) {
    const opponent = runes.enemies.find((e) => e.id === runes.opponentId);
    title = "Recherche des runes…";
    detail = `${runes.champion}${lane(runes)}${opponent ? ` vs ${opponent.name}` : ""} · OP.GG`;
  } else if (runes.error) {
    title = runes.champion;
    detail = runes.error;
  } else {
    title = `${runes.keystone} · ${runes.primary} / ${runes.secondary}`;
    const applied = runes.pageApplied ? "page « LoL Companion » appliquée" : "import des runes coupé dans les réglages";
    detail = `${runes.champion}${lane(runes)}${versus(runes)} · ${applied}`;
    if (runes.games > 0) {
      stats = `${percent(runes.wins, runes.games)} % de victoires sur ${runes.games.toLocaleString("fr-FR")} parties`;
    }
  }
  $("runes-title").textContent = title;
  $("runes-detail").textContent = detail;
  $("runes-detail").classList.toggle("error", Boolean(runes.error));
  $("runes-stats").textContent = stats;
}

// Une ligne de colonne : icône (avec le niveau en partie), nom et une ligne de détail.
function championRow({ championId, name, detail = "", level, me, faded, opponent, onClick }) {
  const row = document.createElement(onClick ? "button" : "div");
  row.className = ["player", me && "me", faded && "faded", opponent && "opponent"].filter(Boolean).join(" ");
  if (onClick) row.addEventListener("click", onClick);
  const portrait = document.createElement("div");
  portrait.className = "portrait";
  if (championId) {
    const img = document.createElement("img");
    setIcon(img, championId);
    img.alt = name;
    portrait.append(img);
  } else {
    // Champion inconnu du client : son initiale ; pas encore choisi : case vide.
    const empty = document.createElement("span");
    empty.className = name ? "initial" : "slot";
    empty.textContent = name.charAt(0);
    portrait.append(empty);
  }
  if (level) {
    const badge = document.createElement("span");
    badge.className = "lvl";
    badge.textContent = level;
    portrait.append(badge);
  }
  const title = document.createElement("div");
  title.className = "name";
  title.textContent = name || "…";
  const sub = document.createElement("div");
  sub.className = "muted";
  sub.textContent = detail;
  const text = document.createElement("div");
  text.append(title, sub);
  row.append(portrait, text);
  return row;
}

// Sélection des champions : les deux équipes, et le duel contre ton adversaire direct.
function renderOpponent(runes) {
  $("opponent").hidden = !runes;
  if (!runes) return;
  const { allies, enemies, laning, opponentId, opponentManual: manual, matchup } = runes;
  $("select-allies").replaceChildren(
    ...allies.map((ally) =>
      championRow({
        championId: ally.championId,
        name: ally.name,
        detail: CLIENT_POSITIONS[ally.position] ?? "",
        me: ally.me,
        faded: ally.championId && !ally.locked,
      }),
    ),
  );
  // Adversaires : ceux déjà verrouillés, puis des cases vides (5 en tout dans les modes avec postes).
  const enemyRows = enemies.map((enemy) => {
    const isOpponent = enemy.id === opponentId;
    return championRow({
      championId: enemy.id,
      name: enemy.name,
      detail: isOpponent ? (manual ? "choisi" : "deviné") : "",
      opponent: isOpponent,
      // Clic : cet adversaire ; reclic sur celui choisi : retour à la devinette.
      onClick: laning
        ? () => invoke("set_opponent", { championId: manual && isOpponent ? null : enemy.id })
        : null,
    });
  });
  const slots = laning ? Math.max(0, allies.length - enemies.length) : 0;
  for (let i = 0; i < slots; i++) enemyRows.push(championRow({ name: "" }));
  if (!enemyRows.length) {
    const hidden = document.createElement("div");
    hidden.className = "muted note";
    hidden.textContent = "Choix cachés dans ce mode";
    enemyRows.push(hidden);
  }
  $("select-enemies").replaceChildren(...enemyRows);
  $("select-enemies").classList.toggle("has-choice", Boolean(opponentId));

  $("duel").hidden = !laning;
  if (!laning) return;
  $("opponent-mode").textContent = !enemies.length
    ? "En attente des choix adverses…"
    : manual
      ? "Adversaire direct choisi · reclique dessus pour revenir à la devinette"
      : opponentId
        ? "Adversaire direct deviné · clique sur un autre pour corriger"
        : "Clique sur ton adversaire direct";

  let stats = "", tip = "";
  if (!enemies.length || !runes.championId) {
    stats = "";
  } else if (matchup?.error) {
    stats = `Duel contre ${matchup.opponent} : ${matchup.error}`;
  } else if (matchup) {
    const parts = [`Contre ${matchup.opponent}`];
    if (matchup.games > 0) {
      parts[0] += ` : ${percent(matchup.wins, matchup.games)} % de victoires sur ${matchup.games.toLocaleString("fr-FR")} parties`;
    }
    if (matchup.advantage.toUpperCase() === "EVEN") parts.push("duel équilibré");
    else if (matchup.advantage) parts.push(`avantage ${matchup.advantage}`);
    const style = PLAY_STYLES[matchup.playStyle] ?? matchup.playStyle;
    if (style) parts.push(style);
    if (!matchup.applied) parts.push("trop peu de parties : runes générales gardées");
    stats = parts.join(" · ");
    tip = matchup.tip;
  }
  $("matchup-stats").textContent = stats;
  $("matchup-stats").classList.toggle("error", Boolean(matchup?.error));
  $("duel-history").textContent = runes.duelGames
    ? `Ton bilan dans ce duel : ${record(runes.duelGames, runes.duelWins)}`
    : "";
  $("matchup-tip").textContent = tip;
}

// Avant de verrouiller : qui bat ton adversaire, qui va bien avec ton partenaire.
function renderHelp(runes) {
  const help = runes?.help;
  $("help").hidden = !help;
  if (!help) return;
  const block = (title, picks, detail) => {
    if (!title) return [];
    const head = document.createElement("div");
    head.className = "k help-title";
    head.textContent = title;
    return [head, ...picks.map((p) => championRow({ championId: p.championId, name: p.name, detail: detail(p) }))];
  };
  const rate = (p) => `${number(p.winRate * 100)} %`;
  $("help-counters").replaceChildren(
    ...block(help.opponent && `Contre ${help.opponent}`, help.counters, (p) => `${rate(p)} de victoires contre lui`),
  );
  $("help-synergies").replaceChildren(
    ...block(help.partner && `Avec ${help.partner}`, help.synergies, (p) => `${rate(p)} de victoires en duo`),
  );
  const empty = !help.counters.length && !help.synergies.length;
  $("help-pending").textContent = help.pending ? "Recherche sur OP.GG…" : empty ? "Pas de suggestion d'OP.GG ici." : "";
}

// En partie : les 10 champions, avec rang, niveau et KDA (rang seul pendant l'écran de chargement).
function renderTeams(game) {
  const show = Boolean(game && game.allies.length + game.enemies.length);
  $("teams").hidden = !show;
  if (!show) return;
  const row = (player) =>
    championRow({
      championId: player.championId,
      name: player.champion,
      detail: [game.ready && `${player.kills} / ${player.deaths} / ${player.assists}`, rankLabel(player.rank)]
        .filter(Boolean)
        .join(" · "),
      level: game.ready && player.level,
      me: player.me,
    });
  $("team-allies").replaceChildren(...game.allies.map(row));
  $("team-enemies").replaceChildren(...game.enemies.map(row));
}

// « Anneau de Doran, Potion de soin ×2 »
const itemNames = (items, separator = ", ") =>
  items.map((it) => (it.count > 1 ? `${it.name} ×${it.count}` : it.name)).join(separator);

function renderBuild(runes) {
  const build = runes?.build;
  $("build").hidden = !build;
  if (!build) return;
  const [d, f] = runes.spellsApplied;
  const rows = [
    ["Sorts", f ? `D ${d} · F ${f}` : runes.spells.join(" + ")],
    ["Départ", itemNames(build.starter)],
    ["Principal", itemNames(build.core, " → ")],
    ["Bottes", itemNames(build.boots)],
    ["Ensuite", itemNames(build.options, " / ")],
    ["Montée", build.skillMax.join(" > ")],
    ["Niveaux", build.skillOrder.join(" ")],
  ].filter(([, value]) => value);
  $("build-list").replaceChildren(
    ...rows.flatMap(([label, value]) => {
      const dt = document.createElement("dt");
      const dd = document.createElement("dd");
      dt.textContent = label;
      dd.textContent = value;
      if (label === "Niveaux") dd.className = "mono";
      return [dt, dd];
    }),
  );
  $("spells-note").textContent = runes.spellsNote ?? (f ? "Sorts mis dans le client" : "");
  $("item-set").textContent = runes.itemSetOk
    ? `Dans la boutique : set « LoL Companion · ${runes.champion}${versus(runes)} »`
    : (runes.itemSetError ?? "");
  $("item-set").classList.toggle("error", Boolean(runes.itemSetError));
}

// Après la partie : tes stats exactes (CS compris), d'après l'historique du client.
function renderLastGame(state) {
  const last = state.history?.last;
  const lobby = !state.runes && !state.game;
  $("last-game").hidden = !lobby || !(last || state.recording);
  if ($("last-game").hidden) return;
  if (state.recording && !last) {
    $("last-icon").hidden = true;
    $("last-when").textContent = "";
    $("last-title").textContent = "Partie terminée";
    $("last-detail").textContent = "Récupération de tes stats dans l'historique du client…";
    $("last-grid").replaceChildren();
    return;
  }
  const minutes = last.duration / 60;
  $("last-icon").hidden = !last.championId;
  if (last.championId) setIcon($("last-icon"), last.championId);
  // Pendant la récupération de la partie qui vient de finir, la précédente reste affichée.
  $("last-when").textContent = state.recording
    ? "mise à jour…"
    : new Date(last.date).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
  $("last-title").textContent = `${last.win ? "Victoire" : "Défaite"} · ${last.champion}`;
  $("last-title").className = `big ${last.win ? "win" : "loss"}`;
  $("last-detail").textContent = [
    QUEUES[last.queueId],
    POSITIONS[last.position],
    last.opponent && `contre ${last.opponent}`,
    `${Math.round(minutes)} min`,
  ]
    .filter(Boolean)
    .join(" · ");
  const cells = [
    ["KDA", `${last.kills} / ${last.deaths} / ${last.assists}`],
    ["CS", `${last.cs} · ${number(last.cs / Math.max(minutes, 1))}/min`],
    ["Or", last.gold.toLocaleString("fr-FR")],
    ["Dégâts", last.damage.toLocaleString("fr-FR")],
    ["Vision", String(last.vision)],
    ["Ratio KDA", kda(last.kills, last.deaths, last.assists)],
  ];
  $("last-grid").replaceChildren(
    ...cells.map(([label, value]) => {
      const cell = document.createElement("div");
      const k = document.createElement("span");
      const v = document.createElement("span");
      k.className = "k";
      k.textContent = label;
      v.textContent = value;
      cell.append(k, v);
      return cell;
    }),
  );
}

// Moyennes par champion (et par duel) sur les parties enregistrées par l'app.
function renderStats(state) {
  const history = state.history;
  $("stats").hidden = Boolean(!history?.champions.length || state.runes || state.game);
  if ($("stats").hidden) return;
  const detail = (t) => {
    const parts = [`${t.games} partie${t.games > 1 ? "s" : ""}`, `${percent(t.wins, t.games)} %`, `KDA ${kda(t.kills, t.deaths, t.assists)}`];
    if (!t.aram && t.duration) parts.push(`${number(t.cs / (t.duration / 60))} CS/min`);
    return parts.join(" · ");
  };
  $("stats-champions").replaceChildren(
    ...history.champions.map((t) =>
      championRow({ championId: t.championId, name: t.aram ? `${t.champion} (ARAM)` : t.champion, detail: detail(t) }),
    ),
  );
  $("duels-label").hidden = !history.duels.length;
  $("stats-duels").replaceChildren(
    ...history.duels.map((t) =>
      championRow({
        championId: t.championId,
        name: `${t.champion} contre ${t.opponent}`,
        detail: `${t.games} partie${t.games > 1 ? "s" : ""} · ${record(t.games, t.wins)}`,
      }),
    ),
  );
}

function formatTime(seconds) {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

function render(state) {
  const found = state.clientFound;
  $("status").textContent = found ? "Client détecté" : "Client non détecté";
  $("status").className = `pill ${found ? "on" : "off"}`;
  $("hint").hidden = found;

  $("update").hidden = !state.update;
  if (state.update && !$("update-button").disabled) {
    $("update-text").textContent = `Nouvelle version ${state.update} disponible`;
  }

  $("account").hidden = !found;
  $("riot-id").textContent = state.riotId ?? "Connexion au compte…";
  $("level").textContent = state.summonerLevel != null ? `Niveau ${state.summonerLevel}` : "";
  renderProfile(state);

  $("phase-card").hidden = !found;
  $("phase").textContent = PHASES[state.phase] ?? state.phase ?? "—";

  renderRunes(state.runes);
  renderOpponent(state.runes);
  renderHelp(state.runes);
  renderBuild(state.runes);
  renderLastGame(state);
  renderStats(state);

  const game = state.game;
  renderTeams(game);
  // Fenêtre large : les équipes passent dans une colonne à droite (voir styles.css).
  $("main").classList.toggle("split", !$("opponent").hidden || !$("teams").hidden);
  $("game").hidden = !game;
  if (!game) return;

  $("game-loading").hidden = game.ready;
  $("game-stats").hidden = !game.ready;
  $("game-time").textContent = game.ready ? formatTime(game.gameTime) : "";
  if (!game.ready) return;

  $("champion").textContent = `${game.champion} · niveau ${game.level}`;
  $("kda").textContent = `${game.kills} / ${game.deaths} / ${game.assists}`;
  // L'API de la partie ne donne les CS que par paliers de 10 (0, 10, 20…) : on affiche la fourchette,
  // et le rythme par minute estimé au milieu de la fourchette. Le compte exact arrive après la partie.
  const minutes = game.gameTime / 60;
  const range = `${game.cs}–${game.cs + 9}`;
  const perMinute = number((game.cs + 4.5) / minutes);
  $("cs").textContent = range;
  $("cs-rate").textContent = game.cs >= 10 && minutes >= 1 ? `≈ ${perMinute} par minute` : "";
  $("gold").textContent = game.gold.toLocaleString("fr-FR");
  $("items").replaceChildren(
    ...game.items.map((name) => {
      const li = document.createElement("li");
      li.textContent = name;
      return li;
    }),
  );
}

// ---------- Réglages ----------
const settingInputs = () => [...document.querySelectorAll("[data-setting]")];

function showSettings(settings) {
  for (const input of settingInputs()) {
    const value = settings[input.dataset.setting];
    if (input.type === "checkbox") input.checked = value;
    else input.value = String(value);
  }
}

async function saveSettings() {
  const settings = await invoke("get_settings");
  for (const input of settingInputs()) {
    const key = input.dataset.setting;
    if (input.type === "checkbox") settings[key] = input.checked;
    // Les listes donnent du texte : "true"/"false" redevient un booléen.
    else settings[key] = typeof settings[key] === "boolean" ? input.value === "true" : input.value;
  }
  try {
    showSettings(await invoke("set_settings", { settings }));
    $("settings-status").textContent = "Enregistré";
    $("settings-status").classList.remove("error");
  } catch (error) {
    $("settings-status").textContent = String(error);
    $("settings-status").classList.add("error");
    showSettings(await invoke("get_settings"));
  }
}

async function toggleSettings() {
  const open = $("settings").hidden;
  if (open) {
    showSettings(await invoke("get_settings"));
    $("settings-status").textContent = "";
  }
  $("settings").hidden = !open;
}

window.addEventListener("DOMContentLoaded", async () => {
  $("settings-button").addEventListener("click", toggleSettings);
  $("settings-close").addEventListener("click", toggleSettings);
  for (const input of settingInputs()) input.addEventListener("change", saveSettings);
  $("update-button").addEventListener("click", async () => {
    $("update-button").disabled = true;
    $("update-text").textContent = "Téléchargement de la mise à jour… l'app va redémarrer.";
    try {
      await invoke("install_update");
    } catch (error) {
      $("update-text").textContent = String(error);
      $("update-button").disabled = false;
    }
  });
  // D'abord écouter les mises à jour, puis récupérer l'état actuel.
  await listen("lol-state", (event) => render(event.payload));
  render(await invoke("get_state"));
});
