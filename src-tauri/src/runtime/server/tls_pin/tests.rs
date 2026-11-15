use std::sync::Arc;

use base64::Engine as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

/// Two self-signed test certificates (EC P-256, made for these tests only, `CN=pve-test-a` and
/// `-b`), and their keys (PKCS#8), as DER in base64.
const CERT_A: &str = "MIIBljCCATygAwIBAgIUdlcJXJfQvIwsQKJvM2fNqf/IxDMwCgYIKoZIzj0EAwIwFTETMBEGA1UEAwwKcHZlLXRlc3QtYTAgFw0yNjExMTQxMjA5MjBaGA8yMTI2MTAyMTEyMDkyMFowFTETMBEGA1UEAwwKcHZlLXRlc3QtYTBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABIVUSd5gpRiG80NKl6L53HDKg8q2Oy+c6twg13B57fkMHaxIMgW4Kj4CtzhwPAjebKi9OqHE/njtwK2ZFwo++5WjaDBmMB0GA1UdDgQWBBSTC3ABgnwKwQ/qorNE/4LvzFYc3jAfBgNVHSMEGDAWgBSTC3ABgnwKwQ/qorNE/4LvzFYc3jAPBgNVHRMBAf8EBTADAQH/MBMGA1UdEQQMMAqCCHB2ZS50ZXN0MAoGCCqGSM49BAMCA0gAMEUCIQCTZHnKkrrqeIoiJ6yrGiQ44ita0vOnIKkOlDVDw7wZbQIgd2gqGdI80cl+t97ht6K9WF04byPGGZSTr5fKCKwi1FQ=";
const PKCS8_A: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgx46DOTx2qKfKOcyzxBiOXbBoMRX1NzXLACYTUgyvHpyhRANCAASFVEneYKUYhvNDSpei+dxwyoPKtjsvnOrcINdwee35DB2sSDIFuCo+Arc4cDwI3myovTqhxP547cCtmRcKPvuV";
const CERT_B: &str = "MIIBlzCCATygAwIBAgIUefpqUfbHaM4kR7SMy7wzj9521jEwCgYIKoZIzj0EAwIwFTETMBEGA1UEAwwKcHZlLXRlc3QtYjAgFw0yNjExMTQxMjA5MjBaGA8yMTI2MTAyMTEyMDkyMFowFTETMBEGA1UEAwwKcHZlLXRlc3QtYjBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABC+sa9kSFx1wjNxp1Ci4bmsDN1Mqb3adiE90GA9opBfacFCFz+2d/fCIiV3ZspPRkvr5uuWwfgYZu6pQUppYO+KjaDBmMB0GA1UdDgQWBBTS8VyFardlqI/NDMHsM6mozDpDaDAfBgNVHSMEGDAWgBTS8VyFardlqI/NDMHsM6mozDpDaDAPBgNVHRMBAf8EBTADAQH/MBMGA1UdEQQMMAqCCHB2ZS50ZXN0MAoGCCqGSM49BAMCA0kAMEYCIQCiGQlv1MaL+OeFt+T24TZuXOCC2NhRr1YuFhrXCUKUIgIhAP6CbCi50VaF7m59u7h+pCW/bueEqKIKt43viEq0Hegu";
const PKCS8_B: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg7xhIOhxAhgYOdPr6hWJ49jUwl4uIMGFmUcr7Fywet2+hRANCAAQvrGvZEhcdcIzcadQouG5rAzdTKm92nYhPdBgPaKQX2nBQhc/tnf3wiIld2bKT0ZL6+brlsH4GGbuqUFKaWDvi";

/// `openssl x509 -fingerprint -sha256` of certificate A.
pub(crate) const FINGERPRINT_A: &str = "4F:1F:C1:74:0E:F3:4D:3C:35:1D:35:82:F2:1B:1B:36:8D:42:03:EF:17:90:50:E8:15:F1:A2:C2:DA:39:4E:81";

fn der(b64: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(b64).unwrap()
}

/// Always the same certificate and key, whatever the client asks for.
#[derive(Debug)]
struct Fixed(Arc<rustls::sign::CertifiedKey>);

impl rustls::server::ResolvesServerCert for Fixed {
    fn resolve(&self, _: rustls::server::ClientHello<'_>) -> Option<Arc<rustls::sign::CertifiedKey>> {
        Some(Arc::clone(&self.0))
    }
}

/// A loopback HTTPS server offering certificate `cert` signed with key `key` (not necessarily
/// its own), answering every request `200 OK`. Returns its port and how many requests it read
/// (a request it read is one whose headers, and credentials, reached it).
async fn server(cert: &str, key: &str) -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let signing = provider.key_provider.load_private_key(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der(key)))).unwrap();
    let certified = rustls::sign::CertifiedKey::new(vec![CertificateDer::from(der(cert))], signing);
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(Fixed(Arc::new(certified))));
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = Arc::clone(&requests);
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let counted = Arc::clone(&counted);
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else { return };
                let mut buf = [0u8; 4096];
                if matches!(tls.read(&mut buf).await, Ok(n) if n > 0) {
                    counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let _ = tls.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await;
                    let _ = tls.shutdown().await;
                }
            });
        }
    });
    (port, requests)
}

/// [`server`] with certificate A and its own key.
pub(crate) async fn server_a() -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
    server(CERT_A, PKCS8_A).await
}

fn url(port: u16) -> String {
    format!("https://127.0.0.1:{port}/api2/json/version")
}

const LIMIT: Duration = Duration::from_secs(10);

#[test]
fn a_fingerprint_reads_as_the_server_page_shows_it_in_any_case_or_spacing() {
    let a = Fingerprint::of(&der(CERT_A));
    assert_eq!(a.to_string(), FINGERPRINT_A);
    assert_eq!(Fingerprint::parse(&FINGERPRINT_A.to_lowercase()), Some(a));
    assert_eq!(Fingerprint::parse(&FINGERPRINT_A.replace(':', "")), Some(a));
    assert_eq!(Fingerprint::parse(&FINGERPRINT_A.replace(':', " ")), Some(a));
    for bad in ["", "4F:1F", &FINGERPRINT_A.replace("4F", "ZZ"), &format!("{FINGERPRINT_A}:00"), "é".repeat(32).as_str()] {
        assert_eq!(Fingerprint::parse(bad), None, "{bad}");
    }
    assert_eq!(serde_json::to_value(a).unwrap(), serde_json::json!(FINGERPRINT_A));
    assert_eq!(serde_json::from_value::<Fingerprint>(serde_json::json!(FINGERPRINT_A.to_lowercase())).unwrap(), a);
    assert!(serde_json::from_value::<Fingerprint>(serde_json::json!("nope")).is_err());
}

#[tokio::test]
async fn the_certificate_is_read_for_confirmation_without_sending_a_request() {
    let (port, requests) = server(CERT_A, PKCS8_A).await;
    assert_eq!(fetch("127.0.0.1", port, LIMIT).await.unwrap().to_string(), FINGERPRINT_A);
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing reached the server");
}

#[tokio::test]
async fn only_the_confirmed_certificate_is_accepted() {
    let a = Fingerprint::parse(FINGERPRINT_A).unwrap();
    let (port, requests) = server(CERT_A, PKCS8_A).await;
    // Confirmed: the request goes through.
    let pinned = Client::new(Trust::Pinned(Some(a)), LIMIT).unwrap();
    assert_eq!(pinned.http.get(url(port)).send().await.unwrap().text().await.unwrap(), "ok");
    assert_eq!(pinned.refused(), None);
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
    // None confirmed yet: refused, the certificate offered said.
    let unconfirmed = Client::new(Trust::Pinned(None), LIMIT).unwrap();
    assert!(unconfirmed.http.get(url(port)).header("Authorization", "PVEAPIToken=x").send().await.is_err());
    assert_eq!(unconfirmed.refused(), Some(Refusal::Unconfirmed { presented: a }));
    // Another server at the same address: refused, both certificates said.
    let (other, other_requests) = server(CERT_B, PKCS8_B).await;
    let changed = Client::new(Trust::Pinned(Some(a)), LIMIT).unwrap();
    assert!(changed.http.get(url(other)).send().await.is_err());
    let b = Fingerprint::of(&der(CERT_B));
    assert_eq!(changed.refused(), Some(Refusal::Changed { pinned: a, presented: b }));
    assert_eq!(
        requests.load(std::sync::atomic::Ordering::SeqCst) + other_requests.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "no refused request reached a server"
    );
    // The error says the server and both fingerprints, in its code.
    let said = Refusal::Changed { pinned: a, presented: b }.error("Garage Proxmox");
    assert!(matches!(said.code(), Code::TlsChanged { ref server, ref pinned, .. } if server == "Garage Proxmox" && pinned == FINGERPRINT_A), "{said:?}");
}

#[tokio::test]
async fn a_server_replaying_the_confirmed_certificate_without_its_key_is_refused() {
    let a = Fingerprint::parse(FINGERPRINT_A).unwrap();
    // Certificate A, its handshake signed with B's key: the fingerprint matches, the proof doesn't.
    let (port, requests) = server(CERT_A, PKCS8_B).await;
    let pinned = Client::new(Trust::Pinned(Some(a)), LIMIT).unwrap();
    assert!(pinned.http.get(url(port)).send().await.is_err());
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_server_trusted_by_the_system_is_not_pinned() {
    let (port, _) = server(CERT_A, PKCS8_A).await;
    let system = Client::new(Trust::System, LIMIT).unwrap();
    assert!(system.http.get(url(port)).send().await.is_err(), "a self-signed certificate isn't trusted by the system");
    assert_eq!(system.refused(), None, "no pin to say anything about");
}
