mod history;
mod lol;
mod opgg;
mod overlay;
mod search;
mod settings;

use overlay::Overlay;
use settings::Settings;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent,
};
use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use tauri_plugin_notification::NotificationExt as _;
use tauri_plugin_updater::UpdaterExt as _;

/// Dernier état envoyé à l'interface.
struct Latest(Mutex<lol::Snapshot>);
/// Adversaire direct choisi d'un clic dans l'interface (None = deviné par l'app).
struct Opponent(Mutex<Option<i64>>);
/// Forme de Kayn choisie d'un clic (None = conseillée par l'app).
struct ChosenForm(Mutex<Option<lol::Form>>);
/// Page de runes choisie d'un clic dans la sélection (ses runes), pour la sélection en cours.
struct ChosenPage(Mutex<Option<Vec<i64>>>);
/// Réglages, et le fichier où ils sont gardés.
struct Prefs {
    file: PathBuf,
    settings: Mutex<Settings>,
}
/// Nouvelle version trouvée, prête à installer.
struct PendingUpdate(Mutex<Option<tauri_plugin_updater::Update>>);
/// Lancement avec Windows (clé « Run » du registre, au nom de l'app). None en développement : seule la version
/// installée y touche, jamais l'exe de développement (Windows le lancerait au démarrage, avec un terminal).
struct Startup(Option<AutoLaunch>);

/// Argument du lancement avec Windows : l'app démarre cachée près de l'horloge.
const MINIMIZED: &str = "--minimized";

/// Mises à jour : vérifiées au démarrage puis toutes les 3 h ; 5 min après un échec (au démarrage de Windows,
/// internet n'est pas toujours là).
const UPDATE_EVERY: Duration = Duration::from_secs(3 * 60 * 60);
const UPDATE_RETRY: Duration = Duration::from_secs(5 * 60);
/// Installation automatique : après une minute ni en sélection ni en partie (fenêtre au premier plan ou non) ;
/// 3 essais au plus, à 30 min d'écart (le bouton du bandeau reste là).
const UPDATE_QUIET: Duration = Duration::from_secs(60);
const UPDATE_TRIES: u32 = 3;
const UPDATE_GAP: Duration = Duration::from_secs(30 * 60);
/// Fenêtre cachée ou affichée juste avant une mise à jour, relu au redémarrage (s'il a moins de 5 min) pour la
/// remettre pareil : l'installateur relance l'app avec les options de son lancement, pas son état du moment.
const UPDATE_MARK: &str = "maj-fenetre.txt";
const UPDATE_MARK_AGE: Duration = Duration::from_secs(5 * 60);
/// Une seule installation à la fois (bouton du bandeau ou installation automatique).
static INSTALLING: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn get_state(latest: tauri::State<'_, Latest>) -> lol::Snapshot {
    latest.0.lock().unwrap().clone()
}

#[tauri::command]
fn set_opponent(opponent: tauri::State<'_, Opponent>, champion_id: Option<i64>) {
    *opponent.0.lock().unwrap() = champion_id;
}

/// "red" ou "blue" ; null (ou autre chose) : retour à la forme conseillée.
#[tauri::command]
fn set_form(chosen: tauri::State<'_, ChosenForm>, form: Option<String>) {
    *chosen.0.lock().unwrap() = form.as_deref().and_then(lol::Form::parse);
}

/// Runes de la page choisie parmi celles proposées ; null : retour à la première.
#[tauri::command]
fn set_rune_page(chosen: tauri::State<'_, ChosenPage>, perks: Option<Vec<i64>>) {
    *chosen.0.lock().unwrap() = perks;
}

#[tauri::command]
fn get_settings(prefs: tauri::State<'_, Prefs>, startup: tauri::State<'_, Startup>) -> Settings {
    let mut settings = prefs.settings.lock().unwrap().clone();
    settings.start_with_windows = startup.0.as_ref().is_some_and(|launch| launch.is_enabled().unwrap_or(false));
    settings
}

#[tauri::command]
fn set_settings(
    prefs: tauri::State<'_, Prefs>,
    startup: tauri::State<'_, Startup>,
    settings: Settings,
) -> Result<Settings, String> {
    let changed = |launch: &&AutoLaunch| settings.start_with_windows != launch.is_enabled().unwrap_or(false);
    if let Some(launch) = startup.0.as_ref().filter(changed) {
        let done = if settings.start_with_windows { launch.enable() } else { launch.disable() };
        done.map_err(|e| format!("Lancement avec Windows impossible à changer : {e}"))?;
    }
    settings.save(&prefs.file)?;
    *prefs.settings.lock().unwrap() = settings.clone();
    Ok(settings)
}

/// Une partie de ton historique en détail (salon) : les 10 joueurs, ton équipe d'abord.
#[tauri::command]
async fn match_details(
    assets: tauri::State<'_, lol::Assets>,
    game_id: i64,
    champion_id: i64,
) -> Result<history::MatchDetails, String> {
    assets.match_details(game_id, champion_id).await
}

/// Tes amis d'après le client (barre de recherche), ceux qui jouent d'abord.
#[tauri::command]
async fn friends(assets: tauri::State<'_, lol::Assets>) -> Result<Vec<lol::Friend>, String> {
    assets.friends().await.ok_or_else(|| "Liste d'amis indisponible : client fermé ?".to_string())
}

/// Salon d'un autre joueur, cherché par son Riot ID (Nom#TAG) et sa région (vide : celle de ton client), ou par
/// son compte (un joueur d'une de tes parties).
#[tauri::command]
async fn search_player(
    assets: tauri::State<'_, lol::Assets>,
    search: tauri::State<'_, search::Search>,
    riot_id: String,
    region: String,
    puuid: Option<String>,
) -> Result<search::Searched, String> {
    search.find(&assets, &riot_id, &region, puuid.as_deref()).await
}

/// Tous les champions, pour les suggestions de la barre de recherche.
#[tauri::command]
async fn champions(
    assets: tauri::State<'_, lol::Assets>,
    search: tauri::State<'_, search::Search>,
) -> Result<Vec<search::ChampionEntry>, String> {
    search.champions(&assets).await
}

/// Page d'un champion (comme sur DPM) à un poste ("top"…), sinon à son poste le plus joué, d'après OP.GG.
#[tauri::command]
async fn champion_page(
    assets: tauri::State<'_, lol::Assets>,
    search: tauri::State<'_, search::Search>,
    champion_id: i64,
    position: Option<String>,
) -> Result<search::ChampionPage, String> {
    search.champion_page(&assets, champion_id, position).await
}

/// Une partie d'un joueur cherché, lue sur OP.GG, en détail.
#[tauri::command]
async fn searched_match_details(
    search: tauri::State<'_, search::Search>,
    game_id: i64,
) -> Result<history::MatchDetails, String> {
    search.match_details(game_id).await
}

/// Télécharge et installe la nouvelle version, puis relance l'app
/// (sous Windows, l'installateur ferme l'app et la relance lui-même).
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("Mise à jour déjà en cours…".to_string());
    }
    let update = app.state::<PendingUpdate>().0.lock().unwrap().clone();
    let Some(update) = update else {
        INSTALLING.store(false, Ordering::SeqCst);
        return Err("Aucune mise à jour à installer.".to_string());
    };
    let mark = remember_window(&app);
    if let Err(e) = update.download_and_install(|_, _| {}, || {}).await {
        if let Some(file) = mark {
            let _ = std::fs::remove_file(file);
        }
        INSTALLING.store(false, Ordering::SeqCst);
        return Err(format!("Mise à jour impossible : {e}"));
    }
    app.restart()
}

/// Note si la fenêtre est cachée (ou réduite) juste avant une mise à jour.
fn remember_window(app: &AppHandle) -> Option<PathBuf> {
    let window = app.get_webview_window("main")?;
    let hidden = !window.is_visible().unwrap_or(true) || window.is_minimized().unwrap_or(false);
    let file = app.path().app_config_dir().ok()?.join(UPDATE_MARK);
    std::fs::write(&file, if hidden { "cachee" } else { "visible" }).ok()?;
    Some(file)
}

/// Le lancement avec Windows de cet exe : son chemin entre guillemets (son dossier, « LoL Companion », a une
/// espace : sans guillemets, Windows peut ne pas le lancer ou perdre « --minimized »), puis « --minimized ».
fn startup_entry(app: &AppHandle) -> Option<AutoLaunch> {
    let exe = std::env::current_exe().ok()?;
    let path = if cfg!(windows) { format!("\"{}\"", exe.display()) } else { exe.display().to_string() };
    AutoLaunchBuilder::new()
        .set_app_name(&app.package_info().name)
        .set_app_path(&path)
        .set_args(&[MINIMIZED])
        .build()
        .inspect_err(|e| eprintln!("[app] lancement avec Windows indisponible : {e}"))
        .ok()
}

/// Fenêtre cachée au lancement ? Comme juste avant la mise à jour qui vient de relancer l'app, sinon seulement au
/// lancement avec Windows (--minimized).
fn start_hidden(app: &AppHandle) -> bool {
    let minimized = std::env::args().any(|arg| arg == MINIMIZED);
    let Some(file) = app.path().app_config_dir().ok().map(|dir| dir.join(UPDATE_MARK)) else {
        return minimized;
    };
    let Ok(text) = std::fs::read_to_string(&file) else {
        return minimized;
    };
    let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    let _ = std::fs::remove_file(&file);
    if !age.is_some_and(|age| age < UPDATE_MARK_AGE) {
        return minimized;
    }
    let hidden = text == "cachee";
    eprintln!("[maj] relancée après la mise à jour : fenêtre {}", if hidden { "cachée" } else { "affichée" });
    hidden
}

/// Ni sélection, ni partie, ni file d'attente, ni fin de partie : une mise à jour peut relancer l'app.
fn quiet(snap: &lol::Snapshot) -> bool {
    let phase = !snap.client_found || matches!(snap.phase.as_deref(), Some("None" | "Lobby"));
    phase && snap.runes.is_none() && snap.game.is_none() && !snap.recording
}


/// Montre la fenêtre (cachée dans la zone de notification, ou réduite).
fn show_window(app: &AppHandle, focus: bool) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        if focus {
            let _ = window.set_focus();
        }
    }
}

/// Taille de départ : celle de tauri.conf.json (deux colonnes), réduite si l'écran est plus petit
/// (au plus 90 % de la place libre, barre des tâches exclue), puis centrée.
fn fit_to_screen(window: &tauri::WebviewWindow) {
    let (Ok(Some(monitor)), Ok(size)) = (window.current_monitor(), window.inner_size()) else {
        return;
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area().size.to_logical::<f64>(scale);
    let size = size.to_logical::<f64>(scale);
    let (width, height) = (area.width * 0.9, area.height * 0.9);
    // Assez de place : la fenêtre garde la taille de tauri.conf.json, déjà centrée.
    if size.width <= width && size.height <= height {
        return;
    }
    let _ = window.set_size(tauri::LogicalSize::new(size.width.min(width), size.height.min(height)));
    let _ = window.center();
}

/// Icône près de l'horloge : clic gauche pour afficher, clic droit pour le menu.
fn create_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Afficher", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("LoL Companion")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app, true),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_window(tray.app_handle(), true);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let assets = lol::Assets::new();
    let icons = assets.clone();
    tauri::Builder::default()
        // En premier : relancer l'app (menu Démarrer...) remontre la fenêtre déjà ouverte.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_window(app, true)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Latest(Mutex::new(lol::Snapshot::default())))
        .manage(Opponent(Mutex::new(None)))
        .manage(ChosenForm(Mutex::new(None)))
        .manage(ChosenPage(Mutex::new(None)))
        .manage(PendingUpdate(Mutex::new(None)))
        .manage(assets.clone())
        .manage(search::Search::new())
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_opponent,
            set_form,
            set_rune_page,
            get_settings,
            set_settings,
            install_update,
            match_details,
            search_player,
            searched_match_details,
            champions,
            champion_page,
            friends
        ])
        // Images lues dans le client : lcu://localhost/<numéro du champion>, lcu://localhost/item-<numéro de l'objet>,
        // lcu://localhost/splash-<numéro du skin> (illustration), lcu://localhost/perk-<numéro de la rune ou de la voie>,
        // lcu://localhost/spell-<numéro du sort>, lcu://localhost/rank-<rang en minuscules> (emblème),
        // lcu://localhost/gold (icône de l'or), lcu://localhost/profile-<numéro de l'icône de profil>,
        // lcu://localhost/ability-<numéro du champion>-<q, w, e ou r> (compétence),
        // lcu://localhost/loadscreen-<numéro du skin> (illustration d'écran de chargement, en portrait).
        .register_asynchronous_uri_scheme_protocol("lcu", move |_ctx, request, responder| {
            let icons = icons.clone();
            let path = request.uri().path().trim_start_matches('/').to_string();
            tauri::async_runtime::spawn(async move {
                let number = |text: &str| text.parse::<i64>().ok();
                let image = if let Some(item) = path.strip_prefix("item-") {
                    match number(item) {
                        Some(id) => icons.item_icon(id).await.map(|png| (png, "image/png")),
                        None => None,
                    }
                } else if let Some(perk) = path.strip_prefix("perk-") {
                    match number(perk) {
                        Some(id) => icons.perk_icon(id).await.map(|png| (png, "image/png")),
                        None => None,
                    }
                } else if let Some(spell) = path.strip_prefix("spell-") {
                    match number(spell) {
                        Some(id) => icons.spell_icon(id).await.map(|png| (png, "image/png")),
                        None => None,
                    }
                } else if let Some(tier) = path.strip_prefix("rank-") {
                    icons.rank_emblem(tier).await.map(|png| (png, "image/png"))
                } else if path == "gold" {
                    icons.gold_icon().await.map(|png| (png, "image/png"))
                } else if let Some((champion, key)) = path.strip_prefix("ability-").and_then(|rest| rest.split_once('-')) {
                    match number(champion) {
                        Some(id) => icons.ability_icon(id, key).await.map(|png| (png, "image/png")),
                        None => None,
                    }
                } else if let Some(icon) = path.strip_prefix("profile-") {
                    match number(icon) {
                        Some(id) => icons.profile_icon(id).await.map(|jpg| (jpg, "image/jpeg")),
                        None => None,
                    }
                } else if let Some(skin) = path.strip_prefix("splash-") {
                    match number(skin).filter(|id| *id > 0) {
                        Some(id) => icons.splash(id).await.map(|jpg| (jpg, "image/jpeg")),
                        None => None,
                    }
                } else if let Some(skin) = path.strip_prefix("loadscreen-") {
                    match number(skin).filter(|id| *id > 0) {
                        Some(id) => icons.load_screen(id).await.map(|jpg| (jpg, "image/jpeg")),
                        None => None,
                    }
                } else {
                    match path.parse::<u32>() {
                        Ok(id) => icons.champion_icon(id).await.map(|png| (png, "image/png")),
                        Err(_) => None,
                    }
                };
                let response = match image {
                    Some((bytes, kind)) => tauri::http::Response::builder().header("Content-Type", kind).body(bytes),
                    None => tauri::http::Response::builder().status(404).body(Vec::new()),
                };
                responder.respond(response.expect("réponse HTTP invalide"));
            });
        })
        // La croix cache la fenêtre (l'app reste près de l'horloge), si les réglages le demandent.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let to_tray = app.state::<Prefs>().settings.lock().unwrap().close_to_tray;
                if to_tray && app.tray_by_id("main").is_some() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            let file = app.path().app_config_dir()?.join("reglages.json");
            let mut settings = Settings::load(&file);
            let startup = if cfg!(debug_assertions) { None } else { startup_entry(app.handle()) };
            // Lancement avec Windows activé par défaut, une seule fois : ensuite, seul ton choix dans les réglages
            // compte. Déjà activé, sa commande est réécrite à chaque ouverture : elle remplace une ancienne qui
            // lancerait autre chose que cet exe (exe de développement, ancien dossier, chemin sans guillemets).
            if let Some(launch) = &startup {
                let first = !settings.startup_default_applied;
                if first || launch.is_enabled().unwrap_or(false) {
                    match launch.enable() {
                        Ok(()) if first => {
                            eprintln!("[app] lancement avec Windows activé (par défaut)");
                            settings.start_with_windows = true;
                            settings.startup_default_applied = true;
                            if let Err(e) = settings.save(&file) {
                                eprintln!("[app] {e}");
                            }
                        }
                        Ok(()) => {}
                        Err(e) => eprintln!("[app] lancement avec Windows impossible à écrire ({e}) : réessayé au prochain démarrage"),
                    }
                }
            }
            app.manage(Startup(startup));
            app.manage(Prefs { file, settings: Mutex::new(settings) });
            let data_dir = app.path().app_data_dir()?;

            if let Err(e) = create_tray(app) {
                eprintln!("[app] icône près de l'horloge impossible : {e}");
            }
            if let Some(window) = app.get_webview_window("main") {
                fit_to_screen(&window);
            }
            // Fenêtre cachée au lancement avec Windows (--minimized), affichée sinon ; après une mise à jour,
            // comme elle était avant.
            if !start_hidden(app.handle()) {
                show_window(app.handle(), true);
            }

            // Mises à jour de la version installée : au démarrage puis toutes les 3 h (pas pendant le
            // développement, sauf avec UPDATE_URL pour les tests). Installées plus bas, hors sélection et hors partie.
            let test_url = std::env::var("UPDATE_URL").ok().filter(|_| cfg!(debug_assertions));
            if !cfg!(debug_assertions) || test_url.is_some() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        let builder = handle.updater_builder();
                        let builder = match test_url.as_deref().map(str::parse) {
                            Some(Ok(url)) => builder.endpoints(vec![url]),
                            _ => Ok(builder),
                        };
                        let found = match builder.and_then(|b| b.build()) {
                            Ok(updater) => updater.check().await,
                            Err(e) => Err(e),
                        };
                        let wait = match found {
                            Ok(Some(update)) => {
                                eprintln!("[maj] version {} disponible", update.version);
                                *handle.state::<PendingUpdate>().0.lock().unwrap() = Some(update);
                                UPDATE_EVERY
                            }
                            Ok(None) => {
                                eprintln!("[maj] l'app est à jour");
                                UPDATE_EVERY
                            }
                            Err(e) => {
                                eprintln!("[maj] vérification impossible : {e}");
                                UPDATE_RETRY
                            }
                        };
                        tokio::time::sleep(wait).await;
                    }
                });
            }

            // Fenêtres posées sur le jeu : elles suivent la touche Tab, regardée à part, bien plus souvent.
            let ready = Arc::new(overlay::Ready::default());
            overlay::watch_tab(app.handle().clone(), ready.clone());

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut tracker = lol::Tracker::new(assets, data_dir);
                let mut previous = lol::Snapshot::default();
                // Installation automatique : calme depuis quand, essais faits, dernier essai.
                let mut quiet_since: Option<Instant> = None;
                let mut tries = 0;
                let mut last_try: Option<Instant> = None;
                loop {
                    let manual = *handle.state::<Opponent>().0.lock().unwrap();
                    let form = *handle.state::<ChosenForm>().0.lock().unwrap();
                    let page = handle.state::<ChosenPage>().0.lock().unwrap().clone();
                    let settings = handle.state::<Prefs>().settings.lock().unwrap().clone();
                    let mut snap = tracker.poll(manual, form, page, &settings).await;
                    snap.update = handle.state::<PendingUpdate>().0.lock().unwrap().as_ref().map(|u| u.version.clone());
                    // Nouvelle version : installée toute seule après une minute ni en sélection ni en partie,
                    // fenêtre au premier plan ou non.
                    if snap.update.is_some() && !INSTALLING.load(Ordering::SeqCst) {
                        quiet_since = if quiet(&snap) { quiet_since.or_else(|| Some(Instant::now())) } else { None };
                        let due = quiet_since.is_some_and(|t| t.elapsed() >= UPDATE_QUIET)
                            && tries < UPDATE_TRIES
                            && last_try.is_none_or(|t| t.elapsed() >= UPDATE_GAP);
                        if due {
                            tries += 1;
                            last_try = Some(Instant::now());
                            let app = handle.clone();
                            tauri::async_runtime::spawn(async move {
                                eprintln!("[maj] installation automatique (ni sélection ni partie)");
                                if let Err(e) = install_update(app).await {
                                    eprintln!("[maj] {e}");
                                }
                            });
                        }
                    }
                    if snap.runes.is_none() {
                        // Hors sélection des champions : le choix fait d'un clic est oublié.
                        *handle.state::<Opponent>().0.lock().unwrap() = None;
                        *handle.state::<ChosenForm>().0.lock().unwrap() = None;
                        *handle.state::<ChosenPage>().0.lock().unwrap() = None;
                    }
                    // Le client LoL vient de s'ouvrir : on montre la fenêtre (sans lui voler le clavier).
                    if snap.client_found && !previous.client_found && settings.show_on_client_start {
                        show_window(&handle, false);
                    }
                    // Partie trouvée : notification Windows (l'app n'accepte rien à ta place).
                    if snap.phase.as_deref() == Some("ReadyCheck") && previous.phase != snap.phase && settings.notify_match_found {
                        let shown = handle
                            .notification()
                            .builder()
                            .title("Partie trouvée !")
                            .body("Accepte-la dans le client League of Legends.")
                            .show();
                        if let Err(e) = shown {
                            eprintln!("[app] notification impossible : {e}");
                        }
                    }
                    if snap != previous {
                        *handle.state::<Latest>().0.lock().unwrap() = snap.clone();
                        let _ = handle.emit("lol-state", &snap);
                    }
                    // En partie : le panneau des objets (en tenant Tab, ou toute la partie) et les écarts d'or
                    // (en tenant Tab).
                    let started = snap.game.as_ref().filter(|g| g.ready);
                    let build = settings.show_build_overlay && started.is_some_and(|g| !g.panel.is_empty());
                    let gold = settings.show_gold_overlay && started.is_some();
                    for (wanted, which) in [(build, Overlay::Build), (gold, Overlay::Gold)] {
                        if wanted {
                            overlay::prepare(&handle, which);
                        } else if snap.game.is_none() {
                            // Plus de partie : la fenêtre est fermée, sa page ne garde plus de mémoire.
                            overlay::close(&handle, which);
                        }
                    }
                    ready.build.store(build, Ordering::Relaxed);
                    ready.gold.store(gold, Ordering::Relaxed);
                    ready.build_always.store(settings.build_overlay_always, Ordering::Relaxed);
                    ready.in_game.store(snap.game.is_some(), Ordering::Relaxed);
                    let delay = lol::Tracker::next_delay(&snap);
                    previous = snap;
                    tokio::time::sleep(delay).await;
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
