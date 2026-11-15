//! Fenêtres posées sur le jeu : transparentes, toujours au-dessus, que les clics traversent et qui ne prennent
//! jamais le clavier. Elles ne se voient que si LoL est en mode « sans bordure » : en plein écran, le jeu passe
//! par-dessus tout.
//! - Le panneau des objets à acheter, sur le bord gauche de l'écran du jeu, à mi-hauteur.
//! - Les écarts d'or, à gauche du tableau des scores, tant que Tab est tenu.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

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
            // Collé au bord gauche, centré en hauteur ; 172 × 367 en 1080p.
            Self::Build => {
                let (w, h) = (part(0.16), part(0.34));
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

/// Montre une fenêtre sur le jeu (`wanted`), ou la cache.
/// Elle reste cachée tant que le jeu n'est pas la fenêtre active (Alt+Tab, clic sur l'autre écran).
pub fn update(app: &AppHandle, overlay: Overlay, wanted: bool) {
    place(app, overlay, if wanted { game_screen(app) } else { None });
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

/// Écarts d'or : affichés tant que Tab est tenu (sans Alt), jeu au premier plan.
/// `ready` : une partie est en cours et le réglage est actif.
pub fn watch_scoreboard(app: AppHandle, ready: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut shown: Option<Rect> = None;
        loop {
            let ready = ready.load(Ordering::Relaxed);
            let screen = if ready && scoreboard_key_held() { game_screen(&app) } else { None };
            if screen != shown {
                place(&app, Overlay::Gold, screen);
                shown = screen;
            }
            std::thread::sleep(if ready { TAB_POLL } else { IDLE_POLL });
        }
    });
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
