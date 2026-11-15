const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// Phase du client (/lol-gameflow/v1/gameflow-phase), telle que l'en-tête l'affiche.
const PHASES = {
  None: "CLIENT CONNECTÉ",
  Lobby: "SALON",
  Matchmaking: "EN RECHERCHE",
  CheckedIntoTournament: "INSCRIT AU TOURNOI",
  ReadyCheck: "PARTIE TROUVÉE",
  ChampSelect: "SÉLECTION",
  GameStart: "LANCEMENT",
  FailedToLaunch: "ÉCHEC DU LANCEMENT",
  InProgress: "EN PARTIE",
  Reconnect: "RECONNEXION",
  WaitingForStats: "FIN DE PARTIE",
  PreEndOfGame: "FIN DE PARTIE",
  EndOfGame: "FIN DE PARTIE",
  TerminatedInError: "PARTIE INTERROMPUE",
};

// Postes au sens d'OP.GG (sélection, historique).
const POSITIONS = { top: "TOP", jungle: "JUNGLE", mid: "MILIEU", adc: "BAS", support: "SUPPORT" };
// Postes tels que le client les nomme en sélection, en abrégé pour la liste des champions.
const CLIENT_POSITIONS = { top: "TOP", jungle: "JGL", middle: "MID", bottom: "BOT", utility: "SUP" };
// Postes au sens d'OP.GG, en abrégé pour les lignes de l'historique.
const SHORT_POSITIONS = { top: "TOP", jungle: "JGL", mid: "MID", adc: "BOT", support: "SUP" };
// Postes de la partie en cours, dans l'ordre du face-à-face.
const LANES = [
  ["TOP", "TOP"],
  ["JUNGLE", "JUNGLE"],
  ["MIDDLE", "MILIEU"],
  ["BOTTOM", "BAS"],
  ["UTILITY", "SUPPORT"],
];
const LANE_NAMES = Object.fromEntries(LANES);
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
// Files d'attente (queues.json de Riot), en court.
const QUEUES = {
  0: "PERSONNALISÉE",
  400: "NORMALE",
  420: "SOLO/DUO",
  430: "AVEUGLE",
  440: "FLEXIBLE",
  450: "ARAM",
  480: "SWIFTPLAY",
  490: "PARTIE RAPIDE",
  700: "CLASH",
  900: "URF",
  1700: "ARENA",
  1710: "ARENA",
  1900: "URF",
};

const $ = (id) => document.getElementById(id);

// Crée un élément : el("span", "classe", "texte") ou el("div", "classe", [enfants ou textes]).
function el(tag, className, content) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (Array.isArray(content)) node.append(...content.filter((c) => c != null && c !== false && c !== ""));
  else if (content != null) node.textContent = String(content);
  return node;
}

// ---------- Images servies par l'app depuis le client (lcu://localhost/...) ----------

const iconUrl = (id) => convertFileSrc(String(id), "lcu");
const splashUrl = (skinId) => convertFileSrc(`splash-${skinId}`, "lcu");

// Met une image du client dans <img> (rien à faire si c'est déjà la bonne).
// Si le client était trop occupé pour la donner, on réessaie une fois, 2 secondes plus tard.
function setImage(img, url) {
  if (img.dataset.url === url) return;
  img.dataset.url = url;
  img.onerror = () => {
    img.onerror = null;
    setTimeout(() => {
      if (img.dataset.url === url) img.src = `${url}?retry`;
    }, 2000);
  };
  img.src = url;
}

// Illustration d'un skin : cachée tant qu'elle n'est pas chargée (les bandes de couleur restent derrière).
// 0 = pas d'illustration.
function setArt(img, skinId) {
  if (!skinId) {
    img.hidden = true;
    delete img.dataset.url;
    img.removeAttribute("src");
    return;
  }
  const url = splashUrl(skinId);
  if (img.dataset.url === url) return;
  img.hidden = true;
  img.onload = () => {
    if (img.dataset.url === url || img.dataset.url === `${url}?retry`) img.hidden = false;
  };
  setImage(img, url);
}

// Icône d'un champion ; inconnu du client : son initiale ; pas encore choisi : case vide.
function champIcon(id, name) {
  if (id > 0) {
    const img = el("img", "icon");
    img.alt = "";
    setImage(img, iconUrl(id));
    return img;
  }
  return el("span", name ? "icon" : "icon slot", name ? name.charAt(0) : "");
}

// ---------- Mise en forme ----------

const number = (value, digits = 1) => value.toLocaleString("fr-FR", { maximumFractionDigits: digits });
// Toujours une décimale : « 5,0 ».
const decimal = (value) => value.toLocaleString("fr-FR", { minimumFractionDigits: 1, maximumFractionDigits: 1 });
const whole = (value) => Math.round(value).toLocaleString("fr-FR");
const kda = (k, d, a) => decimal((k + a) / Math.max(d, 1));
const upper = (text) => (text ?? "").toLocaleUpperCase("fr-FR");
const plural = (count, word) => `${count} ${word}${count > 1 ? "s" : ""}`;
// « +950 », « −500 », « 0 ».
const signed = (n) => (n > 0 ? `+${whole(n)}` : n < 0 ? `−${whole(-n)}` : "0");
const signClass = (n) => (n > 0 ? "pos" : n < 0 ? "neg" : "even");
// Écart d'or : en dessous de 100 pièces, à égalité ; pas encore connu : « even » aussi.
const EVEN_GOLD = 100;
const gapClass = (gap) => (gap == null || Math.abs(gap) < EVEN_GOLD ? "even" : signClass(gap));
// « vs Sett » quand runes et build viennent du duel.
const versus = (runes) => (runes.matchup?.applied ? ` vs ${runes.matchup.opponent}` : "");

function clock(seconds) {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

// Victoires en vert-bleu, défaites en rouge-rose : « 12–7 ».
const record = (wins, losses, separator = "–") => [el("span", "win", wins), separator, el("span", "loss", losses)];

// Emblème d'un rang (image du client, le nom au survol). Si l'image manque, même après un nouvel essai
// 2 secondes plus tard, le nom du rang en texte à sa place.
function emblem(tier, size) {
  const name = TIERS[tier] ?? tier;
  const box = el("span", `emblem ${size}`);
  box.title = name;
  const img = document.createElement("img");
  img.alt = name;
  const url = iconUrl(`rank-${tier.toLowerCase()}`);
  img.onerror = () => {
    if (img.dataset.retried) return box.replaceWith(el("span", "tier-name", upper(name)));
    img.dataset.retried = "1";
    setTimeout(() => (img.src = `${url}?retry`), 2000);
  };
  img.src = url;
  box.append(img);
  return box;
}

// Rang : son emblème et sa division (« II ») ; à partir de Maître, ses LP. « NON CLASSÉ » sans rang.
// Rien n'est refait si le rang affiché n'a pas changé (l'image ne clignote pas).
function setRank(node, rank, size, withLp = false) {
  const key = rank ? `${rank.tier}|${rank.division}|${withLp ? rank.lp : ""}|${size}` : "";
  if (node.dataset.rank === key && node.childElementCount) return;
  node.dataset.rank = key;
  if (!rank?.tier) return node.replaceChildren(el("span", "tier-name", "NON CLASSÉ"));
  const apex = APEX.includes(rank.tier);
  const detail = apex ? (withLp ? `${rank.lp} LP` : "") : rank.division;
  node.replaceChildren(emblem(rank.tier, size), ...(detail ? [el("span", "division", detail)] : []));
}

// Taille du nom selon sa longueur, pour qu'il tienne sur une ligne.
function setName(node, text, waiting = false) {
  node.textContent = text;
  node.classList.toggle("waiting", waiting);
  node.classList.toggle("mid", text.length > 8 && text.length <= 11);
  node.classList.toggle("long", text.length > 11);
}

const cell = ([label, value, className]) =>
  el("div", "cell", [el("div", "k", label), el("div", `v ${className ?? ""}`, value)]);
const numCell = (label, value) => el("div", "num-cell", [el("span", "k", label), el("span", "v", value)]);
const bar = (rate) => {
  const fill = el("span");
  fill.style.width = `${Math.max(0, Math.min(100, rate))}%`;
  return el("div", "bar", [fill]);
};

// ---------- Salon ----------

// Illustration du salon : le skin de ta dernière partie suivie par l'app,
// sinon l'illustration de base du dernier champion que tu as joué.
function heroSkin(state) {
  const last = state.history?.last;
  const recent = state.profile?.lastChampionId || 0;
  if (last?.skinId && (!recent || recent === last.championId)) return last.skinId;
  const champion = recent || last?.championId || 0;
  return champion > 0 ? champion * 1000 : 0;
}

function renderPlayer(state, show) {
  for (const id of ["player", "form", "moment"]) $(id).hidden = !show;
  if (!show) return;
  const riotId = state.riotId ?? "";
  const cut = riotId.lastIndexOf("#");
  const name = cut >= 0 ? riotId.slice(0, cut) : riotId;
  const tag = cut >= 0 ? riotId.slice(cut + 1) : "";
  const title = name ? upper(name) : "CONNEXION…";
  $("riot-name").textContent = title;
  $("riot-name").className = `name-box ${title.length > 12 ? "long" : title.length > 9 ? "mid" : ""}`;
  const line = [tag && `#${upper(tag)}`, state.summonerLevel != null && `NIVEAU ${state.summonerLevel}`];
  $("riot-tag").textContent = line.filter(Boolean).join(" · ");
  $("riot-tag").hidden = !$("riot-tag").textContent;
  setArt($("player-art"), heroSkin(state));

  const profile = state.profile;
  $("solo-rank").hidden = !profile;
  if (profile) {
    setRank($("solo-tier"), profile.solo, "big");
    $("solo-lp").textContent = profile.solo?.tier ? `${profile.solo.lp} LP` : "";
  }
  renderFigures(profile);
  renderForm(profile);
  renderMoment(profile);
}

// Profil pas encore lu : des tirets.
function renderFigures(profile) {
  if (!profile) {
    for (const id of ["solo-rate", "recent-record", "flex-tier"]) $(id).replaceChildren(el("span", "faint", "—"));
    delete $("flex-tier").dataset.rank;
    $("solo-rate-sub").textContent = "lecture du profil…";
    for (const id of ["recent-sub", "flex-sub"]) $(id).textContent = "";
    return;
  }
  const solo = profile.solo;
  const soloGames = solo ? solo.wins + solo.losses : 0;
  if (soloGames) {
    $("solo-rate").replaceChildren(number((solo.wins / soloGames) * 100, 0), el("small", "", " %"));
    $("solo-rate-sub").textContent = `victoires en solo/duo · ${plural(soloGames, "partie")}`;
  } else {
    $("solo-rate").replaceChildren(el("span", "faint", "—"));
    $("solo-rate-sub").textContent = "solo/duo : aucune partie classée";
  }

  const games = profile.recentGames;
  if (games) {
    $("recent-record").replaceChildren(...record(profile.recentWins, games - profile.recentWins));
    $("recent-sub").textContent = games > 1 ? `sur les ${games} dernières parties` : "sur ta dernière partie";
  } else {
    $("recent-record").replaceChildren(el("span", "faint", "—"));
    $("recent-sub").textContent = "aucune partie récente";
  }

  const flex = profile.flex;
  if (flex?.tier) {
    setRank($("flex-tier"), flex, "mid");
    $("flex-sub").textContent = `flexible · ${flex.lp} LP`;
  } else {
    $("flex-tier").replaceChildren(el("span", "faint", "—"));
    delete $("flex-tier").dataset.rank;
    $("flex-sub").textContent = "flexible : non classé";
  }
}

// Une barre par partie, de la plus récente à la plus ancienne : pleine = victoire, creuse = défaite.
function renderForm(profile) {
  const results = profile?.recentResults ?? [];
  for (const id of ["form-strip", "form-legend"]) $(id).hidden = !results.length;
  $("form-aside").textContent = "";
  if (!results.length) return;
  const wins = results.filter(Boolean).length;
  const count = results.length;
  $("form-aside").textContent = `${count} dernière${count > 1 ? "s" : ""} · ${number((wins / count) * 100, 0)} %`;
  $("form-strip").setAttribute(
    "aria-label",
    `Tes ${count} dernières parties, de la plus récente à la plus ancienne : ${plural(wins, "victoire")} et ${plural(count - wins, "défaite")}`,
  );
  $("form-strip").replaceChildren(...results.map((win) => el("span", win ? "w" : "l")));
}

function renderMoment(profile) {
  const list = profile?.recentChampions ?? [];
  $("moment-empty").hidden = list.length > 0;
  $("moment-empty").textContent = profile ? "Aucune partie récente." : "Lecture de tes dernières parties…";
  $("moment-cards").replaceChildren(
    ...list.map((c) =>
      el("div", "mcard", [
        champIcon(c.championId, c.name),
        el("span", "mname", upper(c.name)),
        el("span", "games", plural(c.games, "partie")),
        el("span", "rec", record(c.wins, c.games - c.wins, " – ")),
      ]),
    ),
  );
}

// « il y a 25 min », « il y a 3 h », « hier », « il y a 4 j », puis la date.
const relative = new Intl.RelativeTimeFormat("fr", { numeric: "auto", style: "short" });
function ago(ms) {
  const minutes = Math.round((Date.now() - ms) / 60000);
  if (minutes < 1) return "à l'instant";
  if (minutes < 60) return relative.format(-minutes, "minute");
  const hours = Math.round(minutes / 60);
  if (hours < 24) return relative.format(-hours, "hour");
  const days = Math.round(hours / 24);
  if (days < 7) return relative.format(-days, "day");
  return new Date(ms).toLocaleDateString("fr-FR", { day: "numeric", month: "short" });
}
// Fin de la partie (le client donne son début et sa durée).
const ended = (game) => game.date + game.duration * 1000;
const queueName = (game) => QUEUES[game.queueId] ?? (game.mapId === 12 ? "ARAM" : "AUTRE MODE");

// Tes 10 dernières parties (liste du client) : la plus récente en grand, avec toutes ses stats,
// les autres en lignes. Victoire en vert-bleu, défaite en rouge-rose.
function renderHistory(state, show) {
  $("history").hidden = !show;
  if (!show) return;
  const games = state.profile?.matches ?? [];
  const local = state.history?.last;
  // Une partie que l'app vient d'enregistrer peut manquer encore dans la liste du client.
  const featured = local && !(games[0]?.date >= local.date) ? local : games[0];
  const rows = games.filter((g) => g.gameId !== featured?.gameId).slice(0, featured ? 9 : 10);
  $("history-empty").hidden = Boolean(featured || rows.length || state.recording);
  $("history-empty").textContent = state.profile ? "Aucune partie récente." : "Lecture de ton historique…";
  renderFeatured(featured, state.recording);
  $("history-rows").replaceChildren(...rows.map(historyRow));
  const shown = [featured, ...rows].filter(Boolean);
  const wins = shown.filter((g) => g.win).length;
  $("history-aside").replaceChildren(
    ...(shown.length ? [`${shown.length > 1 ? `${shown.length} dernières` : "dernière"} · `, ...record(wins, shown.length - wins)] : []),
  );
}

// Après la partie : tes stats exactes (CS compris), d'après l'historique du client.
function renderFeatured(last, recording) {
  $("last-game").hidden = !last && !recording;
  if (!last) {
    if (!recording) return;
    $("last-game").className = "panel last neutral";
    $("last-result").textContent = "TERMINÉE";
    $("last-result").className = "result neutral";
    $("last-meta").textContent = "DERNIER MATCH";
    $("last-versus").replaceChildren(el("span", "muted", "Récupération de tes stats dans l'historique du client…"));
    $("last-grid").replaceChildren();
    $("last-when").textContent = "";
    return;
  }
  $("last-game").className = `panel last${last.win ? "" : " lost"}`;
  $("last-result").textContent = last.win ? "VICTOIRE" : "DÉFAITE";
  $("last-result").className = `result${last.win ? "" : " lost"}`;
  $("last-meta").textContent = ["DERNIER MATCH", queueName(last), clock(last.duration)].join(" · ");
  $("last-versus").replaceChildren(
    ...[
      el("span", "champ ally", upper(last.champion)),
      last.opponent && el("span", "vs", "VS"),
      last.opponent && el("span", "champ enemy", upper(last.opponent)),
      POSITIONS[last.position] && el("span", "lane-name", POSITIONS[last.position]),
    ].filter(Boolean),
  );
  const minutes = last.duration / 60;
  $("last-grid").replaceChildren(
    ...[
      ["KDA", `${last.kills}/${last.deaths}/${last.assists}`],
      ["Ratio KDA", kda(last.kills, last.deaths, last.assists), last.win ? "win" : "loss"],
      [`CS · ${decimal(last.cs / Math.max(minutes, 1))}/min`, String(last.cs)],
      ["Or", whole(last.gold)],
      ["Dégâts", whole(last.damage)],
      ["Vision", String(last.vision)],
    ].map(cell),
  );
  // Pendant la récupération de la partie qui vient de finir, la précédente reste affichée.
  const date = new Date(last.date).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
  $("last-when").textContent = recording ? "Mise à jour…" : `${ago(ended(last))} · ${date}`;
}

// Une partie de l'historique : champion, résultat et file, KDA, CS, durée et quand.
function historyRow(game) {
  const minutes = game.duration / 60;
  const lane = SHORT_POSITIONS[game.position];
  const value = (v, k) => el("div", "hnum", [el("span", "v", v), el("span", "k", k)]);
  return el("div", `hrow${game.win ? "" : " lost"}`, [
    champIcon(game.championId, game.champion),
    el("div", "hmain", [
      el("span", "hname", [upper(game.champion), game.opponent && el("small", "", ` VS ${upper(game.opponent)}`)]),
      el("span", "hmeta", [el("b", "", game.win ? "VICTOIRE" : "DÉFAITE"), ` · ${queueName(game)}${lane ? ` · ${lane}` : ""}`]),
    ]),
    value(`${game.kills}/${game.deaths}/${game.assists}`, `${kda(game.kills, game.deaths, game.assists)} KDA`),
    value(String(game.cs), `${decimal(game.cs / Math.max(minutes, 1))}/min`),
    value(clock(game.duration), ago(ended(game))),
  ]);
}

// Moyennes par champion, et bilan par duel, sur les parties enregistrées par l'app.
function renderStats(state, show) {
  for (const id of ["stats", "duels"]) $(id).hidden = !show;
  if (!show) return;
  const champions = state.history?.champions ?? [];
  const duels = state.history?.duels ?? [];
  $("stats-empty").hidden = champions.length > 0;
  $("stats-champions").replaceChildren(
    ...champions.map((t) => {
      const rate = (t.wins / t.games) * 100;
      const csPerMinute = !t.aram && t.duration ? decimal(t.cs / (t.duration / 60)) : "—";
      return el("div", "srow", [
        champIcon(t.championId, t.champion),
        el("div", "smid", [
          el("span", "sname", upper(t.aram ? `${t.champion} (ARAM)` : t.champion)),
          bar(rate),
          el("span", "meta", [`${plural(t.games, "partie")} · `, el("b", "", `${number(rate, 0)} %`)]),
        ]),
        numCell("KDA", kda(t.kills, t.deaths, t.assists)),
        numCell("CS/MIN", csPerMinute),
      ]);
    }),
  );
  $("duels-empty").hidden = duels.length > 0;
  $("stats-duels").replaceChildren(
    ...duels.map((t) =>
      el("div", "drow", [
        el("span", "", [upper(t.champion), el("small", "", " VS "), upper(t.opponent)]),
        el("span", "dscore", [...record(t.wins, t.games - t.wins), el("small", "", ` · ${plural(t.games, "partie")}`)]),
      ]),
    ),
  );
}

// ---------- Sélection des champions ----------

function renderSelection(runes) {
  for (const id of ["duel-hero", "duel", "picks", "help", "runes", "build"]) $(id).hidden = true;
  if (!runes) return;
  renderDuelHero(runes);
  renderDuel(runes);
  renderPicks(runes);
  renderHelp(runes);
  renderRunes(runes);
  renderBuild(runes);
}

// Toi (avec ton skin) contre ton adversaire direct (son illustration de base).
function renderDuelHero(runes) {
  $("duel-hero").hidden = false;
  const me = runes.allies.find((a) => a.me);
  const myId = runes.championId || me?.championId || 0;
  const myName = runes.champion || me?.name || "";
  const locked = Boolean(runes.champion);
  const skin = runes.skinId && Math.floor(runes.skinId / 1000) === myId ? runes.skinId : myId * 1000;
  setArt($("me-art"), myId > 0 ? skin : 0);
  const position = runes.laning ? POSITIONS[runes.position] : "";
  $("me-label").textContent = ["TOI", position, !locked && myId > 0 && "SURVOL"].filter(Boolean).join(" · ");
  setName($("me-name"), myName ? upper(myName) : "EN ATTENTE", !myName);

  $("duel-hero").classList.toggle("solo", !runes.laning);
  if (!runes.laning) return;
  const opponent = runes.enemies.find((e) => e.id === runes.opponentId);
  setArt($("them-art"), opponent ? opponent.id * 1000 : 0);
  $("them-label").textContent = !opponent
    ? "ADVERSAIRE"
    : runes.opponentManual
      ? "ADVERSAIRE CHOISI"
      : "ADVERSAIRE DEVINÉ";
  setName($("them-name"), opponent ? upper(opponent.name) : "?", !opponent);
}

// Le duel selon OP.GG, et ton bilan dans ce duel d'après l'historique de l'app.
function renderDuel(runes) {
  $("duel").hidden = false;
  const matchup = runes.matchup;
  const ready = runes.laning && runes.championId > 0 && runes.enemies.length > 0 && Boolean(matchup || runes.duelGames);
  $("duel-empty").hidden = ready;
  $("duel-empty").textContent = !runes.laning
    ? "Pas de duel dans ce mode : il n'y a pas de postes."
    : runes.championId > 0
      ? "Le duel s'affiche dès que ton adversaire direct est connu."
      : "Le duel s'affiche dès que tu choisis ton champion.";
  for (const id of ["duel-rates", "duel-bar", "duel-meta", "duel-record", "matchup-tip"]) $(id).hidden = !ready;
  $("matchup-games").textContent = "";
  if (!ready) return;
  const stats = matchup && !matchup.error && matchup.games > 0;
  $("duel-rates").hidden = !stats;
  $("duel-bar").hidden = !stats;
  $("matchup-games").textContent = stats ? `${whole(matchup.games)} parties analysées` : "";
  const edge = $("matchup-edge");
  edge.textContent = "";
  edge.className = "edge";
  if (stats) {
    const rate = (matchup.wins / matchup.games) * 100;
    $("rate-me").replaceChildren(number(rate, 0), el("small", "", " %"));
    $("rate-them").replaceChildren(number(100 - rate, 0), el("small", "", " %"));
    $("bar-me").style.width = `${rate}%`;
    const advantage = matchup.advantage ?? "";
    if (advantage.toUpperCase() === "EVEN") {
      edge.textContent = "duel équilibré";
    } else if (advantage) {
      edge.textContent = `avantage ${advantage}`;
      const same = (name) => name && name.toLowerCase() === advantage.toLowerCase();
      edge.classList.add(same(matchup.opponent) ? "them" : same(runes.champion) ? "me" : "neutral");
    }
  }
  const note = [];
  if (matchup?.error) note.push(`Duel contre ${matchup.opponent} : ${matchup.error}`);
  else if (matchup) {
    const style = PLAY_STYLES[matchup.playStyle] ?? matchup.playStyle;
    if (style) note.push(style.charAt(0).toUpperCase() + style.slice(1));
    if (!matchup.applied) note.push("trop peu de parties : runes générales gardées");
  }
  $("matchup-note").textContent = note.join(" · ");
  $("matchup-note").classList.toggle("error-text", Boolean(matchup?.error));

  const opponent = runes.enemies.find((e) => e.id === runes.opponentId);
  $("duel-record").hidden = !runes.duelGames;
  if (runes.duelGames) {
    $("duel-score").replaceChildren(...record(runes.duelWins, runes.duelGames - runes.duelWins));
    $("duel-who").textContent = `avec ${runes.champion}${opponent ? ` contre ${opponent.name}` : ""}`;
  }
  $("matchup-tip").hidden = !matchup?.tip;
  $("tip-text").textContent = matchup?.tip ? `« ${matchup.tip} »` : "";
}

// Les deux équipes ; un clic sur un adversaire le choisit comme adversaire direct.
function renderPicks(runes) {
  $("picks").hidden = false;
  const { allies, enemies, laning, opponentId, opponentManual: manual } = runes;
  $("picks-aside").textContent = !laning
    ? ""
    : !enemies.length
      ? "en attente des choix adverses"
      : manual
        ? "reclique dessus pour revenir à la devinette"
        : opponentId
          ? "clique ton adversaire si besoin"
          : "clique ton adversaire direct";

  $("pick-allies").replaceChildren(
    ...allies.map((ally) => {
      const hovered = ally.championId > 0 && !ally.locked && !ally.me;
      const tag = ally.me ? "TOI" : hovered ? "SURVOL" : (CLIENT_POSITIONS[ally.position] ?? "");
      const classes = ["pick", ally.me && "me", hovered && "faded", !ally.championId && "empty"];
      return el("div", classes.filter(Boolean).join(" "), [
        champIcon(ally.championId, ally.name),
        el("span", "pname", ally.name ? upper(ally.name) : "…"),
        el("span", "tag", tag),
      ]);
    }),
  );

  const rows = enemies.map((enemy) => {
    const isOpponent = enemy.id === opponentId;
    const row = el(laning ? "button" : "div", `pick enemy${isOpponent ? " opponent" : ""}`, [
      champIcon(enemy.id, enemy.name),
      el("span", "pname", upper(enemy.name)),
      el("span", "tag", isOpponent ? (manual ? "CHOISI" : "DEVINÉ") : ""),
    ]);
    if (laning) {
      row.type = "button";
      // Clic : cet adversaire ; reclic sur celui choisi : retour à la devinette.
      row.addEventListener("click", () =>
        invoke("set_opponent", { championId: manual && isOpponent ? null : enemy.id }),
      );
    }
    return row;
  });
  // Cases vides pour les adversaires pas encore verrouillés (5 en tout dans les modes avec postes).
  const slots = laning ? Math.max(0, allies.length - enemies.length) : 0;
  for (let i = 0; i < slots; i++) {
    rows.push(el("div", "pick enemy empty", [champIcon(0, ""), el("span", "pname", "…")]));
  }
  if (!rows.length) rows.push(el("div", "note", "Choix cachés dans ce mode"));
  $("pick-enemies").replaceChildren(...rows);
}

// Avant de verrouiller : les 10 champions qui battent le mieux ton adversaire (podium or, argent, bronze
// pour les 3 premiers), puis ceux qui vont le mieux avec ton partenaire.
const MEDALS = ["gold", "silver", "bronze"];
function renderHelp(runes) {
  const help = runes.help;
  $("help").hidden = !help;
  if (!help) return;
  const percent = (p) => `${decimal(p.winRate * 100)}\u00a0%`;
  const games = (n) => (n > 1 ? `${whole(n)} parties` : `${n} partie`);
  const tip = (p, place) =>
    `${place}. ${p.name} : ${percent(p)} de victoires contre ${help.opponent}${p.games ? ` (${games(p.games)})` : ""}`;
  const head = (title, aside) => el("div", "sub-head", [el("span", "kicker", title), el("span", "aside", aside)]);
  const counters = help.opponent && help.counters.length > 0;
  const blocks = [];
  if (counters) {
    const podium = help.counters.slice(0, 3).map((p, i) => {
      const card = el("div", `pod ${MEDALS[i]}`, [
        el("span", "place", i + 1),
        champIcon(p.championId, p.name),
        el("span", "pchamp", upper(p.name)),
        el("span", "pct", [decimal(p.winRate * 100), el("small", "", " %")]),
        p.games ? el("span", "pgames", games(p.games)) : null,
      ]);
      card.title = tip(p, i + 1);
      return card;
    });
    // Comme un vrai podium : le 2e à gauche, le 1er au milieu, le 3e à droite.
    if (podium.length > 1) podium.splice(0, 2, podium[1], podium[0]);
    const ranks = help.counters.slice(3).map((p, i) => {
      const row = el("div", "rrow", [
        el("span", "rank", i + 4),
        champIcon(p.championId, p.name),
        el("span", "rname", upper(p.name)),
        el("span", "rpct", percent(p)),
      ]);
      row.title = tip(p, i + 4);
      return row;
    });
    blocks.push(
      el("div", "counters", [
        head(`CONTRE ${upper(help.opponent)}`, "victoires contre lui"),
        el("div", "podium", podium),
        ranks.length ? el("div", "ranks", ranks) : null,
      ]),
    );
  }
  if (help.partner) {
    blocks.push(
      el("div", "duo", [
        head(`AVEC ${upper(help.partner)}`, "victoires en duo"),
        el(
          "div",
          "duo-list",
          help.synergies.map((p) =>
            el("div", "pick", [
              champIcon(p.championId, p.name),
              el("span", "pname", upper(p.name)),
              el("span", "tag", percent(p)),
            ]),
          ),
        ),
      ]),
    );
  }
  $("help-list").replaceChildren(...blocks);
  // Avec les contres au-dessus, les partenaires tiennent sur une ligne.
  $("help-list").classList.toggle("with-counters", counters);
  const empty = !help.counters.length && !help.synergies.length;
  $("help-pending").textContent = help.pending ? "Recherche sur OP.GG…" : empty ? "Pas de suggestion d'OP.GG ici." : "";
}

const CHECK =
  '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" aria-hidden="true"><path d="M5 12l5 5 9-10"></path></svg>';

function renderRunes(runes) {
  $("runes").hidden = false;
  let title;
  let trees = "";
  let rate = null;
  let games = "";
  let applied = "";
  let appliedClass = "applied span";
  if (!runes.champion) {
    title = "EN ATTENTE";
    trees = "Verrouille ton champion pour importer ses runes.";
  } else if (runes.pending) {
    const opponent = runes.enemies.find((e) => e.id === runes.opponentId);
    title = "RECHERCHE…";
    trees = `${runes.champion}${opponent ? ` contre ${opponent.name}` : ""} · OP.GG`;
  } else if (runes.error) {
    title = runes.champion;
    applied = runes.error;
    appliedClass += " error";
  } else {
    title = runes.keystone;
    trees = `${runes.primary} · ${runes.secondary}`;
    if (runes.games > 0) {
      rate = number((runes.wins / runes.games) * 100, 0);
      games = `${whole(runes.games)} parties`;
    }
    if (runes.pageApplied) {
      applied = `Page « LoL Companion » appliquée${runes.matchup?.applied ? ` · duel contre ${runes.matchup.opponent}` : ""}`;
    } else {
      applied = "Import des runes coupé dans les réglages";
      appliedClass += " off";
    }
  }
  const normal = runes.champion && !runes.pending && !runes.error;
  setKeystone(normal ? runes.keystoneId : 0, title);
  $("runes-rate").replaceChildren(...(rate ? [rate, el("small", "", " %")] : []));
  // Les deux voies : leur icône devant leur nom (l'icône disparaît si le client ne la donne pas).
  const tree = (id, name) => {
    if (!id) return [name];
    const img = el("img", "tree-icon");
    img.alt = "";
    img.onerror = () => img.remove();
    img.src = iconUrl(`perk-${id}`);
    return [img, name];
  };
  $("runes-trees").replaceChildren(
    ...(normal ? [...tree(runes.primaryId, runes.primary), " · ", ...tree(runes.secondaryId, runes.secondary)] : [trees]),
  );
  $("runes-games").textContent = games;
  const line = $("runes-applied");
  line.className = appliedClass;
  line.hidden = !applied;
  line.innerHTML = appliedClass === "applied span" ? CHECK : "";
  line.append(applied);
}

// Rune clé : son icône, avec son nom au survol ; le nom en texte tant que l'icône n'est pas là
// (et pendant l'attente ou la recherche).
function setKeystone(id, title) {
  const icon = $("keystone-icon");
  const text = $("keystone");
  text.textContent = title;
  icon.alt = icon.title = title;
  const url = id ? iconUrl(`perk-${id}`) : "";
  if (!url) {
    icon.hidden = true;
    text.hidden = false;
    delete icon.dataset.url;
    icon.removeAttribute("src");
  } else if (icon.dataset.url !== url && icon.dataset.url !== `${url}?retry`) {
    icon.hidden = true;
    text.hidden = false;
    icon.onload = () => {
      icon.hidden = false;
      text.hidden = true;
    };
    setImage(icon, url);
  }
}

// « Anneau de Doran, Potion de soin ×2 »
const itemNames = (items, separator = ", ") =>
  items.map((it) => (it.count > 1 ? `${it.name} ×${it.count}` : it.name)).join(separator);

function renderBuild(runes) {
  const build = runes.build;
  $("build").hidden = false;
  $("build-empty").hidden = Boolean(build);
  $("build-empty").textContent = !runes.champion
    ? "Le build arrive quand tu verrouilles ton champion."
    : runes.pending
      ? "Recherche sur OP.GG…"
      : "Pas de build trouvé pour ce champion.";
  if (!build) {
    $("build-list").replaceChildren();
    return;
  }
  const [d, f] = runes.spellsApplied;
  const keys = (list) =>
    el(
      "span",
      "keys",
      list.flatMap((key, i) => (i ? [">", el("span", "key", key)] : [el("span", "key", key)])),
    );
  const setTitle = `« LoL Companion · ${runes.champion}${versus(runes)} » `;
  const rows = [
    [
      "SORTS",
      f ? [`D ${d} · F ${f} `, el("span", "badge", "appliqués")] : runes.spells.join(" + "),
      runes.spellsNote,
    ],
    ["DÉPART", itemNames(build.starter)],
    ["CŒUR", itemNames(build.core, " → ")],
    ["BOTTES", itemNames(build.boots)],
    ["ENSUITE", itemNames(build.options, " / ")],
    ["MONTÉE", build.skillMax.length ? keys(build.skillMax) : ""],
    ["NIVEAUX", build.skillOrder.length ? el("span", "mono", build.skillOrder.join(" ")) : ""],
    [
      "SET",
      runes.itemSetOk ? [setTitle, el("span", "badge", "importé")] : runes.itemSetError && el("span", "error-text", runes.itemSetError),
    ],
  ].filter(([, value]) => (Array.isArray(value) ? value.length : value));
  $("build-list").replaceChildren(
    ...rows.map(([label, value, note]) =>
      el("div", "lrow", [
        el("span", "k", label),
        el("div", "", [...(Array.isArray(value) ? value : [value]), note && el("span", "note", note)]),
      ]),
    ),
  );
}

// ---------- En partie ----------

// Valeur des objets d'une équipe ; null tant qu'un prix manque.
function teamValue(team) {
  if (!team.length || team.some((p) => p.itemsValue == null)) return null;
  return team.reduce((sum, p) => sum + p.itemsValue, 0);
}

function renderGame(game) {
  for (const id of ["score", "game-loading", "me-card", "faceoff"]) $(id).hidden = true;
  if (!game) return;
  $("game-loading").hidden = game.ready;
  if (game.ready) {
    renderScore(game);
    renderMe(game);
  }
  renderFaceoff(game);
}

function renderScore(game) {
  $("score").hidden = false;
  const kills = (team) => team.reduce((sum, p) => sum + p.kills, 0);
  $("kills-us").textContent = kills(game.allies);
  $("kills-them").textContent = kills(game.enemies);
  $("clock").textContent = clock(game.gameTime);
  const ours = teamValue(game.allies);
  const theirs = teamValue(game.enemies);
  const gap = ours == null || theirs == null ? null : ours - theirs;
  $("worth-us").textContent = ours == null ? "—" : whole(ours);
  $("worth-them").textContent = theirs == null ? "—" : whole(theirs);
  $("gap-value").textContent = gap == null ? "—" : signed(gap);
  // La case de l'écart prend la couleur de l'équipe qui a le plus d'or en objets.
  $("gold-gap").className = `gold-gap ${gapClass(gap)}`;
}

function renderMe(game) {
  $("me-card").hidden = false;
  setArt($("me-skin"), game.skinId);
  $("me-line").textContent = ["TOI", LANE_NAMES[game.position], `NIVEAU ${game.level}`].filter(Boolean).join(" · ");
  $("me-champion").textContent = upper(game.champion);

  $("kda").textContent = `${game.kills}/${game.deaths}/${game.assists}`;
  // L'API de la partie ne donne les CS que par paliers de 10 (0, 10, 20…) : on affiche la fourchette,
  // et le rythme par minute estimé au milieu de la fourchette. Le compte exact arrive après la partie.
  const minutes = game.gameTime / 60;
  $("cs").textContent = `${game.cs}–${game.cs + 9}`;
  $("cs-label").textContent = game.cs >= 10 && minutes >= 1 ? `CS · ≈ ${decimal((game.cs + 4.5) / minutes)}/min` : "CS";
  $("gold").textContent = whole(game.gold);
}

// Les 10 champions couloir par couloir, avec l'écart de valeur des objets ;
// sans postes (ARAM...), les deux équipes côte à côte. Pendant le chargement : champions et rangs seulement.
function renderFaceoff(game) {
  const { allies, enemies, ready } = game;
  if (!allies.length && !enemies.length) return;
  $("faceoff").hidden = false;
  const laned = (p) => Boolean(LANE_NAMES[p.position]);
  const lanes = allies.some(laned) && enemies.some(laned);
  const myLane = allies.find((p) => p.me)?.position;
  const rows = [];
  if (lanes) {
    for (const [lane, label] of LANES) {
      const ally = allies.find((p) => p.position === lane);
      const enemy = enemies.find((p) => p.position === lane);
      if (!ally && !enemy) continue;
      const gap = ready && ally?.itemsValue != null && enemy?.itemsValue != null ? ally.itemsValue - enemy.itemsValue : null;
      // La case du milieu prend la couleur du joueur qui a le plus d'or en objets dans ce couloir.
      rows.push(
        el("div", "forow", [
          faceCell(ally, ready, false, false),
          el("div", `lane ${gapClass(gap)}`, [el("span", "k", label), el("span", "v", gap == null ? "" : signed(gap))]),
          faceCell(enemy, ready, true, lane === myLane),
        ]),
      );
    }
  }
  // Sans postes, ou joueurs au poste inconnu : côte à côte, sans écart par couloir.
  const rest = (team) => (lanes ? team.filter((p) => !laned(p)) : team);
  const [left, right] = [rest(allies), rest(enemies)];
  for (let i = 0; i < Math.max(left.length, right.length); i++) {
    rows.push(el("div", "forow nolane", [faceCell(left[i], ready, false, false), faceCell(right[i], ready, true, false)]));
  }
  $("faceoff-rows").replaceChildren(...rows);
  $("faceoff-title").textContent = lanes ? "FACE-À-FACE" : "LES ÉQUIPES";
  $("faceoff-aside").textContent = lanes && ready ? "écart d'or par couloir" : "";
  $("faceoff-note").hidden = !(ready && teamValue(allies) != null && teamValue(enemies) != null);
}

// Rang d'un joueur de la partie : emblème et division, « Non classé », « Rang masqué » (mode streamer) ou « — ».
function rankLine(player) {
  const line = el("span", "rank");
  if (player.rankHidden) line.textContent = "Rang masqué";
  else if (!player.rank) line.textContent = "—";
  else if (!player.rank.tier) line.textContent = "Non classé";
  else setRank(line, player.rank, "small", true);
  return line;
}

function faceCell(player, ready, enemy, opponent) {
  const side = enemy ? " enemy" : "";
  if (!player) return el("div", `fcell${side} empty`, [champIcon(0, ""), el("div", "txt", [el("span", "fname", "?")])]);
  const classes = `fcell${side}${player.me ? " me" : ""}${opponent ? " opp" : ""}`;
  return el("div", classes, [
    champIcon(player.championId, player.champion),
    el("div", "txt", [
      el("span", "fname", `${upper(player.champion)}${player.me ? " · TOI" : ""}`),
      ready && el("span", "line", `niv. ${player.level} · ${player.kills}/${player.deaths}/${player.assists}`),
      rankLine(player),
    ]),
  ]);
}

// ---------- Ensemble ----------

function render(state) {
  const found = state.clientFound;
  $("status-text").textContent = !found ? "CLIENT NON DÉTECTÉ" : (PHASES[state.phase] ?? upper(state.phase ?? "CLIENT CONNECTÉ"));
  $("status").className = `status ${!found ? "off" : state.game ? "game" : "on"}`;
  $("hint").hidden = found;

  $("update").hidden = !state.update;
  if (state.update && !$("update-button").disabled) {
    $("update-text").textContent = `Nouvelle version ${state.update} disponible`;
  }

  // Salon : seulement hors sélection et hors partie (la place y sert à autre chose).
  const lobby = found && !state.runes && !state.game;
  renderPlayer(state, lobby);
  renderHistory(state, lobby);
  renderStats(state, lobby);
  renderSelection(state.runes);
  renderGame(state.game);
  layout();
}

// ---------- Taille de la fenêtre ----------

// Taille standard de la fenêtre, comme le client LoL (celle de tauri.conf.json) : le tableau de bord est fait
// pour elle. Les tailles de styles.css sont en pixels de cette fenêtre (avant mise à l'échelle).
const STANDARD = { width: 1280, height: 720 };
// En dessous, le texte deviendrait trop petit : les blocs passent en une seule colonne, qui défile.
const MIN_ZOOM = 0.8;
const ONE_COLUMN = 460;
const EDGE = 12;

// Autre taille que la standard : le même tableau de bord, à l'échelle ; la place en plus (fenêtre plus large
// ou plus haute que 16:9) élargit ou allonge les blocs.
function layout() {
  const width = document.documentElement.clientWidth;
  const height = window.innerHeight;
  const view = !$("player").hidden
    ? "lobby"
    : !$("duel-hero").hidden
      ? "selection"
      : !$("faceoff").hidden || !$("game-loading").hidden
        ? "game"
        : "";
  let zoom = Math.min(width / STANDARD.width, height / STANDARD.height, 2);
  const board = Boolean(view) && zoom >= MIN_ZOOM;
  if (!board) zoom = Math.min(1, Math.max(0.8, width / (ONE_COLUMN + 2 * EDGE)));
  document.body.style.zoom = zoom.toFixed(3);
  document.body.style.setProperty("--vh", `${(height / zoom).toFixed(1)}px`);
  document.body.classList.toggle("board", board);
  document.documentElement.classList.toggle("board", board);
  document.body.dataset.view = view;
  // Dans chaque colonne, le dernier bloc affiché va jusqu'en bas ; une colonne vide ne prend pas de place.
  for (const col of ["col-a", "col-b", "col-c"].map($)) {
    const shown = [...col.children].filter((child) => !child.hidden);
    col.classList.toggle("unused", !shown.length);
    for (const child of col.children) child.classList.toggle("grow", child === shown.at(-1));
  }
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
  layout();
  window.addEventListener("resize", layout);
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
