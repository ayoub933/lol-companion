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
// « vs Sett » quand runes et build viennent du duel.
const versus = (runes) => (runes.matchup?.applied ? ` vs ${runes.matchup.opponent}` : "");

function clock(seconds) {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${String(s).padStart(2, "0")}`;
}

// Victoires en bleu, défaites en orange : « 12–7 ».
const record = (wins, losses, separator = "–") => [el("span", "win", wins), separator, el("span", "loss", losses)];

// « Émeraude II », « Maître · 245 LP », « Non classé ».
function rankLabel(rank) {
  if (!rank) return "";
  if (!rank.tier) return "Non classé";
  const name = TIERS[rank.tier] ?? rank.tier;
  return APEX.includes(rank.tier) ? `${name} · ${rank.lp} LP` : `${name} ${rank.division}`.trim();
}

// « ÉMERAUDE II », « MAÎTRE », « NON CLASSÉ ».
function tierName(rank) {
  if (!rank?.tier) return "NON CLASSÉ";
  const name = TIERS[rank.tier] ?? rank.tier;
  return upper(APEX.includes(rank.tier) ? name : `${name} ${rank.division}`.trim());
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
  $("player").hidden = !show;
  for (const id of ["figures", "form", "moment"]) $(id).hidden = true;
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
  if (!profile) return;
  $("solo-tier").textContent = tierName(profile.solo);
  $("solo-lp").textContent = profile.solo?.tier ? `${profile.solo.lp} LP` : "";
  renderFigures(profile);
  renderForm(profile);
  renderMoment(profile);
}

function renderFigures(profile) {
  $("figures").hidden = false;
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
    $("flex-tier").replaceChildren(el("span", "tier-text", tierName(flex)));
    $("flex-sub").textContent = `flexible · ${flex.lp} LP`;
  } else {
    $("flex-tier").replaceChildren(el("span", "faint", "—"));
    $("flex-sub").textContent = "flexible : non classé";
  }
}

// Une barre par partie, de la plus récente à la plus ancienne : pleine = victoire, creuse = défaite.
function renderForm(profile) {
  const results = profile.recentResults ?? [];
  $("form").hidden = !results.length;
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
  const list = profile.recentChampions;
  $("moment").hidden = !list.length;
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

// Après la partie : tes stats exactes (CS compris), d'après l'historique du client.
function renderLastGame(state, lobby) {
  const last = state.history?.last;
  $("last-game").hidden = !lobby || !(last || state.recording);
  if ($("last-game").hidden) return;
  if (state.recording && !last) {
    $("last-result").textContent = "TERMINÉE";
    $("last-result").className = "result neutral";
    $("last-meta").textContent = "DERNIER MATCH";
    $("last-versus").replaceChildren(el("span", "muted", "Récupération de tes stats dans l'historique du client…"));
    $("last-grid").replaceChildren();
    $("last-when").textContent = "";
    return;
  }
  $("last-result").textContent = last.win ? "VICTOIRE" : "DÉFAITE";
  $("last-result").className = `result ${last.win ? "" : "lost"}`;
  $("last-meta").textContent = ["DERNIER MATCH", QUEUES[last.queueId], clock(last.duration)].filter(Boolean).join(" · ");
  $("last-versus").replaceChildren(
    ...[
      el("span", "champ win", upper(last.champion)),
      last.opponent && el("span", "vs", "VS"),
      last.opponent && el("span", "champ loss", upper(last.opponent)),
      POSITIONS[last.position] && el("span", "lane-name", POSITIONS[last.position]),
    ].filter(Boolean),
  );
  const minutes = last.duration / 60;
  $("last-grid").replaceChildren(
    ...[
      ["KDA", `${last.kills}/${last.deaths}/${last.assists}`],
      ["Ratio KDA", kda(last.kills, last.deaths, last.assists), "win"],
      [`CS · ${decimal(last.cs / Math.max(minutes, 1))}/min`, String(last.cs)],
      ["Or", whole(last.gold)],
      ["Dégâts", whole(last.damage)],
      ["Vision", String(last.vision)],
    ].map(cell),
  );
  // Pendant la récupération de la partie qui vient de finir, la précédente reste affichée.
  $("last-when").textContent = state.recording
    ? "Mise à jour…"
    : new Date(last.date).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
}

// Moyennes par champion (et par duel) sur les parties enregistrées par l'app.
function renderStats(state, lobby) {
  const history = state.history;
  $("stats").hidden = !lobby || !history?.champions.length;
  if ($("stats").hidden) return;
  $("stats-champions").replaceChildren(
    ...history.champions.map((t) => {
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
  $("stats-duels").hidden = !history.duels.length;
  $("stats-duels").replaceChildren(
    el("span", "kicker", "TES DUELS"),
    ...history.duels.map((t) =>
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
  const matchup = runes.matchup;
  const show = runes.laning && runes.championId > 0 && runes.enemies.length > 0 && Boolean(matchup || runes.duelGames);
  $("duel").hidden = !show;
  if (!show) return;
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

// Avant de verrouiller : qui bat ton adversaire, qui va bien avec ton partenaire.
function renderHelp(runes) {
  const help = runes.help;
  $("help").hidden = !help;
  if (!help) return;
  const block = (title, aside, picks) =>
    !title
      ? []
      : [
          el("div", "sub-head", [el("span", "kicker", title), el("span", "aside", aside)]),
          ...picks.map((p) =>
            el("div", "pick", [
              champIcon(p.championId, p.name),
              el("span", "pname", upper(p.name)),
              el("span", "tag", `${number(p.winRate * 100)} %`),
            ]),
          ),
        ];
  $("help-list").replaceChildren(
    ...block(help.opponent && `CONTRE ${upper(help.opponent)}`, "victoires contre lui", help.counters),
    ...block(help.partner && `AVEC ${upper(help.partner)}`, "victoires en duo", help.synergies),
  );
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
  $("keystone").textContent = title;
  $("runes-rate").replaceChildren(...(rate ? [rate, el("small", "", " %")] : []));
  $("runes-trees").textContent = trees;
  $("runes-games").textContent = games;
  const line = $("runes-applied");
  line.className = appliedClass;
  line.hidden = !applied;
  line.innerHTML = appliedClass === "applied span" ? CHECK : "";
  line.append(applied);
}

// « Anneau de Doran, Potion de soin ×2 »
const itemNames = (items, separator = ", ") =>
  items.map((it) => (it.count > 1 ? `${it.name} ×${it.count}` : it.name)).join(separator);

function renderBuild(runes) {
  const build = runes.build;
  $("build").hidden = !build;
  if (!build) return;
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
  for (const id of ["score", "game-loading", "me-card", "me-stats", "items", "faceoff"]) $(id).hidden = true;
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
  $("gap-value").textContent = gap == null ? "—" : signed(gap);
  $("gap-value").className = `v ${gap == null ? "even" : signClass(gap)}`;
}

function renderMe(game) {
  $("me-card").hidden = false;
  setArt($("me-skin"), game.skinId);
  $("me-line").textContent = ["TOI", LANE_NAMES[game.position], `NIVEAU ${game.level}`].filter(Boolean).join(" · ");
  $("me-champion").textContent = upper(game.champion);

  $("me-stats").hidden = false;
  $("kda").textContent = `${game.kills}/${game.deaths}/${game.assists}`;
  // L'API de la partie ne donne les CS que par paliers de 10 (0, 10, 20…) : on affiche la fourchette,
  // et le rythme par minute estimé au milieu de la fourchette. Le compte exact arrive après la partie.
  const minutes = game.gameTime / 60;
  $("cs").textContent = `${game.cs}–${game.cs + 9}`;
  $("cs-label").textContent = game.cs >= 10 && minutes >= 1 ? `CS · ≈ ${decimal((game.cs + 4.5) / minutes)}/min` : "CS";
  $("gold").textContent = whole(game.gold);

  $("items").hidden = !game.items.length;
  $("items").replaceChildren(...game.items.map((name) => el("span", "", name)));
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
      rows.push(
        el("div", "forow", [
          faceCell(ally, ready, false, false),
          el("div", "lane", [
            el("span", "k", label),
            el("span", `v ${gap == null ? "even" : signClass(gap)}`, gap == null ? "" : signed(gap)),
          ]),
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
  $("faceoff-aside").textContent = lanes && ready ? "écart d'objets par couloir" : "";
  $("faceoff-note").hidden = !(ready && teamValue(allies) != null && teamValue(enemies) != null);
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
      el("span", "rank", player.rankHidden ? "Rang masqué" : rankLabel(player.rank) || "—"),
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
  const lobby = !state.runes && !state.game;
  renderPlayer(state, found && lobby);
  renderLastGame(state, lobby);
  renderStats(state, lobby);
  renderSelection(state.runes);
  renderGame(state.game);

  // Fenêtre large : les équipes passent dans une colonne à droite (voir styles.css).
  const split = !$("picks").hidden || !$("faceoff").hidden;
  $("main").classList.toggle("split", split);
  document.body.classList.toggle("split", split);
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
