//! Panneau des objets à acheter, posé sur le bord gauche de l'écran du jeu, à mi-hauteur.
//! Fenêtre transparente, toujours au-dessus, que les clics traversent et qui ne prend jamais le clavier.
//! Elle ne se voit que si LoL est en mode « sans bordure » : en plein écran, le jeu passe par-dessus tout.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const LABEL: &str = "overlay";
/// Taille de la fenêtre du panneau, en part de la hauteur de l'écran du jeu :
/// le panneau garde les mêmes proportions en 1080p, en 1440p...
const WIDTH: f64 = 0.16;
const HEIGHT: f64 = 0.34;

/// Rectangle en pixels de l'écran : (x, y, largeur, hauteur).
type Rect = (i32, i32, i32, i32);

/// Montre le panneau sur le jeu (`wanted`), ou le cache.
/// Il reste caché tant que le jeu n'est pas la fenêtre active (Alt+Tab, clic sur l'autre écran).
pub fn update(app: &AppHandle, wanted: bool) {
    let window = app.get_webview_window(LABEL);
    let Some(screen) = (if wanted { game_screen(app) } else { None }) else {
        if let Some(window) = window {
            hide(&window);
        }
        return;
    };
    let window = match window {
        Some(window) => window,
        None => match create(app) {
            Ok(window) => window,
            Err(e) => {
                eprintln!("[panneau] fenêtre impossible : {e}");
                return;
            }
        },
    };
    show(&window, panel(screen));
}

/// Le panneau : collé au bord gauche de l'écran du jeu, centré en hauteur.
fn panel((x, y, _, height): Rect) -> Rect {
    let (w, h) = ((f64::from(height) * WIDTH) as i32, (f64::from(height) * HEIGHT) as i32);
    (x, y + (height - h) / 2, w, h)
}

fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
        .title("LoL Companion · objets")
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
