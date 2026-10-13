//! The host / cloud account setup form, opened in its own window over the main one.

use crate::AppHandle;
use tauri::Manager;

use super::input::valid_id;
use crate::error::{Error, Result};

const LABEL: &str = "server-setup";

/// The setup page to open: editing host `id`, or a new server / cloud account.
fn setup_path(id: Option<String>, cloud: bool) -> Result<String> {
    Ok(match id {
        Some(id) if valid_id(&id) => format!("server-setup?id={id}"),
        Some(_) => return Err(Error::Invalid("unknown server host".into())),
        None if cloud => "server-setup?kind=cloud".into(),
        None => "server-setup".into(),
    })
}

/// Opens the host setup in its own window (label `server-setup`); the window closes
/// itself when setup ends. An already open setup window is replaced.
#[tauri::command]
pub async fn server_open_setup(app: AppHandle, id: Option<String>, kind: Option<String>) -> Result<()> {
    let cloud = kind.as_deref() == Some("cloud");
    let path = setup_path(id, cloud)?;
    if let Some(existing) = app.get_webview_window(LABEL) {
        let _ = existing.destroy();
    }
    let mut builder = tauri::WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::App(path.into()))
        .title(if cloud { "Connect a cloud account" } else { "Connect a host" })
        .inner_size(680.0, 760.0)
        .min_inner_size(560.0, 560.0)
        .resizable(true);
    #[cfg(target_os = "macos")]
    {
        builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    }
    if let Some(main) = app.get_webview_window("main") {
        builder = builder.parent(&main).map_err(|e| Error::Invalid(e.to_string()))?;
    }
    builder.build().map_err(|e| Error::Invalid(format!("could not open the setup window: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_setup_window_opens_at_its_page_and_an_unknown_host_is_refused() {
        let t = crate::test_app::TestApp::new();
        let err = server_open_setup(t.handle().clone(), Some("../x".into()), None).await.unwrap_err();
        assert_eq!(err.to_string(), "unknown server host");
        assert!(t.handle().webview_windows().is_empty());
        server_open_setup(t.handle().clone(), None, Some("cloud".into())).await.unwrap();
        let w = t.handle().get_webview_window(LABEL).expect("the setup window");
        assert!(w.url().unwrap().as_str().ends_with("server-setup?kind=cloud"), "{}", w.url().unwrap());
    }

    #[test]
    fn setup_paths() {
        assert_eq!(setup_path(None, false).unwrap(), "server-setup");
        assert_eq!(setup_path(None, true).unwrap(), "server-setup?kind=cloud");
        // Editing ignores the kind: the host says what it is.
        assert_eq!(setup_path(Some("ab12".into()), true).unwrap(), "server-setup?id=ab12");
        assert!(setup_path(Some("../x".into()), false).is_err());
    }
}
