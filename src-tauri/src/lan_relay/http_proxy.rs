//! A loopback HTTP forward proxy (plain HTTP only), for Vagrant's WinRM to Windows guests: the
//! WinRM client (HTTPClient) follows `http_proxy` and sends absolute-URI requests. Each client
//! connection gets one connection to its target, which WinRM's Negotiate auth needs.

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use super::Relay;

const BAD_GATEWAY: &[u8] = b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
/// A request head longer than this is cut off (and then refused as malformed).
const MAX_HEAD: usize = 64 * 1024;

pub async fn start_http_proxy() -> std::io::Result<Relay> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let local = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        while let Ok((inbound, _)) = listener.accept().await {
            tokio::spawn(async move {
                let _ = proxy_connection(inbound).await;
            });
        }
    });
    Ok(Relay { port: local, task })
}

/// The request head's target as (host:port, origin-form path), from an absolute `http://` URI.
pub(super) fn split_target(uri: &str) -> Option<(String, String)> {
    let rest = uri.strip_prefix("http://")?;
    let (authority, path) = rest.find('/').map(|i| (&rest[..i], &rest[i..])).unwrap_or((rest, "/"));
    if authority.is_empty() {
        return None;
    }
    let authority = if authority.rsplit_once(':').is_some_and(|(_, p)| p.parse::<u16>().is_ok()) && !authority.ends_with(']') {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    Some((authority, path.to_string()))
}

/// Rewrites one request head for the origin server: origin-form target, no proxy headers.
/// Returns the target and the body length.
pub(super) fn rewrite_head(head: &str) -> Option<(String, String, usize)> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.splitn(3, ' ');
    let (method, uri, version) = (first.next()?, first.next()?, first.next()?);
    let (target, path) = split_target(uri)?;
    let mut out = format!("{method} {path} {version}\r\n");
    let mut len = 0;
    for line in lines.filter(|l| !l.is_empty()) {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("proxy-connection:") || lower.starts_with("proxy-authorization:") {
            continue;
        }
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().ok()?;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    Some((target, out, len))
}

/// Reads one request head (up to its blank line), skipping stray CRLFs between requests.
/// None when the client closed the connection.
async fn read_head(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> std::io::Result<Option<String>> {
    let mut head = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        if line == "\r\n" && head.is_empty() {
            continue; // stray CRLF between requests
        }
        head.push_str(&line);
        if line == "\r\n" || head.len() > MAX_HEAD {
            return Ok(Some(head));
        }
    }
}

/// Answers 502 and closes, while the client's write half is still ours.
async fn refuse(client_out: &mut Option<OwnedWriteHalf>) -> std::io::Result<()> {
    if let Some(mut out) = client_out.take() {
        out.write_all(BAD_GATEWAY).await?;
    }
    Ok(())
}

async fn proxy_connection(inbound: TcpStream) -> std::io::Result<()> {
    let (read_half, write_half) = inbound.into_split();
    let mut reader = BufReader::new(read_half);
    // The client's write half, until the response pump takes it over.
    let mut client_out = Some(write_half);
    let mut upstream: Option<(String, OwnedWriteHalf, JoinHandle<()>)> = None;
    loop {
        let Some(head) = read_head(&mut reader).await? else {
            // Client done: let the last response finish.
            if let Some((_, mut to_origin, pump)) = upstream {
                let _ = to_origin.shutdown().await;
                let _ = pump.await;
            }
            return Ok(());
        };
        let parsed = rewrite_head(head.trim_end());
        let Some((target, rewritten, body_len)) = parsed.filter(|(t, ..)| upstream.as_ref().is_none_or(|(u, ..)| u == t)) else {
            // Malformed, or a second origin on one connection: refuse and close.
            return refuse(&mut client_out).await;
        };
        if upstream.is_none() {
            let Ok(stream) = TcpStream::connect(target.as_str()).await else {
                return refuse(&mut client_out).await;
            };
            let (mut from_origin, to_origin) = stream.into_split();
            let mut out = client_out.take().expect("client write half until the first connect");
            // Responses stream back in order; the client pairs them with its requests.
            let pump = tokio::spawn(async move {
                let _ = tokio::io::copy(&mut from_origin, &mut out).await;
                let _ = out.shutdown().await;
            });
            upstream = Some((target, to_origin, pump));
        }
        let (_, to_origin, _) = upstream.as_mut().expect("connected above");
        to_origin.write_all(rewritten.as_bytes()).await?;
        let mut body = (&mut reader).take(body_len as u64);
        tokio::io::copy(&mut body, to_origin).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_heads_become_origin_form() {
        let (t, head, len) =
            rewrite_head("POST http://10.0.0.7:5985/wsman HTTP/1.1\r\nHost: 10.0.0.7:5985\r\nProxy-Connection: Keep-Alive\r\nContent-Length: 12").unwrap();
        assert_eq!(t, "10.0.0.7:5985");
        assert_eq!(head, "POST /wsman HTTP/1.1\r\nHost: 10.0.0.7:5985\r\nContent-Length: 12\r\n\r\n");
        assert_eq!(len, 12);
        assert_eq!(split_target("http://host").unwrap(), ("host:80".into(), "/".into()));
        assert!(rewrite_head("GET /relative HTTP/1.1").is_none());
    }

    #[test]
    fn split_target_defaults_the_port_and_keeps_ipv6() {
        assert_eq!(split_target("http://[fd00::7]/wsman").unwrap(), ("[fd00::7]:80".into(), "/wsman".into()));
        assert_eq!(split_target("http://[fd00::7]:5985/x").unwrap(), ("[fd00::7]:5985".into(), "/x".into()));
        assert!(split_target("https://host/").is_none());
        assert!(split_target("http:///path").is_none());
    }

    #[tokio::test]
    async fn malformed_requests_get_a_502() {
        let proxy = start_http_proxy().await.unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
        c.write_all(b"GET /relative HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        let mut got = Vec::new();
        c.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, BAD_GATEWAY);
    }

    #[tokio::test]
    async fn proxy_keeps_one_origin_connection_for_several_requests() {
        let origin = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = origin.local_addr().unwrap().port();
        // Answers each request on the same connection with the body it got (echo).
        tokio::spawn(async move {
            let (s, _) = origin.accept().await.unwrap();
            let mut r = BufReader::new(s);
            for _ in 0..2 {
                let mut len = 0;
                loop {
                    let mut l = String::new();
                    r.read_line(&mut l).await.unwrap();
                    if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if l == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).await.unwrap();
                let reply = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                r.get_mut().write_all(reply.as_bytes()).await.unwrap();
                r.get_mut().write_all(&body).await.unwrap();
            }
        });
        let proxy = start_http_proxy().await.unwrap();
        let mut c = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();
        for msg in ["first", "second!"] {
            let req = format!("POST http://127.0.0.1:{port}/wsman HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{msg}", msg.len());
            c.write_all(req.as_bytes()).await.unwrap();
            let expect = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{msg}", msg.len());
            let mut got = vec![0; expect.len()];
            c.read_exact(&mut got).await.unwrap();
            assert_eq!(String::from_utf8(got).unwrap(), expect);
        }
    }
}

#[cfg(test)]
mod proptests;
