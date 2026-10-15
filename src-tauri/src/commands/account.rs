//! The account's commands: the sign-in, the backend's queries, and the agent.

use serde_json::Value;
use tauri::ipc::Channel;

use crate::AppHandle;
use crate::account::agent::{self, AgentInfo};
use crate::account::api;
use crate::account::api_operation::ApiOperation;
use crate::account::auth::{self, AuthStatus};
use crate::error::Result;

/// Signs in through the system browser, the sign-in link sent on `events` (see [`auth::auth_login`]).
#[tauri::command]
pub async fn auth_login(app: AppHandle, events: Channel<String>) -> Result<AuthStatus> {
    auth::auth_login(app, events).await
}

/// Ends the sign-in waiting for the browser, if any.
#[tauri::command]
pub fn auth_login_cancel() {
    auth::auth_login_cancel();
}

/// Whether the player is signed in, and as whom.
#[tauri::command]
pub async fn auth_status() -> AuthStatus {
    auth::auth_status().await
}

/// Signs out.
#[tauri::command]
pub async fn auth_logout() {
    auth::auth_logout().await;
}

/// One of the backend's known queries, with the player's token (see [`api::api_query`]).
#[tauri::command]
pub async fn api_query(operation: ApiOperation, variables: Option<Value>) -> Result<Value> {
    api::api_query(operation, variables).await
}

/// This launcher as the website's agent sees it.
#[tauri::command]
pub fn agent_info(app: AppHandle) -> Result<AgentInfo> {
    agent::agent_info(&app)
}

/// The player's answer to a launch the website asked for.
#[tauri::command]
pub fn confirm_launch(session_id: &str, approve: bool) {
    agent::confirm_launch(session_id, approve);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// What a webview calling `api_query` with `args` gets back, through Tauri's IPC.
    fn invoke_api_query(args: serde_json::Value) -> Result<tauri::ipc::InvokeResponseBody, serde_json::Value> {
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![super::api_query])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default()).build().unwrap();
        let request = tauri::webview::InvokeRequest {
            cmd: "api_query".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            // The app's own origin, which Tauri trusts: `http://tauri.localhost` on Windows.
            url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: tauri::http::HeaderMap::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        };
        tauri::test::get_ipc_response(&webview, request)
    }

    #[test]
    fn a_webview_can_run_named_operations_only() {
        // Refused while reading the arguments: nothing is sent, with or without a token.
        for operation in ["deleteAccount", "{ me { email } }"] {
            let err = invoke_api_query(json!({ "operation": operation })).unwrap_err().to_string();
            assert!(err.contains("unknown variant"), "{err}");
        }
        let err = invoke_api_query(json!({ "query": "{ me { email } }" })).unwrap_err().to_string();
        assert!(err.contains("missing required key operation"), "{err}");
    }
}
