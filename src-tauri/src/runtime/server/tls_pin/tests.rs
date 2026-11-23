use std::sync::Arc;

use base64::Engine as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::error::Code;

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

const LIMIT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn the_certificate_is_read_for_confirmation_without_sending_a_request() {
    let (port, requests) = server(CERT_A, PKCS8_A).await;
    assert_eq!(fetch("127.0.0.1", port, LIMIT).await.unwrap().to_string(), FINGERPRINT_A);
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing reached the server");
}

#[tokio::test]
async fn a_refused_console_says_the_certificate_and_the_server() {
    let a = Fingerprint::parse(FINGERPRINT_A).unwrap();
    let (other, requests) = server(CERT_B, PKCS8_B).await;
    let changed = Client::upgrade(Trust::Pinned(Some(a))).unwrap();
    assert!(changed.http.get(format!("https://127.0.0.1:{other}/")).send().await.is_err());
    let b = Fingerprint::of(&der(CERT_B));
    assert_eq!(changed.refused(), Some(Refusal::Changed { pinned: a, presented: b }));
    assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing reached the server");
    let said = refused(Refusal::Changed { pinned: a, presented: b }, "Garage Proxmox");
    assert!(matches!(said.code(), Code::TlsChanged { ref server, ref pinned, .. } if server == "Garage Proxmox" && pinned == FINGERPRINT_A), "{said:?}");
    let new = refused(Refusal::Unconfirmed { presented: a }, "Garage Proxmox");
    assert!(matches!(new.code(), Code::TlsUnconfirmed { ref presented, .. } if presented == FINGERPRINT_A), "{new:?}");
}

#[test]
fn a_saved_pin_reads_as_it_was_written() {
    let a = Fingerprint::parse(FINGERPRINT_A).unwrap();
    assert_eq!(serde_json::to_value(a).unwrap(), serde_json::json!(FINGERPRINT_A));
    assert_eq!(serde_json::from_value::<Fingerprint>(serde_json::json!(FINGERPRINT_A.to_lowercase())).unwrap(), a);
}
