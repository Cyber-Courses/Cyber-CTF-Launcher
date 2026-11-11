//! The app's secondary windows (Settings, machine setup, shells) and what they share: their
//! frame (the macOS overlaid title bar, none on Linux and Windows) and theme, attaching to the
//! main window, and fitting the screen.

use tauri::utils::config::WindowConfig;
use tauri::{Manager, Theme, WebviewUrl, WebviewWindowBuilder};

use crate::AppHandle;
use crate::error::{Error, Result};
use crate::runtime::Runtime;
use crate::terminal;

/// The main window's label.
pub const MAIN: &str = "main";

/// A window height of `wanted` logical pixels, or as much as the primary screen can show (with
/// room for the menu bar and dock) when that is less, so a tall dialog fits its content on a big
/// display and still opens fully on a small laptop screen, where its body scrolls instead.
pub fn height_fitting(app: &AppHandle, wanted: f64) -> f64 {
    let available = app.primary_monitor().ok().flatten().map(|m| f64::from(m.size().height) / m.scale_factor() - 80.0);
    fit_height(wanted, available)
}

/// `wanted`, or the screen's `available` height when that is less, never under 480.
fn fit_height(wanted: f64, available: Option<f64>) -> f64 {
    wanted.min(available.unwrap_or(wanted)).max(480.0)
}

/// Focuses the window `label`. False when it isn't open.
pub fn focus_existing(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label).map(|w| w.set_focus()).is_some()
}

/// Whether the system draws the app's windows' frame (title bar and borders): only on macOS,
/// where the page's own title bar sits under the traffic lights. On Linux the window manager drew
/// it from its own theme, whatever the window asked (xfwm4: a white frame around the dark app),
/// and Windows draws its buttons its own way: there each page draws the whole title bar
/// (`src/components/window-title-bar.tsx`). Without the frame, Tauri still resizes the window
/// from its edges, and on Windows keeps the system's shadow and thin border (`shadow`).
const SYSTEM_FRAME: bool = cfg!(target_os = "macos");

/// The windows tauri.conf.json opens at startup (the main one) get the app's frame: none on
/// Linux and Windows. Applied to the configuration before the app is built, so the window is
/// never shown with the system's frame first.
pub fn frame_configured(config: &mut tauri::Config) {
    for window in &mut config.app.windows {
        window.decorations &= SYSTEM_FRAME;
    }
}

/// The theme a new window opens with: the main window's, which its page keeps on the app's
/// appearance (Dark and Black dark, Light light: `src/lib/appearance.ts`), so the system's frame
/// matches the app from the start instead of being the system's (light on Windows) until the
/// page loads. Dark, the app's default, without a main window.
fn opening_theme(app: &AppHandle) -> Theme {
    app.get_webview_window(MAIN).and_then(|w| w.theme().ok()).unwrap_or(Theme::Dark)
}

/// The configuration of window `label` showing `url`: the app's frame (on macOS the page draws
/// its own title bar under the traffic lights, on Linux and Windows all of it) and the app's
/// theme, which the frame left (the taskbar's, the system's dialogs') still follows.
fn window_config(label: &str, url: String, theme: Theme) -> WindowConfig {
    WindowConfig {
        label: label.to_owned(),
        url: WebviewUrl::App(url.into()),
        decorations: SYSTEM_FRAME,
        // Windows: the shadow and thin border of an undecorated window (the default, said here).
        shadow: true,
        theme: Some(theme),
        #[cfg(target_os = "macos")]
        title_bar_style: tauri::TitleBarStyle::Overlay,
        #[cfg(target_os = "macos")]
        hidden_title: true,
        ..WindowConfig::default()
    }
}

/// A window with the app's look (see [`window_config`]), built from a configuration as the main
/// window is.
pub fn builder<'a>(app: &'a AppHandle, label: &str, url: impl Into<String>) -> tauri::Result<WebviewWindowBuilder<'a, crate::AppRuntime, AppHandle>> {
    WebviewWindowBuilder::from_config(app, &window_config(label, url.into(), opening_theme(app)))
}

/// `builder` attached to the main window when it is open (it stays above it and closes with it).
pub fn child_of_main<'a>(
    app: &'a AppHandle,
    builder: WebviewWindowBuilder<'a, crate::AppRuntime, AppHandle>,
) -> tauri::Result<WebviewWindowBuilder<'a, crate::AppRuntime, AppHandle>> {
    match app.get_webview_window(MAIN) {
        Some(main) => builder.parent(&main),
        None => Ok(builder),
    }
}

/// Opens `builder`'s window (from [`builder`]) attached to the main window (see
/// [`child_of_main`]); a failure says it couldn't open `what`.
pub fn open_over_main<'a>(app: &'a AppHandle, builder: tauri::Result<WebviewWindowBuilder<'a, crate::AppRuntime, AppHandle>>, what: &str) -> Result<()> {
    builder.and_then(|b| child_of_main(app, b)).and_then(WebviewWindowBuilder::build).map_err(|e| Error::Invalid(format!("could not open {what}: {e}")))?;
    Ok(())
}

/// Opens Settings in its own window (label `settings`), focusing it if already open. The window
/// loads the main entry with `?window=settings` so the frontend renders only the settings screen.
pub fn open_settings_window(app: &AppHandle) -> tauri::Result<()> {
    if focus_existing(app, "settings") {
        return Ok(());
    }
    // The root page in Settings mode. Not `index.html?...`: in `tauri dev` that resolves against
    // the Next dev server, which has no `/index.html` route (a 404 page), while the root serves
    // in both dev and the static export.
    let settings = builder(app, "settings", "?window=settings")?.title("Settings").inner_size(760.0, 640.0).min_inner_size(560.0, 480.0).resizable(true);
    child_of_main(app, settings)?.build()?;
    Ok(())
}

/// Opens (or focuses) lab `id`'s attack box shell window, titled after `title`; closing it ends
/// the shell it was showing.
pub fn open_shell(app: &AppHandle, id: &str, runtime: Runtime, title: &str) -> Result<()> {
    crate::runtime::validate_id(id)?;
    let (label, page) = terminal::shell_window(id, runtime);
    if focus_existing(app, &label) {
        return Ok(());
    }
    let fail = |e: tauri::Error| Error::Invalid(format!("could not open the shell window: {e}"));
    let window = builder(app, &label, page)
        .map_err(fail)?
        .title(format!("{} · attack box", terminal::window_title(title)))
        .inner_size(900.0, 560.0)
        .min_inner_size(480.0, 280.0)
        .resizable(true)
        .build()
        .map_err(fail)?;
    window.on_window_event(move |e| {
        if matches!(e, tauri::WindowEvent::Destroyed) {
            terminal::close_window(&label);
        }
    });
    Ok(())
}

/// Opens (or focuses) the screen window of `which`'s machine, titled after the lab's `title`: its
/// session's bridge bound first, and the window's content security policy letting it connect to
/// that bridge alone (`connect-src ws://127.0.0.1:<port>`); closing it ends the session.
pub async fn open_screen(app: &AppHandle, which: crate::screen::Which, title: &str) -> Result<()> {
    let (label, page) = crate::screen::window(&which)?;
    if focus_existing(app, &label) {
        return Ok(());
    }
    let machine = which.machine.clone();
    let port = crate::screen::start(&label, which).await?;
    let fail = |e: tauri::Error| Error::Invalid(format!("could not open the screen window: {e}"));
    let built = builder(app, &label, page).map(|b| {
        b.title(format!("{machine} · {}", terminal::window_title(title)))
            .inner_size(1100.0, 720.0)
            .min_inner_size(480.0, 320.0)
            .resizable(true)
            .on_web_resource_request(move |_, response| {
                let csp = response.headers().get("Content-Security-Policy").and_then(|v| v.to_str().ok()).map(|c| crate::screen::csp_with_bridge(c, port));
                if let Some(value) = csp.and_then(|c| tauri::http::HeaderValue::from_str(&c).ok()) {
                    response.headers_mut().insert("Content-Security-Policy", value);
                }
            })
    });
    let window = match built.and_then(tauri::WebviewWindowBuilder::build) {
        Ok(w) => w,
        Err(e) => {
            crate::screen::close(&label);
            return Err(fail(e));
        }
    };
    window.on_window_event(move |e| {
        if matches!(e, tauri::WindowEvent::Destroyed) {
            crate::screen::close(&label);
        }
    });
    Ok(())
}

/// Brings the main window back (it may be hidden while the app finishes in the background, or
/// minimized).
pub fn reveal_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::test_app::TestApp;

    #[test]
    fn settings_open_once_in_their_own_window() {
        // Without the main window: attaching to it needs a real one (Tauri's mock has none).
        let t = TestApp::new();
        open_settings_window(t.handle()).unwrap();
        let settings = t.handle().get_webview_window("settings").expect("the Settings window");
        assert!(settings.url().unwrap().as_str().ends_with("?window=settings"), "{}", settings.url().unwrap());
        // Asked again: the open one is focused, no second window.
        open_settings_window(t.handle()).unwrap();
        assert_eq!(t.handle().webview_windows().len(), 1);
        assert!(focus_existing(t.handle(), "settings") && !focus_existing(t.handle(), "nope"));
    }

    #[test]
    fn a_shell_window_opens_once_per_lab() {
        let t = TestApp::new();
        let open = |id: &str| open_shell(t.handle(), id, Runtime::Docker, "SQLi\n lab");
        assert_eq!(open("../x").unwrap_err().to_string(), "invalid lab id `../x`");
        assert!(t.handle().webview_windows().is_empty());
        open("sqli").unwrap();
        let w = t.handle().get_webview_window("shell-sqli").expect("the shell window");
        assert!(w.url().unwrap().as_str().ends_with("shell?id=sqli&runtime=DOCKER"), "{}", w.url().unwrap());
        open("sqli").unwrap();
        assert_eq!(t.handle().webview_windows().len(), 1);
    }

    #[tokio::test]
    async fn a_screen_window_opens_once_per_machine_with_its_session() {
        let t = TestApp::new();
        let which = |machine: &str| crate::screen::Which { id: "winlab".into(), runtime: Runtime::Vm, machine: machine.into() };
        assert!(open_screen(t.handle(), which("a b"), "GOAD").await.is_err());
        assert!(t.handle().webview_windows().is_empty());
        open_screen(t.handle(), which("dc01"), "GOAD").await.unwrap();
        let w = t.handle().get_webview_window("screen-winlab:dc01").expect("the screen window");
        assert!(w.url().unwrap().as_str().ends_with("screen?id=winlab&runtime=VM&machine=dc01"), "{}", w.url().unwrap());
        assert!(crate::screen::has_session("screen-winlab:dc01"), "its bridge is bound");
        open_screen(t.handle(), which("dc01"), "GOAD").await.unwrap();
        assert_eq!(t.handle().webview_windows().len(), 1);
        w.destroy().unwrap();
        crate::screen::close("screen-winlab:dc01");
        assert!(!crate::screen::has_session("screen-winlab:dc01"));
    }

    #[test]
    fn windows_have_a_system_frame_on_macos_only() {
        // Linux's window manager draws the frame from its own theme (a white frame around the
        // dark app under xfwm4): there and on Windows the page draws its title bar.
        let macos = cfg!(target_os = "macos");
        let settings = window_config("settings", "?window=settings".into(), Theme::Dark);
        assert_eq!(settings.decorations, macos);
        assert!(settings.shadow, "Windows keeps an undecorated window's shadow");
        assert_eq!(settings.url, WebviewUrl::App("?window=settings".into()));
        // macOS keeps its traffic lights over the page's own title bar.
        #[cfg(target_os = "macos")]
        assert!(settings.hidden_title && settings.title_bar_style == tauri::TitleBarStyle::Overlay);
        // The main window, from tauri.conf.json, the same.
        let mut config: tauri::Config = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert!(config.app.windows[0].decorations, "the configuration itself keeps the frame (macOS)");
        frame_configured(&mut config);
        assert_eq!(config.app.windows[0].decorations, macos);
        assert!(config.app.windows[0].shadow);
    }

    #[test]
    fn a_window_opens_in_the_main_windows_theme_or_dark() {
        // Regression: secondary windows set no theme, so on Windows the shell window opened with
        // the system's white title bar over the dark app.
        let t = TestApp::new();
        assert_eq!(window_config("shell-sqli", "shell".into(), opening_theme(t.handle())).theme, Some(Theme::Dark));
        // With the main window open, its theme, which its page keeps on the appearance (Tauri's
        // mock runtime answers Light).
        builder(t.handle(), MAIN, "").unwrap().build().unwrap();
        assert_eq!(opening_theme(t.handle()), Theme::Light);
        assert_eq!(window_config("settings", String::new(), Theme::Light).theme, Some(Theme::Light));
    }

    #[test]
    fn a_window_fits_the_screen_but_stays_usable() {
        crate::testing::assert_close(fit_height(920.0, None), 920.0);
        crate::testing::assert_close(fit_height(920.0, Some(1400.0)), 920.0);
        crate::testing::assert_close(fit_height(920.0, Some(700.0)), 700.0);
        crate::testing::assert_close(fit_height(920.0, Some(300.0)), 480.0);
    }
}
