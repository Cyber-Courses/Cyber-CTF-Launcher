//! A scripted cyber-auth for the tests: it answers each request with the next of its answers,
//! and keeps what each request asked for.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A request the server took: its path and its form, decoded.
#[derive(Debug, Clone)]
pub(super) struct Request {
    pub path: String,
    pub form: std::collections::HashMap<String, String>,
}

/// The requests a [`serve`] took so far.
pub(super) type Requests = Arc<Mutex<Vec<Request>>>;

/// Serves `answers` (status, JSON body) in order, one per connection, at the returned base URL.
pub(super) async fn serve(answers: Vec<(u16, &'static str)>) -> (String, Requests) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests: Requests = Arc::default();
    let seen = requests.clone();
    tokio::spawn(async move {
        for (status, body) in answers {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            let request = read(&mut stream).await;
            seen.lock().unwrap().push(request);
            let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = stream.write_all(reply.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });
    (base, requests)
}

/// One request: its head, then as much body as its `Content-Length` says.
async fn read(stream: &mut tokio::net::TcpStream) -> Request {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await.unwrap_or(0);
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
            if body.len() >= length || n == 0 {
                let path = head.split_whitespace().nth(1).unwrap_or_default().to_string();
                let form = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
                return Request { path, form };
            }
        }
        if n == 0 {
            return Request { path: String::new(), form: std::collections::HashMap::new() };
        }
    }
}
