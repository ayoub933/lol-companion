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
// Si le client était trop occupé pour la donner, on réessaie une fois, 2 secondes plus tard ; toujours rien
// (client fermé) : l'image est cachée, sa case reste.
function setImage(img, url) {
  if (img.dataset.url === url) return;
  img.dataset.url = url;
  img.classList.remove("missing");
  img.onerror = () => {
    img.onerror = () => img.classList.add("missing");
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
// 2 secondes plus tard, le nom du rang en texte à sa place ; `quiet` (le rang est déjà écrit à côté) : rien.
function emblem(tier, size, quiet = false) {
  const name = TIERS[tier] ?? tier;
  const box = el("span", `emblem ${size}`);
  box.title = name;
  const img = document.createElement("img");
  img.alt = name;
  const url = iconUrl(`rank-${tier.toLowerCase()}`);
  img.onerror = () => {
    if (img.dataset.retried && quiet) return box.classList.add("missing");
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
  const line = [tag && `#${upper(tag)}`, state.searched?.region, state.summonerLevel != null && `NIVEAU ${state.summonerLevel}`];
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
  $("solo-rate-sub").title = solo?.recordFromOpgg
    ? "Le client ne donne pas les défaites des autres joueurs : victoires et défaites d'OP.GG (parfois en retard de quelques parties)."
    : "";
  if (solo?.lossesHidden) {
    // Défaites cachées par le client, et OP.GG n'a pas répondu : ses victoires seulement.
    $("solo-rate").replaceChildren(whole(solo.wins), el("small", "", " V"));
    $("solo-rate-sub").textContent = "victoires en solo/duo · défaites inconnues";
  } else if (soloGames) {
    $("solo-rate").replaceChildren(number((solo.wins / soloGames) * 100, 0), el("small", "", " %"));
    const source = solo.recordFromOpgg ? " (OP.GG)" : "";
    $("solo-rate-sub").textContent = `victoires en solo/duo · ${plural(soloGames, "partie")}${source}`;
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

// Une barre par partie, de la plus ancienne (à gauche) à la plus récente (à droite) : pleine = victoire, creuse = défaite.
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
    `Tes ${count} dernières parties, de la plus ancienne à la plus récente : ${plural(wins, "victoire")} et ${plural(count - wins, "défaite")}`,
  );
  // Le profil les donne de la plus récente à la plus ancienne.
  $("form-strip").replaceChildren(...[...results].reverse().map((win) => el("span", win ? "w" : "l")));
}

function renderMoment(profile) {
  const list = profile?.recentChampions ?? [];
  $("moment-empty").hidden = list.length > 0;
  $("moment-empty").textContent = profile ? "Aucune partie récente." : "Lecture de tes dernières parties…";
  $("moment-aside").textContent = list.some((c) => c.meta) ? "tier OP.GG" : "";
  $("moment-cards").replaceChildren(
    ...list.map((c) =>
      el("div", "mcard", [
        champIcon(c.championId, c.name),
        el("span", "mname", upper(c.name)),
        el("span", "games", plural(c.games, "partie")),
        metaLine(c.meta),
        el("span", "rec", record(c.wins, c.games - c.wins, " – ")),
      ]),
    ),
  );
}

const place = (n) => (n === 1 ? "1er" : `${n}e`);
// Sa place dans le classement d'OP.GG : tier (1 = le meilleur, chacun sa couleur), poste, victoires, et places
// gagnées (▲) ou perdues (▼) depuis le patch précédent.
function metaLine(meta) {
  if (!meta) return null;
  const moved = meta.rankPrevPatch > 0 ? meta.rankPrevPatch - meta.rank : null;
  const trend =
    moved == null
      ? null
      : el("span", `trend ${moved > 0 ? "up" : moved < 0 ? "down" : "same"}`, moved > 0 ? `▲${moved}` : moved < 0 ? `▼${-moved}` : "=");
  const line = el("div", "mmeta", [
    el("span", `opgg-tier t${meta.tier}`, `T${meta.tier}`),
    el("span", "mpos", SHORT_POSITIONS[meta.position] ?? ""),
    el("span", "mwr", `${number(meta.winRate * 100, 0)}\u202f%`),
    trend,
  ]);
  const before = meta.rankPrevPatch > 0 ? ` (${place(meta.rankPrevPatch)} au patch précédent)` : "";
  line.title =
    `OP.GG, ${POSITIONS[meta.position] ?? meta.position} : tier ${meta.tier}, ${place(meta.rank)} sur ${meta.ranked}${before}, ` +
    `${number(meta.winRate * 100, 1)} % de victoires`;
  return line;
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
  markOpenGame();
  const shown = [featured, ...rows].filter(Boolean);
  const wins = shown.filter((g) => g.win).length;
  $("history-aside").replaceChildren(
    ...(shown.length ? [`${shown.length > 1 ? `${shown.length} dernières` : "dernière"} · `, ...record(wins, shown.length - wins)] : []),
    ...(state.searched?.source === "opgg" ? [" · OP.GG"] : []),
  );
}

// Après la partie : tes stats exactes (CS compris), d'après l'historique du client.
function renderFeatured(last, recording) {
  $("last-game").hidden = !last && !recording;
  // Cliquable seulement avec une partie à ouvrir.
  delete $("last-game").dataset.game;
  $("last-game").removeAttribute("role");
  $("last-game").removeAttribute("tabindex");
  $("last-game").onclick = $("last-game").onkeydown = null;
  if (last) clickable($("last-game"), last);
  if (!last) {
    if (!recording) return;
    $("last-game").className = "panel last neutral";
    $("last-result").textContent = "TERMINÉE";
    $("last-result").className = "result neutral";
    $("last-meta").textContent = "DERNIER MATCH";
    $("last-lp").replaceChildren();
    $("last-versus").replaceChildren(el("span", "muted", "Récupération de tes stats dans l'historique du client…"));
    $("last-grid").replaceChildren();
    $("last-when").textContent = "";
    return;
  }
  $("last-game").className = `panel last${last.win ? "" : " lost"}`;
  $("last-result").textContent = last.win ? "VICTOIRE" : "DÉFAITE";
  $("last-result").className = `result${last.win ? "" : " lost"}`;
  $("last-lp").replaceChildren(...[lpBadge(last.lpChange, "big")].filter(Boolean));
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
      ["Vision", viewing?.source === "opgg" ? "—" : String(last.vision)],
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
  const row = el("div", `hrow${game.win ? "" : " lost"}`, [
    champIcon(game.championId, game.champion),
    el("div", "hmain", [
      el("span", "hname", [upper(game.champion), game.opponent && el("small", "", ` VS ${upper(game.opponent)}`)]),
      el("span", "hmeta", [el("b", "", game.win ? "VICTOIRE" : "DÉFAITE"), ` · ${queueName(game)}${lane ? ` · ${lane}` : ""}`]),
    ]),
    el("div", "hlp", [lpBadge(game.lpChange)]),
    value(`${game.kills}/${game.deaths}/${game.assists}`, `${kda(game.kills, game.deaths, game.assists)} KDA`),
    value(String(game.cs), `${decimal(game.cs / Math.max(minutes, 1))}/min`),
    value(clock(game.duration), ago(ended(game))),
  ]);
  clickable(row, game);
  return row;
}

// Moyennes par champion, et bilan par duel, sur les parties enregistrées par l'app.
// ---------- Courbe de tes LP ----------

// Repères de division sur la courbe : initiale du rang et division (« É II ») ; le survol donne le nom entier.
const TIER_KEYS = ["IRON", "BRONZE", "SILVER", "GOLD", "PLATINUM", "EMERALD", "DIAMOND"];
const SVG_NS = "http://www.w3.org/2000/svg";
// Dessin en pixels de la carte (sa largeur mesurée, redessiné si elle change) : marge à gauche pour les repères.
const LP_VIEW = { height: 110, left: 34, right: 8, top: 8, bottom: 8 };
let lpDrawn = "";
let lpProfile = null;
new ResizeObserver(() => {
  if (lpProfile && !$("lp").hidden) renderLp(lpProfile, true);
}).observe($("lp-chart"));

function svgEl(tag, attrs) {
  const node = document.createElementNS(SVG_NS, tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  return node;
}

// Repère d'une limite de division (une place de l'échelle classée, 100 par division, Maître dès 2 800).
function lpTick(value) {
  if (value >= 2800) return value === 2800 ? "M" : `M ${value - 2800}`;
  const tier = TIERS[TIER_KEYS[Math.floor(value / 400)]] ?? "";
  return `${tier.charAt(0)} ${["IV", "III", "II", "I"][Math.floor((value % 400) / 100)]}`;
}

const lpRank = (p) => `${TIERS[p.tier] ?? p.tier}${p.division ? ` ${p.division}` : ""} · ${p.lp} LP`;

// Tes LP en solo/duo, partie après partie, retrouvés par l'app (lol.rs, lp_curve) : la courbe des dernières
// classées qu'elle a suivies, avec le total gagné ou perdu. Au survol, la partie et ton rang après elle.
function renderLp(profile, show) {
  $("lp").hidden = !show;
  if (!show) return;
  lpProfile = profile;
  const points = profile?.lpCurve ?? [];
  const games = points.filter((p) => p.gameId);
  $("lp-empty").hidden = games.length > 0;
  $("lp-body").hidden = games.length === 0;
  const width = $("lp-chart").clientWidth;
  // Rien de neuf (ni la courbe ni la largeur) : elle reste telle quelle, survol en cours compris.
  const key = `${width}|${JSON.stringify(points)}`;
  if (key === lpDrawn || !width) return;
  lpDrawn = key;
  if (!games.length) {
    $("lp-chart").replaceChildren();
    return;
  }
  const net = games.reduce((sum, p) => sum + p.change, 0);
  $("lp-net").replaceChildren(net > 0 ? `+${net}` : net < 0 ? `−${-net}` : "±0", el("small", "", " LP"));
  $("lp-sub").textContent = `sur tes ${games.length} ${games.length > 1 ? "dernières classées" : "dernière classée"}`;

  const { height, left, right, top, bottom } = LP_VIEW;
  const values = points.map((p) => p.value);
  const low = Math.floor((Math.min(...values) - 15) / 100) * 100;
  const high = Math.ceil((Math.max(...values) + 15) / 100) * 100;
  const x = (i) => left + (i * (width - left - right)) / (points.length - 1);
  const y = (v) => top + ((high - v) * (height - top - bottom)) / (high - low);
  const svg = svgEl("svg", { viewBox: `0 0 ${width} ${height}`, height, role: "img" });
  svg.setAttribute("aria-label", `Tes LP en solo/duo sur tes ${games.length} dernières classées : ${net > 0 ? "+" : ""}${net} LP`);
  // Une limite de division tous les 100 ; repères un sur deux si la courbe en traverse beaucoup.
  const lines = [];
  for (let v = low; v <= high; v += 100) lines.push(v);
  const every = lines.length > 5 ? 2 : 1;
  lines.forEach((v, i) => {
    svg.append(svgEl("line", { class: "lp-grid", x1: left, x2: width - right, y1: y(v), y2: y(v) }));
    if (i % every === 0) {
      const tick = svgEl("text", { class: "lp-tick", x: left - 6, y: y(v) + 3.5, "text-anchor": "end" });
      tick.textContent = lpTick(v);
      svg.append(tick);
    }
  });
  const path = points.map((p, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(p.value).toFixed(1)}`).join(" ");
  const floor = (height - bottom).toFixed(1);
  svg.append(svgEl("path", { class: "lp-area", d: `${path} L${x(points.length - 1).toFixed(1)},${floor} L${x(0).toFixed(1)},${floor} Z` }));
  svg.append(svgEl("path", { class: "lp-line", d: path }));
  const cross = svgEl("line", { class: "lp-cross", y1: top, y2: height - bottom, visibility: "hidden" });
  svg.append(cross);
  points.forEach((p, i) => {
    const kind = p.gameId ? (p.win ? "win" : "loss") : "start";
    svg.append(svgEl("circle", { class: `lp-dot ${kind}`, cx: x(i), cy: y(p.value), r: p.gameId ? 4 : 3 }));
  });
  // Survol : la ligne verticale va au point le plus proche, la bulle dit la partie et le rang après elle.
  const hit = svgEl("rect", { class: "lp-hit", x: left, y: 0, width: width - left - right, height });
  svg.append(hit);
  const tip = el("div", "lp-tip");
  tip.hidden = true;
  const hover = (event) => {
    const matrix = svg.getScreenCTM();
    if (!matrix) return;
    const at = new DOMPoint(event.clientX, event.clientY).matrixTransform(matrix.inverse());
    const step = (width - left - right) / (points.length - 1);
    const i = Math.max(0, Math.min(points.length - 1, Math.round((at.x - left) / step)));
    const p = points[i];
    cross.setAttribute("x1", x(i));
    cross.setAttribute("x2", x(i));
    cross.setAttribute("visibility", "visible");
    const rows = p.gameId
      ? [
          el("strong", "", `${p.change > 0 ? "+" : p.change < 0 ? "−" : "±"}${Math.abs(p.change)} LP`),
          el("span", "", `${p.win ? "Victoire" : "Défaite"} · ${p.champion}`),
          el("span", "", lpRank(p)),
          el("span", "muted", ago(p.date)),
        ]
      : [el("strong", "", "Départ"), el("span", "", lpRank(p))];
    tip.replaceChildren(...rows);
    tip.hidden = false;
    // Bulle au-dessus du point, sans sortir de la carte.
    const share = x(i) / width;
    tip.style.left = `${share * 100}%`;
    tip.style.transform = `translateX(${share > 0.6 ? "-100%" : share < 0.3 ? "0" : "-50%"})`;
  };
  hit.addEventListener("pointermove", hover);
  hit.addEventListener("pointerleave", () => {
    cross.setAttribute("visibility", "hidden");
    tip.hidden = true;
  });
  $("lp-chart").replaceChildren(svg, tip);
}

function renderStats(state, show) {
  $("stats").hidden = !show;
  if (!show) return;
  const champions = state.history?.champions ?? [];
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
}

// ---------- LP et score carry ----------

// LP gagnés (flèche vers le haut, vert-bleu) ou perdus (vers le bas, rouge-rose) en classée ;
// rien si l'app ne les connaît pas.
function lpBadge(change, size = "") {
  if (change == null) return null;
  const kind = change > 0 ? "up" : change < 0 ? "down" : "even";
  const badge = el("span", `lp-change ${kind} ${size}`.trim(), [String(Math.abs(change)), el("small", "", "LP")]);
  badge.title = change > 0 ? `${change} LP gagnés` : change < 0 ? `${-change} LP perdus` : "LP inchangés";
  return badge;
}

// Médaille du podium du score carry : disque métallique gravé de la place dans une couronne de lauriers,
// et une étoile au-dessus pour le MVP. Or, argent et bronze du podium de la sélection.
// [reflet, couleur, ombre, chiffre]
const MEDAL_COLORS = {
  1: ["#fff6cc", "#f5c451", "#9c7410", "#4a3300"],
  2: ["#ffffff", "#c8d0da", "#6f7a87", "#262b33"],
  3: ["#ffe2c4", "#d68c56", "#7a4219", "#3b1f08"],
};

// Couronne de lauriers (dessin fixe, calculé une fois) : des paires de feuilles le long d'un arc à gauche,
// écartées de la branche, puis la même branche en miroir à droite.
const LAUREL = (() => {
  const leaf = "M0 0C1.6-1.5 1.9-4.2 0-6.6C-1.9-4.2-1.6-1.5 0 0Z";
  const leaves = [];
  for (const deg of [112, 135, 158, 181, 204, 226]) {
    const a = (deg * Math.PI) / 180;
    // Sens de la branche (vers son bout) : la tangente à l'arc.
    const along = (Math.atan2(-Math.sin(a), -Math.cos(a)) * 180) / Math.PI;
    for (const side of [1, -1]) {
      const r = 14 + side * 1.3;
      const x = (20 + r * Math.cos(a)).toFixed(2);
      const y = (21 + r * Math.sin(a)).toFixed(2);
      leaves.push(`<path d="${leaf}" transform="translate(${x} ${y}) rotate(${(along - side * 32).toFixed(1)})"/>`);
    }
  }
  const branch = leaves.join("");
  return `${branch}<g transform="translate(40 0) scale(-1 1)">${branch}</g>`;
})();

// Étoile du MVP, au-dessus du disque.
const STAR = Array.from({ length: 10 }, (_, k) => {
  const r = k % 2 ? 1.7 : 4;
  const a = ((-90 + k * 36) * Math.PI) / 180;
  return `${(20 + r * Math.cos(a)).toFixed(2)},${(4.6 + r * Math.sin(a)).toFixed(2)}`;
}).join(" ");

function medal(place) {
  const [light, main, dark, ink] = MEDAL_COLORS[place];
  const id = `carry-medal-${place}`;
  const node = el("span", `medal m${place}`);
  // Dessin fixe (aucune donnée extérieure).
  node.innerHTML = `<svg viewBox="0 0 40 40" aria-hidden="true">
    <defs><linearGradient id="${id}" x1="0" y1="0" x2=".35" y2="1">
      <stop offset="0" stop-color="${light}"/><stop offset=".5" stop-color="${main}"/><stop offset="1" stop-color="${dark}"/>
    </linearGradient></defs>
    <g fill="url(#${id})" stroke="${dark}" stroke-width=".5">${LAUREL}</g>
    ${place === 1 ? `<polygon points="${STAR}" fill="url(#${id})" stroke="${dark}" stroke-width=".6"/>` : ""}
    <circle cx="20" cy="21" r="9.2" fill="url(#${id})" stroke="${dark}" stroke-width="1.1"/>
    <circle cx="20" cy="21" r="7.2" fill="none" stroke="${light}" stroke-opacity=".8" stroke-width=".8"/>
    <text x="20" y="25.7" text-anchor="middle" fill="${ink}">${place}</text>
  </svg>`;
  node.title = place === 1 ? "Meilleur score carry de la partie" : `${place}e meilleur score carry de la partie`;
  return node;
}

// Score carry d'un joueur (0 à 100) et sa place dans la partie : médaille pour le podium, sinon la place
// dans un rond. Comme dpm.lol : « MVP » sous le score du meilleur de l'équipe gagnante, « ACE » sous
// celui du meilleur de l'équipe perdante.
function carryCell(p, title) {
  const badge = p.place <= 3 ? medal(p.place) : el("span", "medal rank", [el("b", "", String(p.place))]);
  if (p.place > 3) badge.title = `${p.place}e score carry de la partie`;
  const tag = title && el("span", `k ${title.toLowerCase()}`, title);
  if (tag) tag.title = title === "MVP" ? "Meilleur score de l'équipe gagnante" : "Meilleur score de l'équipe perdante";
  return el("div", "mcarry", [badge, el("div", "mpair", [el("span", "v", String(Math.round(p.score))), tag])]);
}

// ---------- Détail d'une partie (salon) ----------

// Partie ouverte en détail ; null = panneau fermé. Une réponse arrivée après un autre clic est ignorée.
let openGame = null;

// « 16,0k » ; sous 1 000, le nombre.
const kilo = (n) => (n >= 1000 ? `${decimal(n / 1000)}k` : String(n));

// Petite icône du client (sort, rune, objet). Introuvable, même 2 secondes plus tard (ex. objet retiré du jeu
// depuis la partie) : une case vide à sa place.
function smallIcon(url, className) {
  const img = el("img", className);
  img.alt = "";
  img.onerror = () => {
    if (img.dataset.retried) return img.replaceWith(el("span", `${className} empty`));
    img.dataset.retried = "1";
    setTimeout(() => (img.src = `${url}?retry`), 2000);
  };
  img.src = url;
  return img;
}

// Clic sur une partie de l'historique : le détail se déroule par-dessus le salon ;
// un nouveau clic sur la même partie le referme.
function openMatch(game) {
  if (openGame?.gameId === game.gameId) return closeMatch();
  openGame = game;
  const panel = $("match");
  panel.hidden = false;
  panel.classList.remove("unroll");
  void panel.offsetWidth; // relance l'animation
  panel.classList.add("unroll");
  $("match-result").textContent = game.win ? "VICTOIRE" : "DÉFAITE";
  $("match-result").className = `result${game.win ? "" : " lost"}`;
  $("match-lp").replaceChildren(...[lpBadge(game.lpChange, "big")].filter(Boolean));
  $("match-title").replaceChildren(
    ...[
      el("span", "ally", upper(game.champion)),
      game.opponent && el("small", "", "VS"),
      game.opponent && el("span", "enemy", upper(game.opponent)),
    ].filter(Boolean),
  );
  const date = new Date(game.date).toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
  $("match-meta").textContent = [queueName(game), clock(game.duration), ago(ended(game)), date].join(" · ");
  $("match-teams").replaceChildren();
  const opgg = viewing?.source === "opgg";
  matchStatus(opgg ? "Lecture de la partie sur OP.GG…" : "Lecture de la partie dans le client…");
  markOpenGame();
  const request = opgg
    ? invoke("searched_match_details", { gameId: game.gameId })
    : invoke("match_details", { gameId: game.gameId, championId: game.championId });
  request
    .then((details) => openGame === game && renderMatch(details, game))
    .catch((error) => openGame === game && matchStatus(String(error)));
}

function closeMatch() {
  openGame = null;
  $("match").hidden = true;
  markOpenGame();
}

function matchStatus(text) {
  $("match-status").hidden = !text;
  $("match-status").textContent = text;
}

// La partie ouverte reste marquée dans l'historique (qui est refait à chaque mise à jour).
function markOpenGame() {
  for (const node of document.querySelectorAll("[data-game]")) {
    node.classList.toggle("open", openGame != null && node.dataset.game === String(openGame.gameId));
  }
}

// Rend une partie de l'historique cliquable (souris ou clavier).
function clickable(node, game) {
  node.dataset.game = game.gameId;
  node.tabIndex = 0;
  node.setAttribute("role", "button");
  node.onclick = () => openMatch(game);
  node.onkeydown = (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      openMatch(game);
    }
  };
}

// Les deux équipes côte à côte ; les barres de dégâts se comparent sur toute la partie.
function renderMatch(details, game) {
  matchStatus("");
  const players = details.teams.flatMap((team) => team.players);
  const most = Math.max(1, ...players.map((p) => p.damage));
  $("match-teams").replaceChildren(...details.teams.map((team, i) => matchTeam(team, i === 0, most, game.duration)));
}

function matchTeam(team, ours, most, duration) {
  const sum = (key) => team.players.reduce((total, p) => total + p[key], 0);
  const best = Math.min(...team.players.map((p) => p.place));
  const score = viewing?.source === "opgg" ? "OP.GG" : "CARRY";
  const labels = ["JOUEUR", score, "KDA", "DÉGÂTS", "OR", "CS", "BALISES", "OBJETS"].map((text) => el("span", "", text));
  return el("div", `mteam ${ours ? "us" : "them"}`, [
    el("div", "mteam-head", [
      el("span", "side", ours ? (viewing ? "SON ÉQUIPE" : "TON ÉQUIPE") : "EN FACE"),
      el("span", `res ${team.win ? "win" : "loss"}`, team.win ? "VICTOIRE" : "DÉFAITE"),
      el("span", "tot", `${sum("kills")} / ${sum("deaths")} / ${sum("assists")} · ${kilo(sum("gold"))} d'or`),
    ]),
    el("div", "mrow labels", labels),
    ...team.players.map((p) => matchRow(p, most, duration, p.place === best ? (team.win ? "MVP" : "ACE") : null)),
  ]);
}

// Un joueur : champion et niveau, sorts, runes, nom et rang, KDA, dégâts, or, CS, balises, objets.
function matchRow(p, most, duration, title) {
  const champion = el("div", "mchamp", [champIcon(p.championId, ""), el("span", "lvl", p.level)]);
  const spells = el("div", "mstack", p.spells.map((id) => smallIcon(iconUrl(`spell-${id}`), "micon")));
  const runes = el("div", "mstack", [
    smallIcon(iconUrl(`perk-${p.keystone}`), "micon"),
    smallIcon(iconUrl(`perk-${p.subStyle}`), "micon sub"),
  ]);
  const rank = el("span", "mrank");
  if (p.rank) setRank(rank, p.rank, "small");
  const you = p.me && !viewing;
  const who = el("div", "mwho", [el("span", "mname", you ? [p.name || "?", el("small", "", " · TOI")] : p.name || "?"), rank]);
  who.title = p.tag ? `${p.name}#${p.tag}` : p.name;
  // Ses parties sont dans la région de ton client, ou dans celle du joueur cherché.
  if (!p.me) profileLink(who, p.tag ? `${p.name}#${p.tag}` : p.name, p.puuid, viewing?.region ?? "");
  const fill = el("span");
  fill.style.width = `${(p.damage / most) * 100}%`;
  const minutes = Math.max(duration / 60, 1);
  return el("div", `mrow${p.me ? " me" : ""}${p.place <= 3 ? ` p${p.place}` : ""}`, [
    el("div", "mcell-group", [champion, spells, runes, who]),
    carryCell(p, title),
    el("div", "mpair", [el("span", "v", `${p.kills}/${p.deaths}/${p.assists}`), el("span", "k", `${kda(p.kills, p.deaths, p.assists)} KDA`)]),
    el("div", "mpair", [el("span", "v", whole(p.damage)), el("span", "mbar", [fill])]),
    el("span", "mnum", kilo(p.gold)),
    el("div", "mpair", [el("span", "v", String(p.cs)), el("span", "k", `${decimal(p.cs / minutes)}/min`)]),
    el("span", "mnum", String(p.wards)),
    el("div", "mitems", p.items.map((id) => (id ? smallIcon(iconUrl(`item-${id}`), "mitem") : el("span", "mitem empty")))),
  ]);
}

// ---------- Sélection des champions ----------

function renderSelection(runes) {
  for (const id of ["duel-hero", "duel", "bans", "picks", "help", "runes", "build"]) $(id).hidden = true;
  if (!runes) return;
  renderDuelHero(runes);
  renderDuel(runes);
  renderBans(runes);
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
  const locked = Boolean(runes.champion) && !runes.preview;
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

// Tant que ton ban n'est pas fait : qui bannir. D'abord ceux qui battent ton champion (survolé ou choisi),
// puis les mieux classés de ton poste sur OP.GG.
function renderBans(runes) {
  const help = runes.bans;
  $("bans").hidden = !help;
  if (!help) return;
  const position = SHORT_POSITIONS[help.position] ?? "";
  $("ban-list").replaceChildren(
    ...help.bans.map((b) => {
      const why = b.counter
        ? `BAT ${upper(help.champion)}`
        : [el("span", `opgg-tier t${b.tier}`, `T${b.tier}`), el("span", "", position)];
      const card = el("div", `ban${b.counter ? " counter" : ""}`, [
        champIcon(b.championId, b.name),
        el("span", "bname", upper(b.name)),
        el("span", "bwhy", why),
        el("span", "bpct", [number(b.winRate * 100, 0), el("small", "", " %")]),
      ]);
      const games = b.games ? ` (${whole(b.games)} parties)` : "";
      card.title = b.counter
        ? `${b.name} : ${number(b.winRate * 100, 1)} % de victoires contre ${help.champion}${games}`
        : `${b.name} : tier ${b.tier} à ton poste sur OP.GG, ${number(b.winRate * 100, 1)} % de victoires${games}`;
      return card;
    }),
  );
  $("bans-pending").textContent = help.pending
    ? "Recherche sur OP.GG…"
    : !help.bans.length
      ? "Pas de suggestion d'OP.GG ici."
      : !help.champion
        ? "Survole ton champion : ceux qui le battent passent en premier."
        : "";
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

  const enemyRow = (enemy) => {
    const isOpponent = enemy.id === opponentId;
    const row = el(laning ? "button" : "div", `pick enemy${isOpponent ? " opponent" : ""}`, [
      champIcon(enemy.id, enemy.name),
      el("span", "pname", upper(enemy.name)),
      el("span", "tag", isOpponent ? (manual ? "CHOISI" : "DEVINÉ") : ""),
    ]);
    if (laning) {
      row.type = "button";
      if (enemy.position) {
        row.title = `Poste deviné d'après OP.GG : ${Math.round(enemy.positionRate * 100)} % de ses parties à ce poste`;
      }
      // Clic : cet adversaire ; reclic sur celui choisi : retour à la devinette.
      row.addEventListener("click", () =>
        invoke("set_opponent", { championId: manual && isOpponent ? null : enemy.id }),
      );
    }
    return row;
  };
  const emptySlot = () => el("div", "pick enemy empty", [champIcon(0, ""), el("span", "pname", "…")]);
  let rows = enemies.map(enemyRow);
  if (laning) {
    // Face à face : chaque adversaire en face de l'allié du même poste (poste deviné d'après OP.GG) ; une case
    // vide en face d'un poste encore sans adversaire, puis ceux sans poste dans les cases restantes.
    const placed = new Set();
    rows = allies.map((ally) => {
      const enemy = enemies.find((e) => e.position && e.position === ally.position && !placed.has(e.id));
      if (!enemy) return null;
      placed.add(enemy.id);
      return enemyRow(enemy);
    });
    const rest = enemies.filter((e) => !placed.has(e.id)).map(enemyRow);
    rows = rows.map((row) => row ?? rest.shift() ?? emptySlot()).concat(rest);
  }
  if (!rows.length) rows.push(el("div", "note", "Choix cachés dans ce mode"));
  $("pick-enemies").replaceChildren(...rows);
  renderDamage(runes);
}

// Dégâts des deux équipes : barre physique (ambre) / magique (violet), tanks, et un conseil quand une équipe n'a
// presque qu'un type de dégâts. Les tiens survolés comptent : on voit l'effet d'un choix avant de le verrouiller.
function renderDamage(runes) {
  const { allyDamage: ours, enemyDamage: theirs } = runes;
  const box = $("picks-damage");
  box.hidden = !ours?.champions && !theirs?.champions;
  if (box.hidden) return;
  const side = (team, title, advice) => {
    if (!team.champions) return el("div", "dmg empty", [el("span", "kicker", title), el("span", "muted small", "…")]);
    const total = team.physical + team.magic || 1;
    const ad = el("span", "ad");
    ad.style.width = `${(team.physical / total) * 100}%`;
    const tanks = team.tanks ? ` · ${team.tanks} tank${team.tanks > 1 ? "s" : ""}` : "";
    return el("div", "dmg", [
      el("span", "kicker", title),
      el("span", "dbar", [ad]),
      el("span", "dtext", `${number(team.physical)} AD · ${number(team.magic)} AP${tanks}`),
      advice && el("span", "dtip", advice),
    ]);
  };
  // Ton équipe : il manque un type de dégâts (dès 3 champions). En face : presque que de l'AD ou de l'AP.
  const ourAdvice =
    ours.champions >= 3 && ours.magic <= 1
      ? "peu de dégâts magiques : un champion AP aiderait"
      : ours.champions >= 3 && ours.physical <= 1
        ? "peu de dégâts physiques : un champion AD aiderait"
        : "";
  const theirAdvice =
    theirs.physical >= 3.5 ? "presque que de l'AD : l'armure sera rentable" : theirs.magic >= 3.5 ? "presque que de l'AP : la résistance magique sera rentable" : "";
  box.replaceChildren(side(ours, "TON ÉQUIPE", ourAdvice), side(theirs, "EN FACE", theirAdvice));
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
    trees = "Clique un champion pour voir ses runes ; verrouille-le pour les importer.";
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
    if (runes.preview) {
      applied = "Aperçu : verrouille ce champion pour les mettre dans ta page";
      appliedClass += " off";
    } else if (runes.pageApplied) {
      // Avec la liste des pages du duel, le duel se voit déjà : une ligne suffit.
      const duel = runes.matchup?.applied && (runes.pages ?? []).length < 2;
      applied = `Mises dans ta page sélectionnée${duel ? ` · duel contre ${runes.matchup.opponent}` : ""}`;
    } else {
      applied = runes.pageNote ?? "Import des runes coupé dans les réglages";
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
  // Plusieurs pages proposées : leur liste remplace la rune clé, son % et ses voies (la page mise y est en bleu).
  const listed = renderPages(runes, normal);
  for (const id of ["rune-main", "runes-rate", "runes-trees", "runes-games"]) $(id).hidden = listed;
  renderForms(runes);
  const line = $("runes-applied");
  line.className = appliedClass;
  line.hidden = !applied;
  line.innerHTML = appliedClass === "applied span" ? CHECK : "";
  line.append(applied);
}

// Pages de runes proposées pour le duel : la plus jouée puis jusqu'à 2 autres qui changent au moins une rune,
// chacune avec son % de victoires à droite. Un clic en change : elle est mise dans ta page si ton champion est
// verrouillé. Une seule page sans duel (OP.GG n'en donne qu'une hors duel) : pas de liste, une note.
// Renvoie true si la liste est affichée.
function renderPages(runes, normal) {
  const box = $("rune-pages");
  const pages = normal ? (runes.pages ?? []) : [];
  box.hidden = pages.length < 2;
  const note = $("rune-pages-note");
  note.textContent = !normal || pages.length > 1 || !runes.laning
    ? ""
    : runes.matchup?.applied
      ? `Une seule page assez jouée contre ${runes.matchup.opponent}`
      : runes.matchup
        ? ""
        : "Autres pages : avec ton adversaire direct";
  note.hidden = !note.textContent;
  // Rien à refaire si les pages et celle choisie n'ont pas changé (les icônes ne sont pas rechargées).
  // (La note du client compte : page refusée, la liste revient sur celle qui est mise.)
  const key = `${pages.map((p) => `${p.perks.join(".")}:${p.games}:${p.wins}`).join("|")}#${runes.page}#${runes.pageNote ?? ""}`;
  if (box.dataset.key !== key) {
    box.dataset.key = key;
    box.replaceChildren(...(pages.length > 1 ? pages.map((page, i) => pageRow(page, i === runes.page)) : []));
  }
  return !box.hidden;
}

function pageRow(page, on) {
  const row = el("button", `rpage${on ? " on" : ""}`);
  row.type = "button";
  row.title = `${page.keystone} · ${page.primary} / ${page.secondary}`;
  // Les 4 runes principales, puis les 2 secondaires ; le nom de chacune au survol.
  const icons = el("span", "ricons");
  page.perks.slice(0, 6).forEach((id, i) => {
    if (i === 4) icons.append(el("span", "rsep"));
    const img = el("img", i === 0 ? "ricon keystone-rune" : "ricon");
    img.alt = "";
    img.title = page.runeNames[i] ?? "";
    setImage(img, iconUrl(`perk-${id}`));
    icons.append(img);
  });
  const rate = page.games > 0 ? number((page.wins / page.games) * 100, 1) : "–";
  row.append(
    icons,
    el("span", "rstat", [el("span", "rpct", [rate, el("small", "", " %")]), el("span", "rgames", `${whole(page.games)} parties`)]),
  );
  row.addEventListener("click", () => {
    if (row.classList.contains("on")) return;
    for (const other of row.parentNode.children) other.classList.toggle("on", other === row);
    invoke("set_rune_page", { perks: page.perks });
  });
  return row;
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

// Kayn : sa forme, rouge ou bleue, décide de ses runes et de son build. Conseillée d'après l'équipe d'en face (les
// mêlées donnent les orbes rouges, les champions à distance les bleues) ; un clic la choisit, un reclic revient à
// la forme conseillée.
// « Kayn rouge » (titre du set d'objets), « forme bleue » (bloc TON BUILD).
const KAYN_FORMS = { red: "rouge", blue: "bleu" };
const FORMS = { red: "forme rouge", blue: "forme bleue" };
function renderForms(runes) {
  const box = $("forms");
  box.hidden = !runes.forms;
  if (!runes.forms) return;
  box.dataset.manual = runes.formManual ? runes.form : "";
  for (const button of box.querySelectorAll("button")) button.classList.toggle("on", button.dataset.form === runes.form);
  const { enemyMelee: melee, enemyRanged: ranged } = runes;
  const comp = `${melee} mêlée${melee > 1 ? "s" : ""} et ${ranged} à distance en face`;
  $("forms-note").textContent = runes.formManual
    ? "choisie · reclique pour la forme conseillée"
    : runes.form
      ? `conseillée : ${comp}`
      : melee + ranged
        ? `à choisir : ${comp}`
        : "à choisir, ou conseillée selon l'équipe d'en face";
}

// « Anneau de Doran, Potion de soin ×2 »
// Icônes du build de la sélection : objets (nombre dans le coin), entre eux une flèche (ordre d'achat) ou une
// barre (au choix) ; le nom au survol.
const itemIcons = (items, separator = "") =>
  el(
    "span",
    "bicons",
    items.flatMap((item, i) => [i && separator ? el("span", "bsep", separator) : null, buildIcon(item, "mid")].filter(Boolean)),
  );

// Sort d'invocateur ou compétence du champion : son icône, sa touche dans le coin (D, F, Q…), son nom au survol.
function keyIcon(url, key, name) {
  const box = el("span", "gb-icon mid", [smallIcon(url, "gb-img")]);
  if (key) box.append(el("span", "count tag", key));
  box.title = name;
  return box;
}

function renderBuild(runes) {
  const build = runes.build;
  $("build").hidden = false;
  $("build-empty").hidden = Boolean(build);
  $("build-empty").textContent = !runes.champion
    ? "Clique un champion pour voir son build."
    : runes.pending
      ? "Recherche sur OP.GG…"
      : "Pas de build trouvé pour ce champion.";
  if (!build) {
    $("build-list").replaceChildren();
    return;
  }
  // Sorts mis dans le client (sur D puis F), sinon ceux conseillés.
  const applied = runes.appliedSpells?.length > 0;
  const spellIds = applied ? runes.appliedSpells : build.spells;
  const spellNames = applied ? runes.spellsApplied : runes.spells;
  const spells = el(
    "span",
    "bicons",
    spellIds.map((id, i) => keyIcon(iconUrl(`spell-${id}`), applied ? ["D", "F"][i] : "", spellNames[i] ?? "")),
  );
  // Ordre de montée des compétences, avec leurs icônes.
  const abilities = (list) =>
    el(
      "span",
      "bicons",
      list.flatMap((key, i) => [
        i ? el("span", "bsep", ">") : null,
        keyIcon(iconUrl(`ability-${runes.championId}-${key.toLowerCase()}`), key, key),
      ].filter(Boolean)),
    );
  const form = runes.form ? ` ${KAYN_FORMS[runes.form]}` : "";
  // Le set d'objets importé, sur une ligne : son nom sans « LoL Companion · » (en entier au survol).
  const setName = `${runes.champion}${form}${versus(runes)}`;
  const setLine = el("span", "set-line", [`« ${setName} » `, el("span", "badge", "importé")]);
  setLine.title = `Set « LoL Companion · ${setName} » importé dans le client`;
  // La compétence prise à chaque niveau, sur une ligne (l'ultime en bleu).
  const levels = el(
    "span",
    "levels",
    build.skillOrder.map((key, i) => {
      const level = el("span", key === "R" ? "lv ult" : "lv", key);
      level.title = `Niveau ${i + 1} : ${key}`;
      return level;
    }),
  );
  const rows = [
    ["SORTS", spellIds.length ? [spells, applied ? el("span", "badge", "appliqués") : null].filter(Boolean) : "", runes.spellsNote],
    ["DÉPART", build.starter.length ? itemIcons(build.starter) : ""],
    ["CŒUR", build.core.length ? itemIcons(build.core, "→") : ""],
    ["BOTTES", build.boots.length ? itemIcons(build.boots) : ""],
    ["ENSUITE", build.options.length ? itemIcons(build.options, "/") : ""],
    ["MONTÉE", build.skillMax.length ? abilities(build.skillMax) : ""],
    ["NIVEAUX", build.skillOrder.length ? levels : ""],
    ["SET", runes.itemSetOk ? setLine : runes.itemSetError && el("span", "error-text", runes.itemSetError)],
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
  for (const id of ["score", "game-loading", "me-card", "game-build", "faceoff"]) $(id).hidden = true;
  if (!game) return;
  $("game-loading").hidden = game.ready;
  if (game.ready) {
    renderScore(game);
    renderMe(game);
    renderGameBuild(game);
  }
  renderFaceoff(game);
}

// Ton build en détail, sous ta carte : le prochain achat avec l'or qu'il te manque, l'ordre des achats, la suite
// avec les % de victoires OP.GG, les objets contre l'équipe d'en face et la montée des compétences.
function renderGameBuild(game) {
  $("game-build").hidden = false;
  $("gb-empty").hidden = Boolean(game.build);
  $("gb-body").hidden = !game.build;
  if (game.build) fillBuild($("gb-body"), $("gb-aside"), game);
}

// Le build dans `body` (et son titre dans `aside`), refait seulement quand il change ; l'or en poche, lui, change
// chaque seconde : juste la barre et le texte. Sert à l'écran de partie et à l'app par-dessus le jeu.
function fillBuild(body, aside, game) {
  const build = game.build;
  const key = JSON.stringify([game.panel, build]);
  if (body.dataset.key !== key) {
    body.dataset.key = key;
    const form = build.form ? ` · ${FORMS[build.form]}` : "";
    aside.textContent = `OP.GG${build.versus ? ` · vs ${build.versus}` : ""}${form}`;
    const me = game.allies.find((p) => p.me);
    body.replaceChildren(...buildBlocks(game.panel, build, me?.championId ?? 0));
  }
  const next = build.next;
  const gold = body.querySelector(".gb-gold");
  if (!next || !gold) return;
  const missing = next.left - game.gold;
  gold.querySelector(".fill").style.width = `${next.left ? Math.min(100, (game.gold / next.left) * 100) : 100}%`;
  gold.querySelector(".text").textContent =
    missing > 0 ? `Il te manque ${whole(missing)} or` : "Tu as de quoi l'acheter";
  gold.classList.toggle("ready", missing <= 0);
}

// Icône d'un objet du build : grisée avec une coche si tu l'as ; au survol, son nom et son prix.
function buildIcon(item, size, under) {
  const box = el("span", `gb-icon ${size}${item.owned ? " owned" : ""}`, [smallIcon(iconUrl(`item-${item.id}`), "gb-img")]);
  if (item.count > 1) box.append(el("span", "count", String(item.count)));
  box.title = [item.name, item.price ? `${whole(item.price)} or` : "", item.owned ? "déjà à toi" : ""]
    .filter(Boolean)
    .join(" · ");
  return under == null ? box : el("span", "gb-under", [box, el("span", "gb-note", under)]);
}

function buildBlocks(panel, build, championId) {
  const blocks = [];
  const next = build.next;
  if (next) {
    const names = next.items.map((it) => (it.count > 1 ? `${it.name} ×${it.count}` : it.name)).join(", ");
    const gold = el("div", "gb-gold", [el("span", "bar", [el("span", "fill")]), el("span", "text")]);
    const parts = next.components.length
      ? el("div", "gb-parts", next.components.map((c) => buildIcon(c, "part")))
      : null;
    blocks.push(
      el("div", "gb-next", [
        el("span", "gb-k", "PROCHAIN ACHAT"),
        el("div", "gb-next-main", [
          el("div", "gb-next-icons", next.items.map((it) => buildIcon(it, "big"))),
          el("div", "gb-next-text", [
            el("div", "gb-name", [el("span", "", names), el("span", "gb-price", `${whole(next.price)} or`)]),
            gold,
          ]),
          parts,
        ]),
      ]),
    );
  } else {
    blocks.push(el("p", "muted small", "Build principal acheté : la suite et les objets contre eux sont en dessous."));
  }
  // L'ordre des achats : départ › 1er objet (son autre choix en petit) › bottes › 2e › 3e.
  const path = panel.filter((row) => /^\d/.test(row.label));
  const steps = path.flatMap((row, i) => {
    const icons = row.items.map((it) => buildIcon(it, it.size === "alt" ? "alt" : "step"));
    const step = el("span", `gb-step${row.done ? " done" : ""}${row.next ? " next" : ""}`, icons);
    return i ? [el("span", "gb-arrow", "›"), step] : [step];
  });
  if (steps.length) blocks.push(el("div", "gb-row", [el("span", "gb-k", "ORDRE"), el("div", "gb-path", steps)]));
  // Sets d'objets du duel (les 3 objets principaux) : le classique, puis 2 autres, avec leur % de victoires sur
  // OP.GG ; celui de l'ordre des achats en bleu. Seulement ici, pas sur le panneau posé sur le jeu.
  const sets = build.sets ?? [];
  if (sets.length > 1) {
    const rows = sets.map((set) => {
      const box = el("div", `gb-set${set.current ? " on" : ""}`, [
        el("span", "gb-set-icons", set.items.map((it) => buildIcon(it, "set"))),
        el("span", "gb-set-rate", [
          set.games ? number((set.wins / set.games) * 100, 1) : "–",
          el("small", "", " %"),
          el("span", "gb-set-games", ` · ${whole(set.games)}`),
        ]),
      ]);
      box.title = `${set.items.map((it) => it.name).join(" → ")} : ${whole(set.games)} parties${set.current ? " (celui de l'ordre)" : ""}`;
      return box;
    });
    blocks.push(el("div", "gb-row", [el("span", "gb-k", "SETS D'OBJETS"), el("div", "gb-sets", rows)]));
  }
  // La suite, les objets contre l'équipe d'en face et la montée des compétences, côte à côte.
  const rate = (it) => (it.games ? `${Math.round((it.wins / it.games) * 100)} %` : "");
  const more = panel.find((row) => row.label === "+");
  const versus = panel.find((row) => row.label === "VS");
  const halves = [];
  if (more) halves.push(el("div", "gb-half", [el("span", "gb-k", "ENSUITE"), el("div", "gb-icons", more.items.map((it) => buildIcon(it, "mid", rate(it))))]));
  if (versus) {
    const icons = versus.items.map((it) => buildIcon(it, "mid", it.reason.toUpperCase()));
    halves.push(el("div", "gb-half", [el("span", "gb-k", "CONTRE EUX"), el("div", "gb-icons", icons)]));
  }
  if (build.skills.length) {
    // Les icônes de ses compétences (la touche dans le coin), sinon juste les touches.
    const skill = (key) => (championId > 0 ? keyIcon(iconUrl(`ability-${championId}-${key.toLowerCase()}`), key, key) : el("span", "key", key));
    const keys = build.skills.flatMap((key, i) => (i ? [el("span", "gb-arrow", "›"), skill(key)] : [skill(key)]));
    halves.push(el("div", "gb-half gb-climb", [el("span", "gb-k", "MONTÉE"), el("div", "gb-skills", keys)]));
  }
  if (halves.length) blocks.push(el("div", "gb-halves", halves));
  if (build.enemyNote) blocks.push(el("div", "gb-foot", [el("span", "gb-enemy", `En face : ${build.enemyNote}`)]));
  // Le conseil d'OP.GG contre ton adversaire de couloir (en anglais : OP.GG ne le traduit pas), sur 2 lignes au
  // plus ; en entier au survol.
  const tip = build.tip;
  if (tip?.text) {
    const style = PLAY_STYLES[tip.playStyle] ?? "";
    const title = `ASTUCE CONTRE ${upper(tip.versus)}${style ? ` · ${upper(style)}` : ""}`;
    const block = el("div", "gb-tip", [el("span", "gb-k", title), el("p", "", `« ${tip.text} »`)]);
    block.title = tip.text;
    blocks.push(block);
  }
  return blocks;
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
  // Joueurs venus avec chacun (duo, groupe) : leurs champions.
  const everyone = [...allies, ...enemies];
  const mates = (p) => (p?.group ? everyone.filter((q) => q !== p && q.group === p.group).map((q) => q.champion) : []);
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
          faceCell(ally, ready, false, false, mates(ally)),
          el("div", `lane ${gapClass(gap)}`, [el("span", "k", label), el("span", "v", gap == null ? "" : signed(gap))]),
          faceCell(enemy, ready, true, lane === myLane, mates(enemy)),
        ]),
      );
    }
  }
  // Sans postes, ou joueurs au poste inconnu : côte à côte, sans écart par couloir.
  const rest = (team) => (lanes ? team.filter((p) => !laned(p)) : team);
  const [left, right] = [rest(allies), rest(enemies)];
  for (let i = 0; i < Math.max(left.length, right.length); i++) {
    rows.push(
      el("div", "forow nolane", [
        faceCell(left[i], ready, false, false, mates(left[i])),
        faceCell(right[i], ready, true, false, mates(right[i])),
      ]),
    );
  }
  $("faceoff-rows").replaceChildren(...rows);
  $("faceoff-title").textContent = lanes ? "FACE-À-FACE" : "LES ÉQUIPES";
  $("faceoff-aside").textContent = lanes && ready ? "écart d'or par couloir" : "";
  $("faceoff-note").hidden = !(ready && teamValue(allies) != null && teamValue(enemies) != null);
}

// Rang d'un joueur de la partie : emblème et division, « Non classé », « Rang masqué » (mode streamer) ou « — ».
// Fiche rapide d'après ses dernières parties (lues pendant le chargement) : sa série en cours, à partir de 3 d'affilée,
// et ses parties sur son champion, avec le % à partir de 3 parties (en dessous, il ne veut rien dire).
function recentLine(player) {
  const recent = player.recent;
  if (!recent?.games) return null;
  const { games, streak, championGames: on, championWins: wins } = recent;
  const parts = [];
  if (Math.abs(streak) >= 3) {
    const won = streak > 0;
    parts.push(el("span", `streak ${won ? "win" : "loss"}`, `${Math.abs(streak)} ${won ? "V" : "D"} d'affilée`));
  }
  const rate = on >= 3 ? ` · ${Math.round((wins / on) * 100)} %` : "";
  parts.push(el("span", "", on ? `${player.champion} : ${on} sur ${games}${rate}` : `${player.champion} : 0 sur ses ${games} dernières`));
  const line = el("span", "recent", parts);
  line.title = `D'après ses ${games} dernières parties (toutes files, sans les parties annulées)`;
  return line;
}

function rankLine(player) {
  const line = el("span", "rank");
  if (player.rankHidden) line.textContent = "Rang masqué";
  else if (!player.rank) line.textContent = "—";
  else if (!player.rank.tier) line.textContent = "Non classé";
  else setRank(line, player.rank, "small", true);
  return line;
}

// Maillon des joueurs venus ensemble (duo, groupe), une couleur par groupe. Dessin fixe (aucune donnée extérieure).
const LINK = `<svg viewBox="0 0 16 16" aria-hidden="true">
  <path d="M6.6 9.4l2.8-2.8M7.3 4.6l1.2-1.2a2.6 2.6 0 0 1 3.7 3.7L11 8.3M8.7 11.4l-1.2 1.2a2.6 2.6 0 0 1-3.7-3.7L5 7.7"/>
</svg>`;

function groupMark(player, mates) {
  if (!player.group || !mates.length) return null;
  const node = el("span", `link g${((player.group - 1) % 4) + 1}`);
  node.innerHTML = LINK;
  const names = mates.length > 1 ? `${mates.slice(0, -1).join(", ")} et ${mates.at(-1)}` : mates[0];
  node.title = `${mates.length > 1 ? "En groupe" : "En duo"} avec ${names} : au moins 2 parties ensemble parmi leurs 20 dernières`;
  return node;
}

// Joueur déjà croisé (allié ou adversaire) dans une de tes 3 dernières parties. Dessin fixe (aucune donnée extérieure).
const HISTORY = `<svg viewBox="0 0 24 24" aria-hidden="true">
  <path d="M4 12a8 8 0 1 0 2.3-5.7L4 8.6"/><path d="M4 4.2v4.4h4.4"/><path d="M12 7.5v4.5l3 1.8"/>
</svg>`;

function metMark(player) {
  const met = player.met;
  if (!met) return null;
  const node = el("span", "met");
  node.innerHTML = HISTORY;
  const when = ["à ta dernière partie", "à ton avant-dernière partie", "il y a 3 parties"][met.ago - 1] ?? "récemment";
  node.title = `Déjà croisé ${when} : il jouait ${met.champion} ${met.ally ? "dans ton équipe" : "contre toi"}`;
  return node;
}

// Smurf probable : hexagone rouge avec un loup (masque). Dessin fixe (aucune donnée extérieure).
const MASK = `<svg viewBox="0 0 24 24" aria-hidden="true">
  <polygon class="hex" points="12,1.5 21.6,7 21.6,17 12,22.5 2.4,17 2.4,7"/>
  <path class="mask" d="M4.6 10.2c2.2-1.7 5-1.9 7.4-.5 2.4-1.4 5.2-1.2 7.4.5.5 2.6-.8 4.9-3 5.2-1.8.3-3-.9-4.4-.9s-2.6 1.2-4.4.9c-2.2-.3-3.5-2.6-3-5.2z"/>
  <ellipse class="eye" cx="8.6" cy="11.6" rx="1.6" ry="1.1"/><ellipse class="eye" cx="15.4" cy="11.6" rx="1.6" ry="1.1"/>
</svg>`;

function smurfMark(player) {
  if (!player.smurf || !player.rank) return null;
  const node = el("span", "smurf");
  node.innerHTML = MASK;
  const level = player.accountLevel ? `, compte niveau ${player.accountLevel}` : "";
  const bilan = (wins, losses) => `${wins} V – ${losses} D (${Math.round((wins / Math.max(wins + losses, 1)) * 100)} %)`;
  const { rank, recent } = player;
  // Défaites de la saison cachées par le client : le bilan de ses dernières classées.
  node.title =
    rank.lossesHidden && recent
      ? `Smurf probable : ${bilan(recent.rankedWins, recent.rankedLosses)} sur ses dernières classées, ` +
        `${rank.wins} victoires cette saison${level}`
      : `Smurf probable : ${bilan(rank.wins, rank.losses)} en classée cette saison${level}`;
  return node;
}

// OTP : il joue surtout ce champion (au moins 60 % de ses dernières parties). Pastille « OTP », vert vif.
function otpMark(player) {
  const recent = player.recent;
  if (!recent?.otp) return null;
  const node = el("span", "otp", "OTP");
  const rate = recent.championGames >= 3 ? ` (${Math.round((recent.championWins / recent.championGames) * 100)} % de victoires)` : "";
  node.title = `OTP : ${recent.championGames} de ses ${recent.games} dernières parties sur ${player.champion}${rate}`;
  return node;
}

function faceCell(player, ready, enemy, opponent, mates) {
  const side = enemy ? " enemy" : "";
  if (!player) return el("div", `fcell${side} empty`, [champIcon(0, ""), el("div", "txt", [el("span", "fname", "?")])]);
  const classes = `fcell${side}${player.me ? " me" : ""}${opponent ? " opp" : ""}`;
  const cell = el("div", classes, [
    champIcon(player.championId, player.champion),
    el("div", "txt", [
      el("div", "fhead", [
        el("span", "fname", `${upper(player.champion)}${player.me ? " · TOI" : ""}`),
        groupMark(player, mates),
        metMark(player),
        smurfMark(player),
        otpMark(player),
      ]),
      ready && el("span", "line", `niv. ${player.level} · ${player.kills}/${player.deaths}/${player.assists}`),
      // Sur la ligne du rang, pour que les 5 couloirs tiennent toujours dans le bloc.
      el("div", "rank-row", [rankLine(player), recentLine(player)]),
    ]),
  ]);
  // Un clic : son profil (joueurs de ta région ; pas ceux en mode streamer).
  return player.me ? cell : profileLink(cell, player.riotId, player.puuid, "");
}

// ---------- L'app par-dessus le jeu (Ctrl+X) ----------

// Les 10 joueurs en cartes, ton équipe en haut, chaque équipe rangée par poste : l'illustration de leur skin, puis
// leur saison en classée d'après OP.GG (% de victoires et KDA, en tout et sur le champion joué) et leurs champions
// préférés. À droite, ton build (le même que sur l'écran de partie). Un clic sur une carte : le profil du joueur.
let panelOpen = false;
// % de victoires : bleu clair à partir de 55 %, rouge à 45 % ou moins ; gris en dessous de 10 parties (trop peu).
const rateClass = (wins, games) => (games < 10 ? "rate-few" : wins >= 0.55 * games ? "rate-hi" : wins <= 0.45 * games ? "rate-lo" : "");
// KDA : bleu clair à partir de 3,5, rouge en dessous de 2.
const kdaClass = (value) => (value >= 3.5 ? "rate-hi" : value < 2 ? "rate-lo" : "");
const ratio = (k, d, a) => (k + a) / Math.max(d, 1);

function renderPanel(game) {
  $("panel-board").hidden = !game;
  if (!game) return;
  const everyone = [...game.allies, ...game.enemies];
  fillTeam($("pb-allies"), game.allies, false, game.ready, everyone);
  fillTeam($("pb-enemies"), game.enemies, true, game.ready, everyone);
  // Ton build : dès le début de la partie (pendant le chargement, rien encore).
  const build = game.ready && game.build;
  $("pb-gb").hidden = !build;
  $("pb-gb-empty").hidden = Boolean(build);
  $("pb-gb-empty").textContent = game.ready
    ? "Pas de build pour cette partie : mode sans postes, ou OP.GG sans réponse."
    : "Ton build arrive au début de la partie.";
  if (build) fillBuild($("pb-gb"), $("pb-build-aside"), game);
  else $("pb-build-aside").textContent = "";
  // En bas : l'or en objets de chaque équipe et l'écart (bleu : ton équipe devant ; rouge : eux).
  const [ours, theirs] = [teamValue(game.allies), teamValue(game.enemies)];
  const known = game.ready && ours != null && theirs != null;
  $("pb-gold").hidden = !known;
  if (known) {
    const gap = ours - theirs;
    const line = $("pb-gold");
    line.replaceChildren(
      glyph("gold"),
      el("span", "pb-gold-k", "OR"),
      el("span", "pb-gold-mine", whole(ours)),
      el("span", "pb-gold-vs", "–"),
      el("span", "pb-gold-theirs", whole(theirs)),
      el("span", `pb-gold-gap ${gapClass(gap)}`, signed(gap)),
    );
    line.title = "Valeur des objets achetés par chaque équipe (au prix de la boutique) et l'écart";
  }
}

// Une carte par joueur, gardée d'un tour à l'autre : son illustration ne se recharge pas.
function fillTeam(row, players, enemy, ready, everyone) {
  const ids = players.map((p) => p.championId).join(",");
  if (row.dataset.ids !== ids) {
    row.dataset.ids = ids;
    row.replaceChildren(...players.map(() => panelCard(enemy)));
  }
  players.forEach((player, i) => fillCard(row.children[i], player, ready, everyone));
}

function panelCard(enemy) {
  const card = el("button", `pb-card ${enemy ? "enemy" : "ally"}`);
  card.type = "button";
  const skin = el("img", "pb-skin");
  skin.alt = "";
  card.append(
    el("div", "pb-art", [skin, el("span", "pb-fade"), el("div", "pb-marks"), el("span", "pb-live"), el("div", "pb-who")]),
    el("div", "pb-info"),
  );
  return card;
}

function fillCard(card, player, ready, everyone) {
  const key = JSON.stringify([player, ready]);
  if (card.dataset.key === key) return;
  card.dataset.key = key;
  card.classList.toggle("me", player.me);
  setImage(card.querySelector(".pb-skin"), iconUrl(`loadscreen-${player.skinId || player.championId * 1000}`));
  const mates = player.group ? everyone.filter((q) => q !== player && q.group === player.group).map((q) => q.champion) : [];
  const recent = player.recent;
  const streak = recent && Math.abs(recent.streak) >= 3 ? recent.streak : 0;
  const streakMark = streak
    ? el("span", `streak ${streak > 0 ? "win" : "loss"}`, `${Math.abs(streak)} ${streak > 0 ? "V" : "D"}`)
    : null;
  if (streakMark) streakMark.title = `${Math.abs(streak)} ${streak > 0 ? "victoires" : "défaites"} d'affilée`;
  card.querySelector(".pb-marks").replaceChildren(
    ...[groupMark(player, mates), metMark(player), smurfMark(player), otpMark(player), streakMark].filter(Boolean),
  );
  const live = card.querySelector(".pb-live");
  live.hidden = !ready;
  live.textContent = ready ? `niv. ${player.level} · ${player.kills}/${player.deaths}/${player.assists}` : "";
  const name = player.riotId ? player.riotId.split("#")[0] : player.rankHidden ? "Joueur masqué" : "";
  card.querySelector(".pb-who").replaceChildren(
    el("span", "pb-champ", `${upper(player.champion)}${player.me ? " · TOI" : ""}`),
    el("span", "pb-name", name),
  );
  card.querySelector(".pb-info").replaceChildren(...panelInfo(player));
  card.classList.toggle("profile-link", Boolean(!player.me && (player.riotId || player.puuid)));
  card.title = card.classList.contains("profile-link") ? `Voir le profil de ${player.riotId || "ce joueur"}` : "";
  card.onclick = card.classList.contains("profile-link") ? () => searchPlayer(player.riotId, "", player.puuid) : null;
}

// Petits dessins fixes (aucune donnée extérieure) pour lire les stats d'un coup d'œil, sans libellés : épées (KDA),
// coupe (la saison), étoile (ses préférés), pièces (l'or).
const GLYPHS = {
  gold: `<ellipse cx="12" cy="6.5" rx="7" ry="3"/><path d="M5 6.5v5c0 1.7 3.1 3 7 3s7-1.3 7-3v-5"/><path d="M5 11.5v5c0 1.7 3.1 3 7 3s7-1.3 7-3v-5"/>`,
  kills: `<path d="M4 4l10.5 10.5"/><path d="M12.5 17.5l5-5"/><path d="M16 16l4 4"/><path d="M20 4L9.5 14.5"/><path d="M6.5 12.5l5 5"/><path d="M8 16l-4 4"/>`,
  season: `<path d="M8 4h8v5a4 4 0 0 1-8 0z"/><path d="M8 6H5c0 2.5 1.5 4 3.5 4M16 6h3c0 2.5-1.5 4-3.5 4"/><path d="M12 13v4"/><path d="M8.5 21h7l-.8-4H9.3z"/>`,
  star: `<path d="M12 3l2.7 5.6 6.1.8-4.4 4.3 1 6.1L12 17l-5.4 2.8 1-6.1-4.4-4.3 6.1-.8z"/>`,
};

function glyph(name, title = "") {
  const node = el("span", `glyph ${name}`);
  node.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true">${GLYPHS[name]}</svg>`;
  if (title) node.title = title;
  return node;
}

// Sous l'illustration : son rang, puis ce qui se lit d'un coup d'œil, en gros : son % de victoires et son KDA sur
// le champion qu'il joue (son icône devant), puis sur toute sa saison (la coupe devant), et ses champions préférés
// (l'étoile devant).
function panelInfo(player) {
  if (player.rankHidden || !player.riotId) return [el("p", "pb-wait", "Mode streamer : rang et saison masqués")];
  // Son rang en entier : « Émeraude II · 45 LP ».
  const tier = player.rank?.tier;
  const rank = el(
    "div",
    "pb-rank",
    tier
      ? [emblem(tier, "small", true), `${TIERS[tier] ?? tier}${player.rank.division ? ` ${player.rank.division}` : ""} · ${whole(player.rank.lp)} LP`]
      : [player.rank ? "Non classé" : "—"],
  );
  const season = player.season;
  if (!season) return [rank, el("p", "pb-wait", "Saison : lecture d'OP.GG…")];
  if (!season.games) return [rank, el("p", "pb-wait", "Pas de classée cette saison")];
  // Une ligne : un dessin (ou l'icône du champion), son % de victoires et ses parties, son KDA.
  const line = (mark, wins, games, kills, deaths, assists, title, big) => {
    const value = ratio(kills, deaths, assists);
    const row = el("div", `pb-line${big ? " big" : ""}`, [
      mark,
      el("span", "pb-rate-box", [
        el("span", `pb-rate ${games ? rateClass(wins, games) : "rate-few"}`, games ? `${Math.round((wins / games) * 100)} %` : "—"),
        el("span", "pb-games", games ? `${whole(games)} parties` : "0 partie"),
      ]),
      el("span", `pb-kda ${games ? kdaClass(value) : "rate-few"}`, [glyph("kills"), games ? decimal(value) : "—"]),
    ]);
    row.title = title;
    return row;
  };
  const champ = season.champions.find((c) => c.championId === player.championId);
  const onChamp = champ
    ? line(
        champIcon(player.championId, player.champion),
        champ.wins,
        champ.games,
        champ.kills,
        champ.deaths,
        champ.assists,
        `Avec ${player.champion} cette saison : ${champ.wins} victoires sur ${whole(champ.games)} parties classées, KDA ${kda(champ.kills, champ.deaths, champ.assists)}`,
        true,
      )
    : line(champIcon(player.championId, player.champion), 0, 0, 0, 0, 0, `Pas parmi ses champions les plus joués en classée cette saison`, true);
  const all = line(
    glyph("season"),
    season.wins,
    season.games,
    season.kills,
    season.deaths,
    season.assists,
    `Saison : ${season.wins} victoires sur ${whole(season.games)} parties classées (solo et flexible), KDA moyen sur ses champions les plus joués`,
    false,
  );
  // Ses 3 champions les plus joués, leur % de victoires dessous.
  const favs = season.champions.slice(0, 3).map((c) => {
    const node = el("span", "pb-fav", [
      champIcon(c.championId, c.name),
      el("span", rateClass(c.wins, c.games), `${Math.round((c.wins / Math.max(c.games, 1)) * 100)} %`),
    ]);
    node.title = `${c.name} : ${whole(c.games)} parties, ${Math.round((c.wins / Math.max(c.games, 1)) * 100)} % de victoires, KDA ${kda(c.kills, c.deaths, c.assists)}`;
    return node;
  });
  return [rank, onChamp, all, el("div", "pb-favs", [glyph("star", "Ses champions les plus joués cette saison"), ...favs])];
}

// ---------- Ensemble ----------

// ---------- Recherche d'un joueur ----------

// Salon du joueur cherché (search_player) à la place du tien ; null = le tien.
let viewing = null;
let lastState = null;
// Numéro de la dernière recherche : une réponse arrivée après une autre recherche (ou un retour) est ignorée.
let searchTurn = 0;
const REGIONS = ["EUW", "EUNE", "NA", "KR", "BR", "LAN", "LAS", "OCE", "TR", "RU", "JP", "ME", "SEA", "TW", "VN"];
// Écran du moment : salon, sélection des champions ou partie. La recherche ramène à celui d'où elle a été faite.
const screenOf = (state) => (state?.game ? "game" : state?.runes ? "selection" : "lobby");
const BACK = {
  lobby: ["MON PROFIL", "Revenir à ton profil"],
  selection: ["LA SÉLECTION", "Revenir à la sélection des champions"],
  game: ["LA PARTIE", "Revenir à la partie"],
};

function setupSearch() {
  const region = $("search-region");
  region.replaceChildren(...REGIONS.map((code) => el("option", "", code)));
  try {
    region.value = localStorage.getItem("search-region") || "EUW";
  } catch {
    region.value = "EUW";
  }
  region.addEventListener("change", () => {
    try {
      localStorage.setItem("search-region", region.value);
    } catch {
      // Pas de mémoire du navigateur : la région revient à EUW au prochain lancement.
    }
  });
  $("search").addEventListener("submit", async (event) => {
    event.preventDefault();
    closeFriends();
    const text = $("search-input").value.trim();
    // Sans « # » : le premier champion proposé pour ce texte, s'il y en a un.
    if (text && !text.includes("#")) {
      await loadChampions();
      const champ = championMatches(text)[0];
      if (champ) {
        $("search-input").value = champ.name;
        return openChampion(champ.id);
      }
    }
    searchPlayer(text, $("search-region").value);
  });
  $("search-input").addEventListener("click", openFriends);
  $("search-input").addEventListener("input", (event) => {
    searchMessage("");
    friendsQuery = event.target.value.trim().toLowerCase();
    if (friendsOpen) renderFriends();
    else openFriends(true);
  });
  $("search-input").addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    // Échap ferme d'abord la liste d'amis.
    if (friendsOpen) return closeFriends();
    event.target.value = "";
    leaveSearch(true);
  });
  $("search-back").addEventListener("click", () => leaveSearch(true));
  // Un clic ailleurs que sur la barre ou la liste la ferme.
  document.addEventListener(
    "pointerdown",
    (event) => {
      if (friendsOpen && !event.target.closest("#search, #friends")) closeFriends();
    },
    true,
  );
}

// ---------- Tes amis et les champions (sous la barre de recherche) ----------

// Liste du client, ouverte d'un clic sur la barre : ceux qui jouent d'abord, puis ceux en ligne, puis les autres
// (la molette la fait défiler). Le texte tapé après l'ouverture la filtre (pas le joueur déjà affiché dans la
// barre) ; un clic sur un ami ouvre son profil. Relue toutes les 20 s tant qu'elle est ouverte, rien sinon.
// Au-dessus, les champions dont le nom commence par le texte tapé (« da » : Darius) : un clic ouvre leur page.
const FRIEND_STATUS = {
  game: "En partie",
  select: "En sélection",
  queue: "En file",
  lobby: "Dans un salon",
  online: "En ligne",
  away: "Absent",
  mobile: "Sur mobile",
  offline: "Hors ligne",
};
const FRIEND_GROUPS = [
  ["EN JEU", ["game", "select", "queue", "other"]],
  ["EN LIGNE", ["lobby", "online", "away", "mobile"]],
  ["HORS LIGNE", ["offline"]],
];
let friendList = [];
let friendsOpen = false;
let friendsTimer = 0;
let friendsQuery = "";

async function loadFriends() {
  try {
    friendList = await invoke("friends");
  } catch {
    friendList = [];
  }
  if (friendsOpen) renderFriends();
}

// `typed` : ouverte en tapant (le texte tapé filtre déjà) ; sinon d'un clic, toute la liste.
function openFriends(typed = false) {
  if (friendsOpen) return;
  friendsOpen = true;
  if (typed !== true) friendsQuery = "";
  renderFriends();
  loadChampions();
  loadFriends();
  friendsTimer = setInterval(loadFriends, 20000);
}

function closeFriends() {
  if (!friendsOpen) return;
  friendsOpen = false;
  clearInterval(friendsTimer);
  $("friends").hidden = true;
}

function renderFriends() {
  const box = $("friends");
  const query = friendsQuery;
  const champions = championMatches(query);
  const shown = friendList.filter((f) => !query || f.riotId.toLowerCase().includes(query));
  box.hidden = !friendsOpen || (!shown.length && !champions.length);
  // Rien à refaire si les listes, le filtre et la minute n'ont pas changé (les icônes ne sont pas rechargées).
  const key = `${query}|${champions.map((c) => c.id)}|${Math.floor(Date.now() / 60000)}|${JSON.stringify(shown)}`;
  if (box.dataset.key === key) return;
  if (!box.dataset.key?.startsWith(`${query}|`)) box.scrollTop = 0;
  box.dataset.key = key;
  const rows = [];
  if (champions.length) {
    rows.push(el("div", "friend-group", ["CHAMPIONS", el("small", "", String(champions.length))]));
    rows.push(...champions.map(championRow));
  }
  for (const [title, statuses] of FRIEND_GROUPS) {
    const group = shown.filter((f) => statuses.includes(f.status));
    if (!group.length) continue;
    rows.push(el("div", "friend-group", [title, el("small", "", String(group.length))]));
    rows.push(...group.map(friendRow));
  }
  box.replaceChildren(...rows);
}

function friendRow(friend) {
  const row = el("button", `friend ${friend.status}`);
  row.type = "button";
  row.title = `Voir le profil de ${friend.riotId}`;
  const icon = el("img", "friend-icon");
  icon.alt = "";
  icon.loading = "lazy";
  if (friend.icon > 0) setImage(icon, iconUrl(`profile-${friend.icon}`));
  const [name, tag] = friend.riotId.split("#");
  row.append(
    icon,
    el("span", "friend-main", [
      el("span", "friend-name", [name, el("small", "", `#${tag}`)]),
      el("span", "friend-status", [el("i", "friend-dot"), el("span", "friend-text", friendStatus(friend))]),
    ]),
  );
  row.addEventListener("click", () => {
    closeFriends();
    $("search-input").value = friend.riotId;
    searchPlayer(friend.riotId, "", friend.puuid);
  });
  return row;
}

// Tous les champions (client, sinon OP.GG), lus une fois. Pour la recherche, leur nom sans accents ni signes
// (« Kai'Sa » : « kaisa »), chacun de ses mots (« Maître Yi » : « yi ») et leur nom interne (« MonkeyKing »).
let championList = [];
const fold = (text) => (text ?? "").normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
const squash = (text) => fold(text).replace(/[^a-z0-9]/g, "");
async function loadChampions() {
  if (championList.length) return;
  try {
    const list = await invoke("champions");
    championList = list.map((c) => ({
      ...c,
      key: squash(c.name),
      words: fold(c.name).split(/[^a-z0-9]+/).filter(Boolean),
      aliasKey: squash(c.alias),
    }));
  } catch {
    // Client fermé et OP.GG sans réponse : réessayé à la prochaine ouverture de la liste.
    championList = [];
  }
  if (friendsOpen) renderFriends();
}

// Champions proposés pour le texte tapé (pas pour un Riot ID) : ceux dont le nom commence par lui, puis ceux dont un
// mot ou le nom interne commence par lui ; 6 au plus, par ordre alphabétique.
const CHAMPIONS_SHOWN = 6;
function championMatches(text) {
  const query = squash(text);
  if (!query || text.includes("#")) return [];
  const score = (c) =>
    c.key.startsWith(query) ? 0 : c.words.some((w) => w.startsWith(query)) ? 1 : c.aliasKey.startsWith(query) ? 2 : -1;
  return championList
    .map((c) => [score(c), c])
    .filter(([rank]) => rank >= 0)
    .sort((a, b) => a[0] - b[0] || a[1].name.localeCompare(b[1].name, "fr"))
    .slice(0, CHAMPIONS_SHOWN)
    .map(([, c]) => c);
}

function championRow(champ) {
  const row = el("button", "friend champ-suggest");
  row.type = "button";
  row.title = `Voir la page de ${champ.name} : stats, build, duels et meilleurs joueurs`;
  const icon = el("img", "friend-icon");
  icon.alt = "";
  setImage(icon, iconUrl(champ.id));
  row.append(icon, el("span", "friend-name", champ.name));
  row.addEventListener("click", () => {
    closeFriends();
    $("search-input").value = champ.name;
    openChampion(champ.id);
  });
  return row;
}

// « En partie · Vayne · SOLO/DUO · 12 min », « Sur Valorant », « Hors ligne · il y a 3 jours »…
function friendStatus(friend) {
  const parts = [friend.status === "other" ? `Sur ${friend.product}` : FRIEND_STATUS[friend.status]];
  if (friend.champion) parts.push(friend.champion);
  if (["game", "select", "queue"].includes(friend.status)) {
    const queue = QUEUES[friend.queueId] ?? (["TFT", "ARAM"].includes(friend.mode) ? friend.mode : "");
    if (queue) parts.push(queue);
  }
  if (friend.status === "game" && friend.since > 0) {
    parts.push(`${Math.max(1, Math.round((Date.now() - friend.since) / 60000))} min`);
  }
  if (friend.status === "offline" && friend.lastSeen > 0) parts.push(ago(friend.lastSeen));
  return parts.join(" · ");
}

// `region` vide : celle de ton client ; `puuid` : son compte, quand il est connu (joueur d'une de tes parties).
async function searchPlayer(riotId, region, puuid = "") {
  if (!riotId && !puuid) return;
  if (!puuid && !riotId.includes("#")) {
    return searchMessage("Ajoute son tag : Pseudo#TAG (ex. Faker#KR1), ou tape le nom d'un champion", true);
  }
  const turn = ++searchTurn;
  // Depuis le profil d'un joueur cherché (ou la page d'un champion), le retour mène toujours à l'écran de départ.
  const from = viewing?.from ?? champView?.from ?? screenOf(lastState);
  $("search").classList.add("busy");
  searchMessage(`Recherche de ${riotId || "ce joueur"}${region ? ` en ${region}` : ""}…`);
  try {
    const found = await invoke("search_player", { riotId, region, puuid: puuid || null });
    if (turn !== searchTurn) return;
    champView = null;
    viewing = { ...found, from };
    $("search-input").value = found.riotId;
    if (REGIONS.includes(found.region)) $("search-region").value = found.region;
    searchMessage("");
    closeMatch();
    if (lastState) render(lastState);
  } catch (error) {
    if (turn === searchTurn) searchMessage(String(error), true);
  } finally {
    if (turn === searchTurn) $("search").classList.remove("busy");
  }
}

// Page d'un champion à la place de l'écran du moment (comme le profil d'un joueur cherché) : à `position` (un de
// ses onglets), sinon à son poste le plus joué. Lue sur OP.GG à la demande, gardée ensuite par l'app.
let champView = null;
async function openChampion(id, position = null) {
  const turn = ++searchTurn;
  const from = champView?.from ?? viewing?.from ?? screenOf(lastState);
  const name = championList.find((c) => c.id === id)?.name ?? "ce champion";
  $("search").classList.add("busy");
  searchMessage(`Page de ${name} : lecture d'OP.GG…`);
  try {
    const page = await invoke("champion_page", { championId: id, position });
    if (turn !== searchTurn) return;
    viewing = null;
    champView = { page, from };
    $("search-input").value = page.name;
    searchMessage("");
    closeMatch();
    if (lastState) render(lastState);
  } catch (error) {
    if (turn === searchTurn) searchMessage(String(error), true);
  } finally {
    if (turn === searchTurn) $("search").classList.remove("busy");
  }
}

// Retour à ton salon ; `redraw` : tout de suite (sinon, le rendu en cours s'en charge).
function leaveSearch(redraw) {
  searchTurn++;
  viewing = null;
  champView = null;
  $("search-input").value = "";
  $("search").classList.remove("busy");
  searchMessage("");
  if (openGame) closeMatch();
  if (redraw && lastState) render(lastState);
}

// Profil d'un joueur d'un clic (détail d'une partie, joueurs de la partie en cours).
function profileLink(node, riotId, puuid, region) {
  if (!riotId && !puuid) return node;
  node.classList.add("profile-link");
  node.title = `Voir le profil de ${riotId || "ce joueur"}`;
  node.addEventListener("click", (event) => {
    event.stopPropagation();
    if (openGame) closeMatch();
    searchPlayer(riotId, region, puuid);
  });
  return node;
}

function searchMessage(text, error = false) {
  $("search-msg").hidden = !text;
  $("search-msg").textContent = text;
  $("search-msg").classList.toggle("error", error);
}

function render(state) {
  lastState = state;
  const found = state.clientFound;
  $("status-text").textContent = !found ? "CLIENT NON DÉTECTÉ" : (PHASES[state.phase] ?? upper(state.phase ?? "CLIENT CONNECTÉ"));
  $("status").className = `status ${!found ? "off" : state.game ? "game" : "on"}`;
  // Joueur ou champion cherché : à la place de l'écran du moment, quel qu'il soit. Un nouvel écran (sélection qui
  // commence, partie qui démarre ou se termine) le referme.
  const screen = screenOf(state);
  const searched = viewing ?? champView;
  if (searched && searched.from !== screen) leaveSearch(false);
  const [back, title] = BACK[screen];
  $("search-back").hidden = !viewing && !champView;
  $("search-back-text").textContent = back;
  $("search-back").title = title;
  document.body.classList.toggle("searching", Boolean(viewing));
  $("hint").hidden = found || Boolean(viewing || champView);

  $("update").hidden = !state.update;
  if (state.update && !$("update-button").disabled) {
    $("update-text").textContent = `Nouvelle version ${state.update} : installée toute seule hors sélection et hors partie`;
  }

  // Salon : seulement hors sélection et hors partie (la place y sert à autre chose). Celui d'un joueur cherché
  // s'affiche sur tous les écrans, même client fermé (OP.GG), sans tes LP ni tes stats.
  const lobby = !champView && (Boolean(viewing) || (found && screen === "lobby"));
  if (!lobby && openGame) closeMatch();
  const shown = viewing ? { riotId: viewing.riotId, summonerLevel: viewing.level, profile: viewing.profile, searched: viewing } : state;
  renderPlayer(shown, lobby);
  renderHistory(shown, lobby);
  renderStats(state, lobby && !viewing);
  renderLp(state.profile, lobby && !viewing && Boolean(state.profile));
  renderSelection(viewing || champView ? null : state.runes);
  // App par-dessus le jeu (Ctrl+X) : les 10 joueurs en cartes à la place de l'écran de partie.
  const panel = panelOpen && !viewing && !champView ? state.game : null;
  renderGame(viewing || champView || panel ? null : state.game);
  renderPanel(panel);
  renderChampion(champView?.page ?? null);
  layout();
}

// ---------- Page d'un champion ----------

// Comme sur DPM, d'après OP.GG : à gauche son illustration, ses stats à chacun de ses postes (un onglet chacun), sa
// page de runes et ses compétences niveau par niveau ; au milieu son build (avec le % de victoires de chaque choix)
// et ses duels ; à droite ses meilleurs partenaires et ses meilleurs joueurs. Un clic sur un champion ouvre sa page,
// un clic sur un joueur son profil.
const CHAMP_CARDS = ["champ-hero", "champ-stats", "champ-runes", "champ-build", "champ-duels", "champ-duos", "champ-players"];
const DIVISIONS = ["", "I", "II", "III", "IV"];
const percent = (rate) => [decimal(rate * 100), el("small", "", " %")];
let shownChampion = "";
function renderChampion(page) {
  for (const id of CHAMP_CARDS) $(id).hidden = !page;
  if (!page) {
    shownChampion = "";
    return;
  }
  // Une page ne change plus une fois lue : refaite seulement pour un autre champion ou un autre poste.
  const key = `${page.id}|${page.position}`;
  if (key === shownChampion) return;
  shownChampion = key;
  const here = page.positions.find((p) => p.position === page.position);
  renderChampionHero(page, here);
  renderChampionStats(page, here);
  renderChampionRunes(page);
  renderChampionBuild(page);
  renderChampionDuels(page);
  renderChampionDuos(page);
  renderChampionPlayers(page);
}

function renderChampionHero(page, here) {
  setArt($("champ-art"), page.id * 1000);
  const title = upper(page.name);
  $("champ-name").textContent = title;
  $("champ-name").className = `name-box ${title.length > 12 ? "long" : title.length > 9 ? "mid" : ""}`;
  $("champ-tag").textContent = POSITIONS[page.position] ?? upper(page.position);
  $("champ-rank").hidden = !here?.tier;
  $("champ-tier").textContent = here?.tier ? `TIER ${here.tier}` : "";
  $("champ-place").textContent = here?.rank ? `${place(here.rank)} au classement` : "";
}

function renderChampionStats(page, here) {
  // Ses postes (au moins 1 % de ses parties), le plus joué d'abord.
  const tabs = page.positions
    .filter((p) => p.roleRate >= 0.01 || p.position === page.position)
    .map((p) => {
      const on = p.position === page.position;
      const label = POSITIONS[p.position] ?? upper(p.position);
      const tab = el("button", `champ-tab${on ? " on" : ""}`, [label, el("small", "", `${whole(p.roleRate * 100)} %`)]);
      tab.type = "button";
      tab.title = `${page.name} ${label} : ${whole(p.roleRate * 100)} % de ses parties (${whole(p.games)})`;
      if (!on) tab.addEventListener("click", () => openChampion(page.id, p.position));
      return tab;
    });
  $("champ-tabs").replaceChildren(...tabs);
  // Sans stats d'OP.GG à ce poste : des tirets.
  const value = (show) => (here ? show(here) : "–");
  $("champ-cells").replaceChildren(
    ...[
      ["VICTOIRES", value((s) => percent(s.winRate)), here ? (here.winRate >= 0.5 ? "win" : "loss") : ""],
      ["CHOIX", value((s) => percent(s.pickRate))],
      ["BANS", value((s) => percent(s.banRate))],
      ["PARTIES", value((s) => whole(s.games))],
      ["KDA", value((s) => decimal(s.kda))],
      ["À CE POSTE", value((s) => [whole(s.roleRate * 100), el("small", "", " %")])],
    ].map(cell),
  );
}

function renderChampionRunes(page) {
  const runes = page.runes;
  const icon = (id, name, className) => {
    const img = el("img", className);
    img.alt = "";
    img.title = name ?? "";
    setImage(img, iconUrl(`perk-${id}`));
    return img;
  };
  if (!runes) {
    $("champ-page").replaceChildren(el("p", "muted small", "Pas de page de runes sur OP.GG à ce poste."));
  } else {
    // Ses 3 autres runes principales | ses 2 secondaires | ses 3 fragments.
    const others = runes.perks.slice(1).flatMap((id, i) => [
      i === 3 || i === 5 ? el("span", "rsep") : null,
      icon(id, runes.runeNames[i + 1], i >= 5 ? "ricon shard" : "ricon"),
    ]);
    $("champ-page").replaceChildren(
      el("div", "cp-rune-head", [
        icon(runes.perks[0], runes.keystone, "cp-keystone"),
        el("span", "cp-rune-text", [el("span", "cp-rune-name", runes.keystone), el("span", "muted small", `${runes.primary} · ${runes.secondary}`)]),
        el("span", "rstat", [
          el("span", "rpct", runes.games ? percent(runes.wins / runes.games) : "–"),
          el("span", "rgames", `${whole(runes.games)} parties`),
        ]),
      ]),
      el("div", "ricons cp-rune-icons", others.filter(Boolean)),
    );
  }
  // Sa montée (Q › E › W), puis la compétence prise à chaque niveau.
  const ability = (key) => keyIcon(iconUrl(`ability-${page.id}-${key.toLowerCase()}`), key, key);
  const order = page.skillOrder;
  const blocks = [];
  if (page.skillMax.length) {
    const keys = page.skillMax.flatMap((key, i) => (i ? [el("span", "gb-arrow", "›"), ability(key)] : [ability(key)]));
    blocks.push(el("div", "sg-max", [el("span", "gb-k", "MONTÉE"), ...keys]));
  }
  if (order.length) {
    for (const key of ["Q", "W", "E", "R"]) {
      const cells = order.map((k, i) => el("span", `sg-cell${k === key ? " on" : ""}`, k === key ? String(i + 1) : ""));
      blocks.push(el("div", `sg-row${key === "R" ? " ult" : ""}`, [ability(key), ...cells]));
    }
  }
  $("champ-skills").replaceChildren(...blocks);
}

function renderChampionBuild(page) {
  // % de victoires et parties d'un choix, à droite de sa ligne.
  const rate = (games, wins) =>
    el("span", "cp-rate", [...(games ? percent(wins / games) : ["–"]), el("span", "cp-games", ` · ${whole(games)}`)]);
  const together = (items) => (items.length ? rate(items[0].games, items[0].wins) : null);
  // 4e, 5e et 6e objet, côte à côte : les choix les plus joués, le % de victoires de chacun dessous.
  const choices = (items) =>
    items.length
      ? el(
          "div",
          "gb-icons",
          items.map((it) => {
            const node = buildIcon(it, "mid", it.games ? `${Math.round((it.wins / it.games) * 100)} %` : "");
            node.firstChild.title = `${it.name} : ${whole(it.games)} parties`;
            return node;
          }),
        )
      : null;
  const spells = page.spells.length
    ? el("span", "bicons", page.spells.map((id, i) => keyIcon(iconUrl(`spell-${id}`), "", page.spellNames[i] ?? "")))
    : null;
  const core = page.core;
  const slots = [
    ["4E OBJET", page.fourth],
    ["5E OBJET", page.fifth],
    ["6E OBJET", page.sixth],
  ].filter(([, items]) => items.length);
  const later = slots.length
    ? el("div", "cp-later", slots.map(([label, items]) => el("div", "cp-slot", [el("span", "gb-k", label), choices(items)])))
    : null;
  const rows = [
    ["SORTS", spells, spells && rate(page.spellGames, page.spellWins)],
    ["DÉPART", page.starter.length ? itemIcons(page.starter) : null, together(page.starter)],
    ["BOTTES", page.boots.length ? itemIcons(page.boots) : null, together(page.boots)],
    ["CŒUR", core ? itemIcons(core.items, "→") : null, core && rate(core.games, core.wins)],
    ["ENSUITE", later],
  ].filter(([, value]) => value);
  $("champ-build-empty").hidden = rows.length > 0;
  $("champ-build-list").replaceChildren(
    ...rows.map(([label, value, stat]) => el("div", "lrow", [el("span", "k", label), el("div", "", [value]), stat ?? el("span")])),
  );
}

// Une ligne de champion : son icône, son nom, (son poste), son % de victoires et ses parties ; un clic ouvre sa page.
function duelRow(duel, title, position = null) {
  const row = el("button", "cd-row", [
    champIcon(duel.championId, duel.name),
    el("span", "cd-name", upper(duel.name)),
    position ? el("span", "cd-pos", SHORT_POSITIONS[position] ?? "") : null,
    el("span", "cd-stat", [
      el("span", `cd-pct ${duel.winRate >= 0.5 ? "win" : "loss"}`, percent(duel.winRate)),
      el("span", "cd-games", `${whole(duel.games)} parties`),
    ]),
  ]);
  row.type = "button";
  row.title = `${title} : ${decimal(duel.winRate * 100)} % de victoires (${whole(duel.games)} parties) · voir sa page`;
  row.addEventListener("click", () => openChampion(duel.championId, position));
  return row;
}

function renderChampionDuels(page) {
  $("champ-duels-aside").textContent = `victoires de ${page.name} contre eux`;
  const column = (title, list) =>
    el("div", "cd-col", [
      el("div", "sub-head", [el("span", "kicker", title)]),
      ...list.map((d) => duelRow(d, `${page.name} contre ${d.name}`)),
    ]);
  $("champ-duel-lists").replaceChildren(column("LES PLUS FACILES", page.good), column("LES PLUS DURS", page.bad));
}

function renderChampionDuos(page) {
  $("champ-duos-empty").hidden = page.synergies.length > 0;
  $("champ-duo-list").replaceChildren(...page.synergies.map((d) => duelRow(d, `${page.name} avec ${d.name}`, d.position)));
}

function renderChampionPlayers(page) {
  $("champ-players-aside").textContent = `${page.region} · OP.GG`;
  $("champ-players-empty").hidden = page.players.length > 0;
  const rows = page.players.map((p) => {
    const cut = p.riotId.lastIndexOf("#");
    const [name, tag] = cut >= 0 ? [p.riotId.slice(0, cut), p.riotId.slice(cut)] : [p.riotId, ""];
    const division = APEX.includes(p.tier) ? "" : ` ${DIVISIONS[p.division] ?? p.division}`;
    const rank = p.tier ? `${TIERS[p.tier] ?? p.tier}${division} · ${whole(p.lp)} LP` : "Non classé";
    const rate = p.games ? p.wins / p.games : 0;
    const row = el("button", "cd-row player-row", [
      el("span", "cd-rank", String(p.rank)),
      p.tier ? emblem(p.tier, "small", true) : el("span", "cd-noemblem"),
      el("span", "cd-who", [el("span", "cd-player", [name, el("small", "", tag)]), el("span", "cd-sub", rank)]),
      el("span", "cd-stat", [el("span", `cd-pct ${rate >= 0.5 ? "win" : "loss"}`, percent(rate)), el("span", "cd-games", `${whole(p.games)} parties`)]),
    ]);
    row.type = "button";
    row.title = `${p.riotId} : ${whole(p.games)} parties avec ${page.name}, KDA ${kda(p.kills, p.deaths, p.assists)} · voir son profil`;
    row.addEventListener("click", () => {
      $("search-input").value = p.riotId;
      searchPlayer(p.riotId, page.region);
    });
    return row;
  });
  $("champ-player-list").replaceChildren(...rows);
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
  const view = !$("panel-board").hidden
    ? "panel"
    : !$("champ-hero").hidden
      ? "champion"
      : !$("player").hidden
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
  setupSearch();
  layout();
  window.addEventListener("resize", layout);
  for (const button of $("forms").querySelectorAll("button")) {
    button.addEventListener("click", () => {
      const same = $("forms").dataset.manual === button.dataset.form;
      invoke("set_form", { form: same ? null : button.dataset.form });
    });
  }
  $("settings-button").addEventListener("click", toggleSettings);
  $("settings-close").addEventListener("click", toggleSettings);
  // Version de l'app (celle de tauri.conf.json), à côté du titre des réglages.
  window.__TAURI__.app
    .getVersion()
    .then((version) => ($("settings-title").textContent = `RÉGLAGES · v${version}`))
    .catch(() => {});
  $("match-close").addEventListener("click", closeMatch);
  // Réglages ou détail d'une partie ouverts : un clic ailleurs dans l'app les referme, sans rien déclencher
  // d'autre (un clic sur le bouton des réglages les referme aussi).
  document.addEventListener(
    "click",
    (event) => {
      const settingsOpen = !$("settings").hidden && !$("settings").contains(event.target);
      const matchOpen = openGame && $("match").contains(event.target) === false;
      if (!settingsOpen && !matchOpen) return;
      event.stopPropagation();
      event.preventDefault();
      if (settingsOpen) $("settings").hidden = true;
      else closeMatch();
    },
    true,
  );
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    if (!$("settings").hidden) $("settings").hidden = true;
    else if (openGame) closeMatch();
  });
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
  // Ctrl+X : l'app vient par-dessus le jeu (true) ou revient à sa place (false).
  await listen("panel", (event) => {
    panelOpen = Boolean(event.payload);
    if (lastState) render(lastState);
  });
  render(await invoke("get_state"));
});
