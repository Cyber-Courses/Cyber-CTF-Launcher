//! The guided "set up this machine" flow, in its own window.

use crate::AppHandle;
use tauri::Manager;

use crate::app::window;
use crate::error::Result;

const LABEL: &str = "machine-setup";

/// The setup steps the window can open at, and "add", the provider picker (the Machine page's
/// "Add a provider"). Only these reach its URL.
const STEPS: [&str; 9] = ["pkgmgr", "virtualization", "docker", "docker-test", "attack", "vm", "vagrant", "vm-test", "add"];

/// The window's page, opened at `step` when given.
fn page(step: Option<&str>) -> String {
    match step {
        Some(s) => format!("{LABEL}?step={s}"),
        None => LABEL.to_string(),
    }
}

/// Opens the guided "set up this machine" flow in its own window (label `machine-setup`),
/// mirroring the server setup window. Focuses it if already open, moved to `step`.
pub fn open(app: &AppHandle, step: Option<&str>) -> Result<()> {
    let step = step.filter(|s| STEPS.contains(s));
    if let Some(existing) = app.get_webview_window(LABEL) {
        if let Some(step) = step {
            crate::app::events::emit(&existing, crate::app::events::MACHINE_SETUP_STEP, step);
        }
        let _ = existing.set_focus();
        return Ok(());
    }
    let builder = window::builder(app, LABEL, page(step))
        .title("Set up this machine")
        // Tall enough for the longest step (the attack-machine toolset choice pushed its buttons
        // below the fold at 760), clamped to the screen so it never opens off-screen; the body
        // scrolls if a step is still taller. The minimum keeps a shrunk window usable.
        .inner_size(760.0, window::height_fitting(app, 920.0))
        .min_inner_size(640.0, window::height_fitting(app, 760.0))
        .resizable(true);
    window::open_over_main(app, builder, "the setup window")
}

#[cfg(test)]
mod tests {
    use tauri::{Listener, Manager};

    /// Opening a new window sizes it to the screen, which Tauri's mock runtime can't tell: only
    /// the open window is driven here.
    #[tokio::test]
    async fn an_open_setup_window_is_focused_and_moved_to_a_known_step_only() {
        let t = crate::testing::test_app::TestApp::new();
        crate::app::window::builder(t.handle(), super::LABEL, super::page(None)).build().unwrap();
        let steps = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = steps.clone();
        t.handle().listen_any(crate::app::events::MACHINE_SETUP_STEP, move |e| seen.lock().unwrap().push(e.payload().to_string()));
        super::open(t.handle(), Some("docker-test")).unwrap();
        super::open(t.handle(), Some("../../settings")).unwrap();
        super::open(t.handle(), Some("add")).unwrap();
        super::open(t.handle(), None).unwrap();
        assert_eq!(*steps.lock().unwrap(), ["\"docker-test\"", "\"add\""]);
        assert_eq!(t.handle().webview_windows().len(), 1);
    }

    #[test]
    fn opens_at_a_step_only_when_given_one() {
        assert_eq!(super::page(None), "machine-setup");
        assert_eq!(super::page(Some("docker-test")), "machine-setup?step=docker-test");
        assert_eq!(super::page(Some("add")), "machine-setup?step=add");
    }
}
