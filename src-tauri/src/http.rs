//! HTTP clients with limits: every request gives up on a host that doesn't answer (connect),
//! goes quiet mid-answer (read) or takes too long overall, instead of hanging a launch, a sign-in
//! or a server test forever; a download refuses a body past its size limit.

use std::time::Duration;

use crate::error::{Error, Result};

/// How long reaching a host may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// API calls (CyberBackend, sign-in, cloud token checks): small answers, quick or broken.
const API_READ_TIMEOUT: Duration = Duration::from_secs(30);
const API_TIMEOUT: Duration = Duration::from_secs(60);

/// Lab archives: large, on slow links too, so a generous overall limit, but a transfer that
/// stalls (no byte for a minute) is given up.
const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The largest lab archive downloaded.
pub const MAX_LAB_ARCHIVE: u64 = 512 * 1024 * 1024;

fn built(b: reqwest::ClientBuilder) -> Result<reqwest::Client> {
    b.build().map_err(|e| Error::Invalid(format!("couldn't set up HTTP: {e}")))
}

/// A client for API calls.
pub fn api_client() -> Result<reqwest::Client> {
    built(reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT).read_timeout(API_READ_TIMEOUT).timeout(API_TIMEOUT))
}

/// A client for downloads.
pub fn download_client() -> Result<reqwest::Client> {
    built(reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT).read_timeout(DOWNLOAD_STALL_TIMEOUT).timeout(DOWNLOAD_TIMEOUT))
}

/// A body read up to a size limit: refused as soon as it is known to be larger (its declared
/// length, or the bytes received so far).
pub struct Limited {
    max: u64,
    buf: Vec<u8>,
}

impl Limited {
    pub fn new(declared: Option<u64>, max: u64) -> Result<Limited> {
        if let Some(len) = declared.filter(|l| *l > max) {
            return Err(too_large(len, max));
        }
        Ok(Limited { max, buf: Vec::with_capacity(declared.unwrap_or(0).min(max).min(64 * 1024 * 1024) as usize) })
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<()> {
        let total = self.buf.len() as u64 + chunk.len() as u64;
        if total > self.max {
            return Err(too_large(total, self.max));
        }
        self.buf.extend_from_slice(chunk);
        Ok(())
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

fn too_large(size: u64, max: u64) -> Error {
    Error::Invalid(format!("download refused: it is larger than {} MB (at least {} MB)", max / (1024 * 1024), size.div_ceil(1024 * 1024)))
}

/// A response's body, streamed, refused past `max` bytes.
pub async fn body_limited(mut res: reqwest::Response, max: u64) -> Result<Vec<u8>> {
    let mut body = Limited::new(res.content_length(), max)?;
    while let Some(chunk) = res.chunk().await.map_err(|e| Error::Invalid(format!("download failed: {e}")))? {
        body.push(&chunk)?;
    }
    Ok(body.into_bytes())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn a_body_past_its_limit_is_refused() {
        assert!(Limited::new(Some(11), 10).err().unwrap().to_string().starts_with("download refused: it is larger than 0 MB"));
        let mut b = Limited::new(None, 10).unwrap();
        b.push(b"hello").unwrap();
        b.push(b"world").unwrap();
        assert!(b.push(b"!").is_err());
        assert_eq!(b.into_bytes(), b"helloworld");
        let mut big = Limited::new(Some(3 * 1024 * 1024), 4 * 1024 * 1024).unwrap();
        let err = big.push(&vec![0; 5 * 1024 * 1024]).unwrap_err().to_string();
        assert_eq!(err, "download refused: it is larger than 4 MB (at least 5 MB)");
    }

    #[test]
    fn clients_build() {
        assert!(api_client().is_ok());
        assert!(download_client().is_ok());
    }

    /// Serves one canned HTTP response on a loopback port (no network), returning its URL.
    pub(crate) async fn serve_once(response: Vec<u8>) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut req = [0u8; 4096];
            let _ = s.read(&mut req).await;
            let _ = s.write_all(&response).await;
            let _ = s.shutdown().await;
        });
        format!("http://{addr}/")
    }

    #[tokio::test]
    async fn a_body_is_streamed_within_its_limit() {
        let url = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello".to_vec()).await;
        let res = download_client().unwrap().get(url).send().await.unwrap();
        assert_eq!(body_limited(res, 10).await.unwrap(), b"hello");
        // Declared too large: refused before reading.
        let url = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\nConnection: close\r\n\r\nhello".to_vec()).await;
        let res = download_client().unwrap().get(url).send().await.unwrap();
        assert!(body_limited(res, 10).await.unwrap_err().to_string().starts_with("download refused"));
        // Undeclared (chunked) and too large: refused as it arrives.
        let url =
            serve_once(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n8\r\n12345678\r\n0\r\n\r\n".to_vec()).await;
        let res = download_client().unwrap().get(url).send().await.unwrap();
        assert!(body_limited(res, 10).await.unwrap_err().to_string().starts_with("download refused"));
    }
}
