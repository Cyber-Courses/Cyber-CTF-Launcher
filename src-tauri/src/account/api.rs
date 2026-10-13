//! CyberBackend access. The access token is attached here, on the Rust side, so it
//! never reaches the webview.

use serde_json::{Value, json};

use crate::account::api_operation::ApiOperation;
use crate::account::auth;
use crate::config;
use crate::error::{Error, Result};

/// Runs a GraphQL operation as the logged-in player (anonymous if logged out
/// and `require_login` is false). Returns `data`, or the first error message.
pub async fn graphql(query: &str, variables: Value, require_login: bool) -> Result<Value> {
    let token = match auth::access_token().await {
        Ok(t) => Some(t),
        Err(e) if require_login => return Err(e),
        Err(_) => None,
    };
    send(&config::api_url(), token, query, variables).await
}

/// Posts a GraphQL operation to the API at `base`, with the access token when there is one.
/// Bounded: a backend that accepts the connection and never answers must not hang the agent
/// loop or a UI call forever.
async fn send(base: &str, token: Option<String>, query: &str, variables: Value) -> Result<Value> {
    let mut req = crate::http::api_client()?
        .post(format!("{base}/graphql"))
        .timeout(std::time::Duration::from_secs(30))
        .json(&json!({ "query": query, "variables": variables }));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let res = req.send().await.map_err(|e| Error::Invalid(format!("API unreachable: {e}")))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| Error::Invalid(format!("API response interrupted: {e}")))?;
    parse_response(status, &text)
}

/// A GraphQL response body as its `data`, or the first error message, or (for a body that
/// isn't JSON) the HTTP status and the start of the body.
fn parse_response(status: reqwest::StatusCode, text: &str) -> Result<Value> {
    // Platform errors (e.g. a timed-out cold start) are not GraphQL JSON.
    let body: Value = serde_json::from_str(text).map_err(|_| {
        let excerpt: String = text.chars().take(160).collect();
        Error::Invalid(format!("API error (HTTP {status}): {excerpt}"))
    })?;
    if let Some(message) = body.pointer("/errors/0/message").and_then(Value::as_str) {
        return Err(Error::Invalid(message.to_string()));
    }
    Ok(body.get("data").cloned().unwrap_or(Value::Null))
}

/// Runs one of the allowed CyberBackend operations for the webview (see [`ApiOperation`]).
#[tauri::command]
pub async fn api_query(operation: ApiOperation, variables: Option<Value>) -> Result<Value> {
    graphql(&operation.document(), variables.unwrap_or(Value::Null), false).await
}

#[cfg(test)]
mod tests {
    use super::parse_response;
    use reqwest::StatusCode;
    use serde_json::json;

    #[test]
    fn a_response_gives_its_data_or_its_first_error() {
        assert_eq!(parse_response(StatusCode::OK, r#"{"data":{"me":{"id":"1"}}}"#).unwrap(), json!({ "me": { "id": "1" } }));
        assert_eq!(parse_response(StatusCode::OK, r#"{"other":1}"#).unwrap(), serde_json::Value::Null);
        let err = parse_response(StatusCode::OK, r#"{"errors":[{"message":"nope"},{"message":"two"}],"data":null}"#).unwrap_err();
        assert_eq!(err.to_string(), "nope");
    }

    #[test]
    fn a_non_json_body_names_the_status_and_an_excerpt() {
        let long = "x".repeat(500);
        let err = parse_response(StatusCode::BAD_GATEWAY, &format!("<html>{long}")).unwrap_err().to_string();
        assert!(err.starts_with("API error (HTTP 502 Bad Gateway): <html>xx"), "{err}");
        assert!(err.len() < 220);
    }

    #[tokio::test]
    async fn an_operation_is_posted_with_the_token_and_its_data_read_back() {
        use crate::http::tests::serve_once;
        let body = r#"{"data":{"me":{"id":"1"}}}"#;
        let base = serve_once(
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes(),
        )
        .await;
        let data = super::send(base.trim_end_matches('/'), Some("tok".into()), "{ me { id } }", serde_json::Value::Null).await.unwrap();
        assert_eq!(data, json!({ "me": { "id": "1" } }));
        let base = serve_once(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndown".to_vec()).await;
        let err = super::send(base.trim_end_matches('/'), None, "{ x }", serde_json::Value::Null).await.unwrap_err().to_string();
        assert_eq!(err, "API error (HTTP 502 Bad Gateway): down");
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
        let err = super::send(&format!("http://{closed}"), None, "{ x }", serde_json::Value::Null).await.unwrap_err().to_string();
        assert!(err.starts_with("API unreachable: "), "{err}");
    }

    /// What a webview calling `api_query` with `args` gets back, through Tauri's IPC.
    fn invoke_api_query(args: serde_json::Value) -> Result<tauri::ipc::InvokeResponseBody, serde_json::Value> {
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![super::api_query])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
        let request = tauri::webview::InvokeRequest {
            cmd: "api_query".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            // The app's own origin, which Tauri trusts: `http://tauri.localhost` on Windows.
            url: if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" }.parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(args),
            headers: Default::default(),
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

    /// Hits the real CyberBackend anonymously: `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn queries_the_api_anonymously() {
        let data = super::graphql("{ challenges { id } me { authId } }", serde_json::Value::Null, false).await.unwrap();
        assert!(data["challenges"].is_array());
        assert!(data["me"].is_null());
        let err = super::graphql("mutation { startLab(labId: \"x\") { labId } }", serde_json::Value::Null, true).await.unwrap_err();
        assert_eq!(err.to_string(), super::super::auth::SIGNED_OUT);
    }
}
