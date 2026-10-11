//! CyberBackend access. The access token is attached here, on the Rust side, so it
//! never reaches the webview.

use serde_json::{Value, json};

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
    // Bounded: a backend that accepts the connection and never answers must not hang the
    // agent loop or a UI call forever.
    let mut req = reqwest::Client::new()
        .post(format!("{}/graphql", config::api_url()))
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

#[tauri::command]
pub async fn api_query(query: String, variables: Option<Value>) -> Result<Value> {
    graphql(&query, variables.unwrap_or(Value::Null), false).await
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
