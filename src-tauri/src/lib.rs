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
    sync::{atomic::Ordering, Arc, Mutex},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt as _;
use tauri_plugin_updater::UpdaterExt as _;

/// Dernier état envoyé à l'interface.
struct Latest(Mutex<lol::Snapshot>);
/// Adversaire direct choisi d'un clic dans l'interface (None = deviné par l'app).
struct Opponent(Mutex<Option<i64>>);
/// Réglages, et le fichier où ils sont gardés.
struct Prefs {
    file: PathBuf,
    settings: Mutex<Settings>,
}
/// Nouvelle version trouvée au démarrage, prête à installer.
struct PendingUpdate(Mutex<Option<tauri_plugin_updater::Update>>);

#[tauri::command]
fn get_state(latest: tauri::State<'_, Latest>) -> lol::Snapshot {
    latest.0.lock().unwrap().clone()
}

#[tauri::command]
fn set_opponent(opponent: tauri::State<'_, Opponent>, champion_id: Option<i64>) {
    *opponent.0.lock().unwrap() = champion_id;
}

#[tauri::command]
fn get_settings(app: AppHandle, prefs: tauri::State<'_, Prefs>) -> Settings {
    let mut settings = prefs.settings.lock().unwrap().clone();
    settings.start_with_windows = app.autolaunch().is_enabled().unwrap_or(false);
    settings
}

#[tauri::command]
fn set_settings(app: AppHandle, prefs: tauri::State<'_, Prefs>, settings: Settings) -> Result<Settings, String> {
    let autolaunch = app.autolaunch();
    if settings.start_with_windows != autolaunch.is_enabled().unwrap_or(false) {
        let done = if settings.start_with_windows { autolaunch.enable() } else { autolaunch.disable() };
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

/// Salon d'un autre joueur, cherché par son Riot ID (Nom#TAG) et sa région.
#[tauri::command]
async fn search_player(
    assets: tauri::State<'_, lol::Assets>,
    search: tauri::State<'_, search::Search>,
    riot_id: String,
    region: String,
) -> Result<search::Searched, String> {
    search.find(&assets, &riot_id, &region).await
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
    let update = app.state::<PendingUpdate>().0.lock().unwrap().clone();
    let Some(update) = update else {
        return Err("Aucune mise à jour à installer.".to_string());
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("Mise à jour impossible : {e}"))?;
    app.restart()
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
        // Lancée avec Windows : elle démarre cachée dans la zone de notification.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Latest(Mutex::new(lol::Snapshot::default())))
        .manage(Opponent(Mutex::new(None)))
        .manage(PendingUpdate(Mutex::new(None)))
        .manage(assets.clone())
        .manage(search::Search::new())
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_opponent,
            get_settings,
            set_settings,
            install_update,
            match_details,
            search_player,
            searched_match_details
        ])
        // Images lues dans le client : lcu://localhost/<numéro du champion>, lcu://localhost/item-<numéro de l'objet>,
        // lcu://localhost/splash-<numéro du skin> (illustration), lcu://localhost/perk-<numéro de la rune ou de la voie>,
        // lcu://localhost/spell-<numéro du sort>, lcu://localhost/rank-<rang en minuscules> (emblème),
        // lcu://localhost/gold (icône de l'or).
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
                } else if let Some(skin) = path.strip_prefix("splash-") {
                    match number(skin).filter(|id| *id > 0) {
                        Some(id) => icons.splash(id).await.map(|jpg| (jpg, "image/jpeg")),
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
            let settings = Settings::load(&file);
            app.manage(Prefs { file, settings: Mutex::new(settings) });
            let data_dir = app.path().app_data_dir()?;

            if let Err(e) = create_tray(app) {
                eprintln!("[app] icône près de l'horloge impossible : {e}");
            }
            if let Some(window) = app.get_webview_window("main") {
                fit_to_screen(&window);
            }
            // Fenêtre cachée au lancement avec Windows (--minimized), affichée sinon.
            if !std::env::args().any(|arg| arg == "--minimized") {
                show_window(app.handle(), true);
            }

            // Mises à jour : vérifiées au démarrage de la version installée (pas pendant le développement).
            if !cfg!(debug_assertions) {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let found = match handle.updater() {
                        Ok(updater) => updater.check().await,
                        Err(e) => Err(e),
                    };
                    match found {
                        Ok(Some(update)) => {
                            eprintln!("[maj] version {} disponible", update.version);
                            *handle.state::<PendingUpdate>().0.lock().unwrap() = Some(update);
                        }
                        Ok(None) => eprintln!("[maj] l'app est à jour"),
                        Err(e) => eprintln!("[maj] vérification impossible : {e}"),
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
                loop {
                    let manual = *handle.state::<Opponent>().0.lock().unwrap();
                    let settings = handle.state::<Prefs>().settings.lock().unwrap().clone();
                    let mut snap = tracker.poll(manual, &settings).await;
                    snap.update = handle.state::<PendingUpdate>().0.lock().unwrap().as_ref().map(|u| u.version.clone());
                    if snap.runes.is_none() {
                        // Hors sélection des champions : le choix fait d'un clic est oublié.
                        *handle.state::<Opponent>().0.lock().unwrap() = None;
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
                    // En partie, Tab fait apparaître le panneau des objets (tant qu'il en reste à acheter)
                    // et les écarts d'or.
                    let started = snap.game.as_ref().filter(|g| g.ready);
                    let build = settings.show_build_overlay && started.is_some_and(|g| !g.panel.is_empty());
                    let gold = settings.show_gold_overlay && started.is_some();
                    for (wanted, which) in [(build, Overlay::Build), (gold, Overlay::Gold)] {
                        if wanted {
                            overlay::prepare(&handle, which);
                        }
                    }
                    ready.build.store(build, Ordering::Relaxed);
                    ready.gold.store(gold, Ordering::Relaxed);
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
