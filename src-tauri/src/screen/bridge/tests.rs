use super::*;

/// A viewer's upgrade request to the bridge on `port`, with `token`, from `origin`.
fn request(port: u16, token: &str, origin: &str) -> String {
    format!(
        "GET /{token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: binary\r\n\r\n"
    )
}

#[test]
fn the_apps_page_with_the_token_is_accepted_and_answered_as_rfc_6455_says() {
    let a = judge(&request(4100, "t0k", "tauri://localhost"), Some("t0k"), 4100, false).unwrap();
    assert!(a.answer.starts_with("HTTP/1.1 101 Switching Protocols\r\n"), "{}", a.answer);
    assert!(a.answer.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
    assert_eq!(a.protocol.as_deref(), Some("binary"));
    for origin in ["http://tauri.localhost", "https://tauri.localhost"] {
        assert!(judge(&request(4100, "t0k", origin), Some("t0k"), 4100, false).is_ok(), "{origin}");
    }
}

#[test]
fn a_wrong_or_spent_token_another_origin_or_host_is_refused() {
    let req = request(4100, "t0k", "tauri://localhost");
    assert_eq!(judge(&req, Some("other"), 4100, false), Err(Refusal::Token));
    assert_eq!(judge(&req, None, 4100, false), Err(Refusal::Token), "spent");
    assert_eq!(judge(&request(4100, "", "tauri://localhost"), Some("t0k"), 4100, false), Err(Refusal::Token));
    for origin in ["https://evil.example", "null", "tauri://localhost.evil", "http://127.0.0.1:3206", "http://localhost:3206"] {
        assert_eq!(judge(&request(4100, "t0k", origin), Some("t0k"), 4100, false), Err(Refusal::Origin), "{origin}");
    }
    assert!(judge(&request(4100, "t0k", "http://127.0.0.1:3206"), Some("t0k"), 4100, true).is_ok(), "the dev server, in a dev build");
    assert_eq!(judge(&request(4100, "t0k", "http://localhost:3206"), Some("t0k"), 4100, true), Err(Refusal::Origin), "not the address it listens on");
    let rebound = req.replace("Host: 127.0.0.1:4100", "Host: attacker.example:4100");
    assert_eq!(judge(&rebound, Some("t0k"), 4100, false), Err(Refusal::Host));
    assert_eq!(judge(&req, Some("t0k"), 4101, false), Err(Refusal::Host), "another port");
    let no_origin = req.replace("Origin: tauri://localhost\r\n", "");
    assert_eq!(judge(&no_origin, Some("t0k"), 4100, false), Err(Refusal::Origin));
    let plain = "GET /t0k HTTP/1.1\r\nHost: 127.0.0.1:4100\r\n\r\n";
    assert_eq!(judge(plain, Some("t0k"), 4100, false), Err(Refusal::NotWebSocket));
}

/// A viewer's masked binary frame.
fn masked(payload: &[u8]) -> Vec<u8> {
    let mask = [3u8, 1, 4, 1];
    let mut out = vec![0x82, 0x80 | u8::try_from(payload.len()).unwrap()];
    out.extend_from_slice(&mask);
    out.extend(payload.iter().zip(mask.iter().cycle()).map(|(b, m)| b ^ m));
    out
}

/// Connects to the bridge as the screen window's viewer would, with `token` from `origin`.
async fn connect(port: u16, token: &str, origin: &str) -> (TcpStream, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(request(port, token, origin).as_bytes()).await.unwrap();
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if s.read(&mut b).await.unwrap() == 0 {
            break;
        }
        head.push(b[0]);
    }
    (s, String::from_utf8_lossy(&head).into_owned())
}

#[tokio::test]
async fn the_bridge_listens_on_the_loopback_relays_one_viewer_and_spends_its_token() {
    // A fake VNC server: says its version, then echoes what it reads.
    let vnc = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let vnc_at = vnc.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut s, _) = vnc.accept().await.unwrap();
        s.write_all(b"RFB 003.008\n").await.unwrap();
        let mut buf = [0u8; 64];
        let n = s.read(&mut buf).await.unwrap();
        s.write_all(&buf[..n]).await.unwrap();
    });
    let bridge = Bridge::bind().await.unwrap();
    let port = bridge.port();
    assert!(bridge.local.ip().is_loopback(), "bound to the loopback only: {}", bridge.local);
    let token = bridge.arm(Target::Tcp(vnc_at));
    let (_, refused) = connect(port, &token, "https://evil.example").await;
    assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    let (mut viewer, head) = connect(port, &token, "tauri://localhost").await;
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    let mut frame = [0u8; 14];
    viewer.read_exact(&mut frame).await.unwrap();
    assert_eq!(&frame, b"\x82\x0cRFB 003.008\n", "the server's bytes in a binary frame");
    viewer.write_all(&masked(b"RFB 003.008\n")).await.unwrap();
    viewer.read_exact(&mut frame).await.unwrap();
    assert_eq!(&frame, b"\x82\x0cRFB 003.008\n", "the viewer's bytes reached the server unmasked");
    let (_, again) = connect(port, &token, "tauri://localhost").await;
    assert!(again.starts_with("HTTP/1.1 403"), "the token was spent: {again}");
    drop(bridge);
}

/// The screen and the answers to the viewer's pings share one connection: a pong goes out
/// whole between screen frames while the screen streams, never inside one (Clean Code ch.13:
/// one writer fed by a channel, rather than a lock both sides take).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ping_is_answered_whole_while_the_screen_streams() {
    let vnc = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let vnc_at = vnc.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut s, _) = vnc.accept().await.unwrap();
        s.write_all(b"RFB 003.008\n").await.unwrap();
        for _ in 0..400 {
            if s.write_all(&[0xAB; 4096]).await.is_err() {
                return;
            }
        }
        // Held open: the relay runs until the test is done with it.
        let _ = s.read(&mut [0u8; 1]).await;
    });
    let bridge = Bridge::bind().await.unwrap();
    let token = bridge.arm(Target::Tcp(vnc_at));
    let (mut viewer, head) = connect(bridge.port(), &token, "tauri://localhost").await;
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    let mut ping = masked(b"hi");
    ping[0] = 0x89;
    viewer.write_all(&ping).await.unwrap();
    let answered = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let mut op = [0u8; 1];
            viewer.peek(&mut op).await.unwrap();
            let payload = frame(&mut viewer).await;
            match op[0] {
                0x8A => {
                    assert_eq!(payload, b"hi", "the ping's payload, whole");
                    break;
                }
                // The first frame carries what the screen said first: its version, and what came
                // right after it.
                0x82 => assert!(payload.strip_prefix(b"RFB 003.008\n").unwrap_or(&payload).iter().all(|b| *b == 0xAB), "a screen frame, whole"),
                other => panic!("an unexpected frame {other:#x}"),
            }
        }
    });
    answered.await.expect("the ping was answered");
    drop(bridge);
}

/// The bridge to a VNC port that refuses, and to one that accepts but never speaks: the viewer's
/// connection is closed saying where the screen didn't answer, never left waiting.
#[tokio::test]
async fn a_screen_that_doesnt_answer_closes_the_viewers_connection_saying_why() {
    let bridge = Bridge::bind().await.unwrap();
    // A port nothing listens on: bound, then freed.
    let free = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
    let token = bridge.arm(Target::Tcp(free));
    let (mut viewer, head) = connect(bridge.port(), &token, "tauri://localhost").await;
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    let closed = close_reason(&mut viewer).await;
    assert!(closed.starts_with(&format!("the VM's screen didn't answer at {free}")), "{closed}");
    drop(bridge);

    let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = silent.local_addr().unwrap();
    let accepting = tokio::spawn(async move { silent.accept().await.map(|(s, _)| s) });
    let bridge = Bridge::bind().await.unwrap();
    let token = bridge.arm(Target::Tcp(at));
    let (mut viewer, _) = connect(bridge.port(), &token, "tauri://localhost").await;
    // The clock stops only once the bridge's connection is accepted: paused while the connect
    // is still in flight, the idle runtime would jump to the connect limit first. Held open,
    // so the screen stays connected and silent.
    let _screen = accepting.await.unwrap().unwrap();
    tokio::time::pause();
    let closed = close_reason(&mut viewer).await;
    assert_eq!(closed, format!("the VM's screen at {at} accepted the connection but said nothing within 15 s"));
}

/// The reason of the close frame the bridge sends (status 1011).
async fn close_reason(s: &mut TcpStream) -> String {
    let mut head = [0u8; 2];
    s.read_exact(&mut head).await.unwrap();
    assert_eq!(head[0], 0x88, "a close frame");
    let mut payload = vec![0u8; usize::from(head[1] & 0x7F)];
    s.read_exact(&mut payload).await.unwrap();
    assert_eq!(&payload[..2], &1011u16.to_be_bytes());
    String::from_utf8(payload[2..].to_vec()).unwrap()
}

/// One frame from the bridge (unmasked): its payload.
async fn frame(s: &mut TcpStream) -> Vec<u8> {
    let mut head = [0u8; 2];
    s.read_exact(&mut head).await.unwrap();
    let len = match head[1] & 0x7F {
        126 => {
            let mut l = [0u8; 2];
            s.read_exact(&mut l).await.unwrap();
            usize::from(u16::from_be_bytes(l))
        }
        127 => {
            let mut l = [0u8; 8];
            s.read_exact(&mut l).await.unwrap();
            usize::try_from(u64::from_be_bytes(l)).unwrap()
        }
        n => usize::from(n),
    };
    let mut payload = vec![0u8; len];
    s.read_exact(&mut payload).await.unwrap();
    payload
}

/// `n` bytes of the server's, across as many frames as they came in.
async fn server_bytes(s: &mut TcpStream, buf: &mut Vec<u8>, n: usize) -> Vec<u8> {
    while buf.len() < n {
        buf.extend(frame(s).await);
    }
    buf.drain(..n).collect()
}

/// The bridge to a real VNC server (a lab VM's console): the RFB handshake through it, up to
/// the server's framebuffer size. `CYBERCTF_TEST_SCREEN_VNC=127.0.0.1:5900 cargo test --lib --
/// --ignored a_real_vnc_server`.
#[tokio::test]
#[ignore = "needs a VNC server with no password: CYBERCTF_TEST_SCREEN_VNC=<loopback host:port>"]
async fn a_real_vnc_server_answers_through_the_bridge() {
    let at: SocketAddr = std::env::var("CYBERCTF_TEST_SCREEN_VNC").expect("CYBERCTF_TEST_SCREEN_VNC").parse().unwrap();
    let bridge = Bridge::bind().await.unwrap();
    let token = bridge.arm(Target::Tcp(at));
    let (mut viewer, head) = connect(bridge.port(), &token, "tauri://localhost").await;
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");
    let mut buf = Vec::new();
    let version = server_bytes(&mut viewer, &mut buf, 12).await;
    assert!(version.starts_with(b"RFB 003.00"), "{version:?}");
    viewer.write_all(&masked(b"RFB 003.008\n")).await.unwrap();
    let count = server_bytes(&mut viewer, &mut buf, 1).await[0];
    let types = server_bytes(&mut viewer, &mut buf, usize::from(count)).await;
    assert!(types.contains(&1), "the server takes no-password: {types:?}");
    viewer.write_all(&masked(&[1])).await.unwrap();
    assert_eq!(server_bytes(&mut viewer, &mut buf, 4).await, [0, 0, 0, 0], "security result: OK");
    viewer.write_all(&masked(&[1])).await.unwrap();
    let init = server_bytes(&mut viewer, &mut buf, 24).await;
    let (w, h) = (u16::from_be_bytes([init[0], init[1]]), u16::from_be_bytes([init[2], init[3]]));
    let name_len = usize::try_from(u32::from_be_bytes([init[20], init[21], init[22], init[23]])).unwrap();
    let name = server_bytes(&mut viewer, &mut buf, name_len).await;
    println!("the screen through the bridge: {w}x{h}, {}", String::from_utf8_lossy(&name));
    assert!(w > 0 && h > 0);
}
