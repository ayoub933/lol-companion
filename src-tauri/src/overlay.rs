//! Fenêtres posées sur le jeu, jeu au premier plan : transparentes, toujours au-dessus, que les clics traversent
//! et qui ne prennent jamais le clavier. Elles ne se voient que si LoL est en mode « sans bordure » : en plein
//! écran, le jeu passe par-dessus tout.
//! - Le panneau des objets à acheter, sur le bord gauche de l'écran du jeu, à mi-hauteur : en tenant Tab, ou
//!   toute la partie (réglage).
//! - Les écarts d'or, juste à gauche du tableau des scores : en tenant Tab (avec le tableau des scores).
//!
//! Créées au début de la partie, cachées entre deux appuis sur Tab, fermées une fois la partie finie.
//!
//! Ctrl+X en partie, jeu au premier plan : la fenêtre de l'app vient par-dessus le jeu, au milieu (les 2/3 de sa
//! hauteur) et sans bordure ; Ctrl+X encore (ou la partie finie) : elle revient où et comme elle était, et le jeu
//! reprend le clavier. Les touches sont seulement lues : Ctrl+X n'est pris à aucune autre app.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Rectangle en pixels de l'écran : (x, y, largeur, hauteur).
type Rect = (i32, i32, i32, i32);

/// Touche Tab regardée à ce rythme pendant une partie (le tableau des scores s'ouvre tant qu'elle est tenue).
const TAB_POLL: Duration = Duration::from_millis(30);
/// Hors partie : juste de quoi voir la partie commencer.
const IDLE_POLL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy)]
pub enum Overlay {
    /// Objets à acheter.
    Build,
    /// Écarts d'or à côté du tableau des scores.
    Gold,
}

impl Overlay {
    fn label(self) -> &'static str {
        match self {
            Self::Build => "overlay",
            Self::Gold => "gold",
        }
    }

    fn page(self) -> &'static str {
        match self {
            Self::Build => "overlay.html",
            Self::Gold => "gold.html",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Build => "LoL Companion · objets",
            Self::Gold => "LoL Companion · écarts d'or",
        }
    }

    /// Place de la fenêtre sur l'écran du jeu. Les tailles sont en part de la hauteur de l'écran (l'interface
    /// du jeu suit cette hauteur) : la même place en 1080p, en 1440p...
    fn rect(self, (x, y, width, height): Rect) -> Rect {
        let part = |share: f64| (f64::from(height) * share).round() as i32;
        match self {
            // Collé au bord gauche, centré en hauteur ; 194 × 367 en 1080p (numéros, puis jusqu'à 4 icônes).
            Self::Build => {
                let (w, h) = (part(0.18), part(0.34));
                (x, y + (height - h) / 2, w, h)
            }
            // Le tableau des scores est centré et va de 380 à 1537 en 1920 × 1080 (relevé sur une capture) :
            // la fenêtre se met juste à sa gauche, de 224 à 374, et de 238 à 702 en hauteur.
            Self::Gold => {
                let right = x + width / 2 - part(586.0 / 1080.0);
                let (w, h) = (part(150.0 / 1080.0), part(464.0 / 1080.0));
                (right - w, y + part(238.0 / 1080.0), w, h)
            }
        }
    }
}

/// Ce que chaque fenêtre a à montrer (réglage actif, partie en cours...) : la boucle principale le tient à jour.
#[derive(Default)]
pub struct Ready {
    pub build: AtomicBool,
    pub gold: AtomicBool,
    /// Panneau des objets affiché toute la partie, sans tenir Tab.
    pub build_always: AtomicBool,
    /// Partie en cours (écran de chargement compris) : Ctrl+X met l'app par-dessus le jeu.
    pub in_game: AtomicBool,
    /// L'app est par-dessus le jeu (Ctrl+X) : elle montre les 10 joueurs en cartes (événement « panel » à sa page).
    pub panel: AtomicBool,
}

/// Fenêtre de l'app juste avant Ctrl+X, pour la remettre pareil.
struct Saved {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    maximized: bool,
    minimized: bool,
    visible: bool,
}

/// Crée la fenêtre à l'avance (cachée), pour qu'elle s'affiche tout de suite le moment venu.
pub fn prepare(app: &AppHandle, overlay: Overlay) {
    if app.get_webview_window(overlay.label()).is_some() {
        return;
    }
    if let Err(e) = create(app, overlay) {
        eprintln!("[panneau] fenêtre impossible : {e}");
    }
}

/// Ferme la fenêtre une fois la partie finie : sa page ne reste pas en mémoire jusqu'à la prochaine partie, où
/// `prepare` la recrée. `destroy` et non `close` : la croix de l'app peut cacher une fenêtre au lieu de la fermer.
pub fn close(app: &AppHandle, overlay: Overlay) {
    let Some(window) = app.get_webview_window(overlay.label()) else {
        return;
    };
    match window.destroy() {
        Ok(()) => eprintln!("[panneau] fenêtre « {} » fermée (partie finie)", overlay.label()),
        Err(e) => eprintln!("[panneau] fermeture impossible : {e}"),
    }
}

/// Les fenêtres prêtes s'affichent tant que Tab est tenue (sans Alt), jeu au premier plan, ou tout le temps pour
/// le panneau des objets si le réglage le demande ; elles restent cachées le reste du temps (Alt+Tab, clic sur
/// l'autre écran...).
pub fn watch_tab(app: AppHandle, ready: Arc<Ready>) {
    std::thread::spawn(move || {
        let mut shown: [Option<Rect>; 2] = [None, None];
        // App par-dessus le jeu (Ctrl+X) : où elle était avant ; Ctrl+X tenues au tour d'avant.
        let mut saved: Option<Saved> = None;
        let mut held = false;
        loop {
            let in_game = ready.in_game.load(Ordering::Relaxed);
            let pressed = in_game && panel_keys_held();
            if pressed && !held {
                saved = match saved.take() {
                    None => game_screen(&app).and_then(|screen| open_panel(&app, screen)),
                    Some(before) if app_or_game_in_front(&app) => {
                        close_panel(&app, before);
                        None
                    }
                    Some(before) => Some(before),
                };
            }
            held = pressed;
            // Partie finie : l'app revient à sa place.
            if let Some(before) = saved.take_if(|_| !in_game) {
                close_panel(&app, before);
            }
            if ready.panel.swap(saved.is_some(), Ordering::Relaxed) != saved.is_some() {
                let _ = app.emit("panel", saved.is_some());
            }
            let wanted = [ready.build.load(Ordering::Relaxed), ready.gold.load(Ordering::Relaxed)];
            let always = [ready.build_always.load(Ordering::Relaxed), false];
            let active = wanted.contains(&true);
            let tab = active && scoreboard_key_held();
            let screen = if tab || (wanted[0] && always[0]) { game_screen(&app) } else { None };
            for (i, overlay) in [Overlay::Build, Overlay::Gold].into_iter().enumerate() {
                let target = screen.filter(|_| wanted[i] && (tab || always[i]));
                if target != shown[i] {
                    place(&app, overlay, target);
                    shown[i] = target;
                }
            }
            std::thread::sleep(if active || in_game { TAB_POLL } else { IDLE_POLL });
        }
    });
}

/// L'app par-dessus le jeu (Ctrl+X) : au milieu, aux 2/3 de sa hauteur, en 16:9 (1280 × 720 sur un jeu en
/// 1920 × 1080) ; pas moins de 576 pixels de haut à 100 % (en dessous, l'app passerait sur une seule colonne).
const PANEL_SHARE: f64 = 2.0 / 3.0;
const PANEL_MIN_HEIGHT: f64 = 576.0;

/// Place de l'app par-dessus le jeu, dans sa fenêtre `game` (en pixels de l'écran).
fn panel_rect(app: &AppHandle, game: Rect) -> Rect {
    let (x, y, w, h) = game;
    let height = (h as f64 * PANEL_SHARE).max(PANEL_MIN_HEIGHT * scale_at(app, game)).min(h as f64);
    let width = (height * 16.0 / 9.0).min(w as f64);
    let (width, height) = (width.round() as i32, height.round() as i32);
    (x + (w - width) / 2, y + (h - height) / 2, width, height)
}

/// Échelle de l'écran du jeu (1,5 à 150 %) ; 1 si inconnue.
fn scale_at(app: &AppHandle, (x, y, w, h): Rect) -> f64 {
    let (cx, cy) = (x + w / 2, y + h / 2);
    let monitors = app.available_monitors().unwrap_or_default();
    let screen = monitors.iter().find(|m| {
        let (p, s) = (m.position(), m.size());
        (p.x..p.x + s.width as i32).contains(&cx) && (p.y..p.y + s.height as i32).contains(&cy)
    });
    screen.map_or(1.0, |m| m.scale_factor())
}

/// L'app par-dessus le jeu : sans bordure, toujours au-dessus, au milieu de sa fenêtre (voir PANEL_SHARE) ; elle
/// prend le clavier. Renvoie où elle était (None si elle n'existe pas).
fn open_panel(app: &AppHandle, game: Rect) -> Option<Saved> {
    let (x, y, w, h) = panel_rect(app, game);
    let window = app.get_webview_window("main")?;
    let maximized = window.is_maximized().unwrap_or(false);
    let minimized = window.is_minimized().unwrap_or(false);
    let visible = window.is_visible().unwrap_or(false);
    // Réduite ou agrandie : d'abord sa taille normale, pour la noter (réduite, sa place ne veut rien dire).
    if minimized || maximized {
        let _ = window.unminimize();
        let _ = window.unmaximize();
        std::thread::sleep(Duration::from_millis(150));
    }
    let saved = Saved { position: window.outer_position().ok()?, size: window.inner_size().ok()?, maximized, minimized, visible };
    let _ = window.set_decorations(false);
    let _ = window.set_always_on_top(true);
    let _ = window.set_position(PhysicalPosition::new(x, y));
    let _ = window.set_size(PhysicalSize::new(w.max(1) as u32, h.max(1) as u32));
    let _ = window.show();
    let _ = window.set_focus();
    eprintln!("[panneau] app par-dessus le jeu ({w} × {h}), Ctrl+X pour la remettre");
    Some(saved)
}

/// L'app revient où et comme elle était, et le jeu reprend le clavier.
fn close_panel(app: &AppHandle, saved: Saved) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.set_always_on_top(false);
    let _ = window.set_decorations(true);
    // D'abord sur son écran, puis sa taille (en pixels de cet écran).
    let _ = window.set_position(saved.position);
    let _ = window.set_size(saved.size);
    if saved.maximized {
        let _ = window.maximize();
    }
    if saved.minimized {
        let _ = window.minimize();
    }
    if !saved.visible {
        let _ = window.hide();
    }
    focus_game();
    eprintln!("[panneau] app remise à sa place");
}

/// Pose la fenêtre sur cet écran du jeu, ou la cache (None).
fn place(app: &AppHandle, overlay: Overlay, screen: Option<Rect>) {
    let window = app.get_webview_window(overlay.label());
    let Some(screen) = screen else {
        if let Some(window) = window {
            hide(&window);
        }
        return;
    };
    let window = match window {
        Some(window) => window,
        None => match create(app, overlay) {
            Ok(window) => window,
            Err(e) => {
                eprintln!("[panneau] fenêtre impossible : {e}");
                return;
            }
        },
    };
    show(&window, overlay.rect(screen));
}

fn create(app: &AppHandle, overlay: Overlay) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, overlay.label(), WebviewUrl::App(overlay.page().into()))
        .title(overlay.title())
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()?;
    // Les clics passent au travers, jusqu'au jeu (hors Windows : seulement une fois la fenêtre affichée).
    #[cfg(windows)]
    window.set_ignore_cursor_events(true)?;
    Ok(window)
}

/// Écran du jeu : la fenêtre de LoL (classe « RiotWindowClass »), seulement si c'est la fenêtre active.
#[cfg(windows)]
fn game_screen(_app: &AppHandle) -> Option<Rect> {
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowRect},
    };
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return None;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(window, class.as_mut_ptr(), class.len() as i32);
        if String::from_utf16_lossy(&class[..len.max(0) as usize]) != "RiotWindowClass" {
            return None;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(window, &mut r) == 0 {
            return None;
        }
        Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
    }
}

/// Ctrl+X enfoncées, sans Alt. Seulement lues : elles arrivent à la fenêtre active comme d'habitude.
#[cfg(windows)]
fn panel_keys_held() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_X};
    let down = |key: u16| unsafe { GetAsyncKeyState(i32::from(key)) } < 0;
    down(VK_CONTROL) && down(VK_X) && !down(VK_MENU)
}

/// Fenêtre active : l'app ou le jeu (Ctrl+X tapé dans une autre app ne remet pas l'app à sa place).
#[cfg(windows)]
fn app_or_game_in_front(app: &AppHandle) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow};
    unsafe {
        let front = GetForegroundWindow();
        if front.is_null() {
            return false;
        }
        let ours = app.get_webview_window("main").and_then(|w| w.hwnd().ok());
        if ours.map(|hwnd| hwnd.0 as windows_sys::Win32::Foundation::HWND) == Some(front) {
            return true;
        }
        let mut class = [0u16; 64];
        let len = GetClassNameW(front, class.as_mut_ptr(), class.len() as i32);
        String::from_utf16_lossy(&class[..len.max(0) as usize]) == "RiotWindowClass"
    }
}

/// Rend le clavier au jeu (fenêtre de classe « RiotWindowClass »).
#[cfg(windows)]
fn focus_game() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow};
    let class: Vec<u16> = "RiotWindowClass\0".encode_utf16().collect();
    unsafe {
        let game = FindWindowW(class.as_ptr(), std::ptr::null());
        if !game.is_null() {
            SetForegroundWindow(game);
        }
    }
}

/// Tab enfoncée, sans Alt (Alt+Tab sert à changer de fenêtre). L'état de la touche est seulement lu :
/// elle arrive au jeu comme d'habitude.
#[cfg(windows)]
fn scoreboard_key_held() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU, VK_TAB};
    // Bit de poids fort à 1 (valeur négative) : touche enfoncée en ce moment.
    let down = |key: u16| unsafe { GetAsyncKeyState(i32::from(key)) } < 0;
    down(VK_TAB) && !down(VK_MENU)
}

/// Mis au-dessus de tout, puis affiché sans prendre le clavier au jeu.
#[cfg(windows)]
fn show(window: &WebviewWindow, (x, y, w, h): Rect) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindowAsync, HWND_TOPMOST, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SW_SHOWNOACTIVATE,
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd = hwnd.0 as windows_sys::Win32::Foundation::HWND;
    unsafe {
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS);
        ShowWindowAsync(hwnd, SW_SHOWNOACTIVATE);
    }
}

#[cfg(windows)]
fn hide(window: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_HIDE};
    if let Ok(hwnd) = window.hwnd() {
        unsafe { ShowWindowAsync(hwnd.0 as windows_sys::Win32::Foundation::HWND, SW_HIDE) };
    }
}

/// Hors Windows (tests) : l'écran principal.
#[cfg(not(windows))]
fn game_screen(app: &AppHandle) -> Option<Rect> {
    let monitor = app.primary_monitor().ok()??;
    let (p, s) = (monitor.position(), monitor.size());
    Some((p.x, p.y, s.width as i32, s.height as i32))
}

/// Hors Windows (tests) : Ctrl+X comptent comme tenues tant que le fichier nommé par LOL_FAKE_CTRLX existe.
#[cfg(not(windows))]
fn panel_keys_held() -> bool {
    std::env::var_os("LOL_FAKE_CTRLX").is_some_and(|file| std::path::Path::new(&file).exists())
}

#[cfg(not(windows))]
fn app_or_game_in_front(_app: &AppHandle) -> bool {
    true
}

#[cfg(not(windows))]
fn focus_game() {}

/// Hors Windows (tests) : Tab compte comme tenue tant que le fichier nommé par LOL_FAKE_TAB existe.
#[cfg(not(windows))]
fn scoreboard_key_held() -> bool {
    std::env::var_os("LOL_FAKE_TAB").is_some_and(|file| std::path::Path::new(&file).exists())
}

#[cfg(not(windows))]
fn show(window: &WebviewWindow, (x, y, w, h): Rect) {
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = window.set_size(tauri::PhysicalSize::new(w as u32, h as u32));
    if !window.is_visible().unwrap_or(false) {
        let _ = window.show();
        let _ = window.set_ignore_cursor_events(true);
    }
}

#[cfg(not(windows))]
fn hide(window: &WebviewWindow) {
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    }
}
