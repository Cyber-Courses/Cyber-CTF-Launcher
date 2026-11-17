//! Trust on first use for a server's self-signed TLS certificate (Proxmox ships one). The learner
//! confirms the certificate's SHA-256 fingerprint once, as the server's own page shows it; it is
//! kept with the server, and from then on only that certificate is accepted: a different one is
//! refused before anything (a token, a password, a console ticket) is sent, until the learner
//! confirms the new one. A server with a publicly trusted certificate needs no pin: the system's
//! trust store checks it.
//!
//! The pin itself is Isothrum's (`isothrum_runner::tls`, the same SHA-256 and the same written
//! form, so a saved pin stays valid); the launcher keeps what the learner is told.

use std::time::Duration;

use isothrum_runner::tls::Tls;
pub use isothrum_runner::tls::{Fingerprint, Refusal, Trust};

use crate::error::{Code, Error, Result};

/// The error the learner sees when Isothrum refused the certificate of server `server` (its name):
/// none confirmed for it yet, or not the confirmed one.
pub fn refused(refusal: Refusal, server: &str) -> Error {
    Error::coded(match refusal {
        Refusal::Unconfirmed { presented } => Code::TlsUnconfirmed { server: server.to_owned(), presented: presented.to_string() },
        Refusal::Changed { pinned, presented } => Code::TlsChanged { server: server.to_owned(), pinned: pinned.to_string(), presented: presented.to_string() },
    })
}

/// An HTTP client for one server's websocket upgrade (the console), its certificate trusted as
/// `trust` says, and the certificate it was last offered.
pub struct Client {
    /// The client: HTTP/1.1, a connect given up after 15 s, no overall limit (the upgraded stream
    /// lives as long as its window).
    pub http: reqwest::Client,
    tls: Tls,
}

impl Client {
    /// A client for a websocket upgrade to a server trusted as `trust` says.
    pub fn upgrade(trust: Trust) -> Result<Client> {
        let tls = Tls::new(trust).map_err(|e| Error::Invalid(format!("couldn't set up TLS: {e}")))?;
        let http = reqwest::Client::builder()
            .http1_only()
            .connect_timeout(Duration::from_secs(15))
            .tls_backend_preconfigured(tls.config().clone())
            .build()
            .map_err(|e| Error::Invalid(format!("couldn't set up HTTP: {e}")))?;
        Ok(Client { http, tls })
    }

    /// Why a request of this client failed on the server's certificate: none confirmed yet, or
    /// not the confirmed one. None when the certificate isn't why.
    pub fn refused(&self) -> Option<Refusal> {
        self.tls.refused()
    }
}

/// The fingerprint of the certificate `host` offers on `port`, for the learner to confirm: read
/// from the handshake alone, which is ended before any request (nothing is sent to the server).
pub async fn fetch(host: &str, port: u16, timeout: Duration) -> Result<Fingerprint> {
    isothrum_machine::place::certificate(host, port, timeout).await.map_err(|e| Error::Invalid(e.to_string()))
}

#[cfg(test)]
pub(crate) mod tests;
