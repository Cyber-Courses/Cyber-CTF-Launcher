//! A loopback HTTPS server standing for a server whose certificate is self-signed (Proxmox VE's
//! own): each request answered by the test, so what the runner asks over its own HTTPS client is
//! answered as the processes it runs are by [`isothrum_runner::fake`].

use std::sync::Arc;

use base64::Engine as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// A self-signed certificate (EC P-256, `CN=proxmox-test`, made for these tests only) and its
/// key (PKCS #8), as DER in base64.
const CERT: &str = "MIIBdjCCARugAwIBAgIUG4IIwA8ZI4RlTWa8G3n8qsryrTEwCgYIKoZIzj0EAwIwFzEVMBMGA1UEAwwMcHJveG1veC10ZXN0MCAXDTI2MTExNzEwMTgzMFoYDzIxMjYxMDI0MTAxODMwWjAXMRUwEwYDVQQDDAxwcm94bW94LXRlc3QwWTATBgcqhkjOPQIBBggqhkjOPQMBBwNCAAQypH3mjiVJKy8e1Ubu99OdfS3EW0/f7tK6bEiqSTbIlsqclvunUcqn6idbqQkRTLx/HEBH1ewRs7tA82DTk1Kto0MwQTAdBgNVHQ4EFgQUbRzARksJJ4YXzJwsMWKvuTtGqbMwDwYDVR0TAQH/BAUwAwEB/zAPBgNVHREECDAGhwR/AAABMAoGCCqGSM49BAMCA0kAMEYCIQDY7UHUYD6nnECaCJMNcai+0W71ZsGa5qhiPPpEZ+DxQwIhAPqSDYv86y2Ohi0DTw47wV4DJtv1ugEUr1+YCWmHM41j";
const KEY: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgd5yH1sqkITrDwHfxATtlVduDLxLL+EZPKGr7xpJOv2uhRANCAAQypH3mjiVJKy8e1Ubu99OdfS3EW0/f7tK6bEiqSTbIlsqclvunUcqn6idbqQkRTLx/HEBH1ewRs7tA82DTk1Kt";

fn der(b64: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(b64).unwrap()
}

/// One request the server was sent: its method, its path and query, its headers (names in
/// lower case).
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
}

impl Request {
    /// Header `name`'s value (lower case), when it was sent.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// A server listening on the loopback until the test's runtime ends.
pub struct Server {
    /// `https://127.0.0.1:<port>`.
    pub endpoint: String,
    /// Its certificate's SHA-256, as the runner pins it (`PROXMOX_VE_TLS_FINGERPRINT`).
    pub fingerprint: String,
}

impl Server {
    /// Serves on a free loopback port, each request answered by `answer` (its HTTP status and
    /// JSON body). Called from a Tokio test.
    pub fn start(answer: impl Fn(&Request) -> (u16, String) + Send + Sync + 'static) -> Server {
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(der(CERT))], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der(KEY))))
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("https://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let answer = Arc::new(answer);
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let (acceptor, answer) = (acceptor.clone(), Arc::clone(&answer));
                tokio::spawn(async move {
                    if let Ok(tls) = acceptor.accept(tcp).await {
                        serve(tls, &*answer).await;
                    }
                });
            }
        });
        Server { endpoint, fingerprint: isothrum_runner::tls::Fingerprint::of(&der(CERT)).to_string() }
    }
}

/// Reads one request (its body skipped), writes `answer`'s answer, and closes.
async fn serve(tls: tokio_rustls::server::TlsStream<tokio::net::TcpStream>, answer: &(impl Fn(&Request) -> (u16, String) + ?Sized)) {
    let mut reader = BufReader::new(tls);
    let mut line = String::new();
    if reader.read_line(&mut line).await.is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or_default().to_owned(), parts.next().unwrap_or_default().to_owned());
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).await.is_err() || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let length = headers.iter().find(|(n, _)| n == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    if reader.read_exact(&mut vec![0; length]).await.is_err() {
        return;
    }
    let (status, body) = answer(&Request { method, target, headers });
    let response = format!("HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let mut tls = reader.into_inner();
    if tls.write_all(response.as_bytes()).await.is_ok() {
        let _ = tls.shutdown().await;
    }
}
