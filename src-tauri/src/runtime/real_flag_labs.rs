//! The CTF flag contract, end to end, without production services (opt-in). CyberBackend issues
//! each player's per-objective values (`CTF_FLAG_<KEY>`, and the launch token a lab claims its
//! evidence with); the launcher fetches them for the player (`startLab`) and gives them to the
//! lab as its inputs; Isoloom makes each a file in the lab's machine (a Compose secret, never the
//! container's environment); the engine delivers it, through the launcher's own Docker path and
//! through the Isothrum runner; the player finds it in the lab; cyberctf-web's lab page submits it
//! (`submitObjectiveAction`) to the same backend, which checks it with the key that derived it.
//! A wrong answer is refused, a park and resume keeps every value, and no value shows in the
//! run's log, the lab's or the runner's files, `docker inspect`, or the backend's own output.
//!
//! ```text
//! CYBERCTF_TEST_BACKEND=<CyberBackend checkout> \
//! CYBERCTF_TEST_WEB=<cyberctf-web checkout> \
//! CYBERCTF_TEST_LAB=<invoice-portal-api, in Isoloom's current syntax> \
//! cargo test real_flag_labs -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The backend is CyberBackend's `test/e2e/server.ts`: the real API on a throwaway database it
//! starts and removes, with test players whose tokens it signs itself. Never production.
//! `CYBERCTF_TEST_LAB` is optional (the invoice lab claims its evidence with the launch token);
//! the flag lab is written here.

mod leaks;
mod services;

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::real_labs::{Run, check, install_copy, leftovers};
use super::{docker, paths};
use crate::platform::exec::run;
use services::{Backend, Expect, Submission};

/// A container lab taking three of the player's values as files: the flags of objectives `main`
/// and `ssrf-to-metadata`, and the key (objective `ctf-key`) the keyed objectives derive from.
const FLAG_LAB: &str = "version: 1
name: flag-contract
networks:
  lab: { cidr: 10.30.77.0/24 }
inputs: [CTF_FLAG_MAIN, CTF_FLAG_SSRF_TO_METADATA, CTF_FLAG_CTF_KEY]
machines:
  target:
    networks: { lab: 10 }
    ports: { web: { port: 80, protocol: http } }
    files:
      - { path: /ctf/flag, input: CTF_FLAG_MAIN, mode: \"0444\" }
      - { path: /ctf/ssrf-flag, input: CTF_FLAG_SSRF_TO_METADATA, mode: \"0444\" }
      - { path: /ctf/key, input: CTF_FLAG_CTF_KEY }
    app: { image: nginx:alpine }
checks:
  - { http: \"http://target/\" }
";

/// The flag lab's files: the objective whose value each holds, the input the lab names it by, and
/// where it lands.
const FILES: [(&str, &str, &str); 3] =
    [("main", "CTF_FLAG_MAIN", "/ctf/flag"), ("ssrf-to-metadata", "CTF_FLAG_SSRF_TO_METADATA", "/ctf/ssrf-flag"), ("ctf-key", "CTF_FLAG_CTF_KEY", "/ctf/key")];

/// The keyed objective of the backend's `flags` lab: HMAC-SHA256 of this text under the player's
/// key, in lowercase hex (as Juice Shop's flags are, there with SHA-1).
const KEYED: (&str, &str) = ("score-board", "Score Board");

/// One run of a lab for one player, and what came of it.
#[derive(Default)]
struct Played {
    /// What the player submits from the lab page.
    submissions: Vec<Submission>,
    /// The values no log, file or container may show.
    secrets: Vec<String>,
    /// What the engine said, the launcher's log of the run.
    log: Vec<String>,
    failures: Vec<String>,
}

/// One launch of a lab: the lab's id at the backend, and the values the launch gives it.
struct Launch {
    lab_id: String,
    env: Vec<(String, String)>,
}

/// The lab's machine `service`'s container (Compose's name for it).
fn container(id: &str, service: &str) -> String {
    format!("{}-{service}-1", docker::project(id))
}

/// The text of file `path` inside `container`; None when it isn't there.
async fn read_in(container: &str, path: &str) -> Option<String> {
    run("docker", &["exec", container, "cat", path], None).await.ok()
}

/// Lowercase hex digits.
const HEX: &[u8; 16] = b"0123456789abcdef";

/// `HMAC-SHA256(key, text)` in lowercase hex (RFC 2104), as a keyed lab computes a flag.
fn hmac_sha256_hex(key: &[u8], text: &[u8]) -> String {
    let mut block = [0u8; 64];
    if key.len() > block.len() {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let inner = Sha256::new().chain_update(block.map(|b| b ^ 0x36)).chain_update(text).finalize();
    let outer = Sha256::new().chain_update(block.map(|b| b ^ 0x5c)).chain_update(inner).finalize();
    outer.iter().flat_map(|b| [char::from(HEX[usize::from(b >> 4)]), char::from(HEX[usize::from(b & 0xf)])]).collect()
}

/// The value the launch gave input `name`, if any.
fn given<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
    env.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// What the player finds in the flag lab: each file's text, by objective (an input the backend
/// didn't issue for this lab leaves no file).
async fn found_in_flag_lab(id: &str) -> Vec<(&'static str, Option<String>)> {
    let target = container(id, "target");
    let mut found = Vec::new();
    for (key, _, path) in FILES {
        found.push((key, read_in(&target, path).await));
    }
    found
}

/// The flag lab's files against the launch: each holds exactly its input's value (byte for byte,
/// no newline added), or is absent when the backend issued none.
fn check_files(env: &[(String, String)], found: &[(&str, Option<String>)], p: &mut Played) {
    for ((key, file), (_, input, path)) in found.iter().zip(FILES) {
        let want = given(env, input).filter(|v| !v.is_empty());
        check(
            file.as_deref() == want,
            format!("{path} holds the launch's {input} ({}), objective {key}", if want.is_some() { "given" } else { "not issued: no file" }),
            &mut p.failures,
        );
    }
}

/// The answers the player submits from the flag lab: each flag found, as is; the keyed flag,
/// computed from the key found; and one wrong answer.
fn flag_lab_answers(lab_id: &str, found: &[(&str, Option<String>)], p: &mut Played) {
    let submit = |key: &str, answer: String, expect| Submission { lab_id: lab_id.to_owned(), key: key.to_owned(), answer, expect };
    for (key, file) in found {
        match (*key, file) {
            ("ctf-key", Some(k)) => {
                let flag = hmac_sha256_hex(k.as_bytes(), KEYED.1.as_bytes());
                p.secrets.push(flag.clone());
                p.submissions.push(submit(KEYED.0, flag, Expect::Correct));
            }
            (key, Some(flag)) => p.submissions.push(submit(key, flag.clone(), Expect::Correct)),
            (_, None) => {}
        }
    }
    p.submissions.push(submit("main", "FLAG{not-this-players}".to_owned(), Expect::Wrong));
}

/// What the player finds in the invoice lab: the invoice's amount, from its API on this machine,
/// as the two-decimal evidence the database holds (`DECIMAL(10,2)`, the AMOUNT the backend derived
/// and checks). The API presents it as a JSON number, which drops a trailing zero (`4563.40` ->
/// `4563.4`); submitting that lossy form is Finding 1, a lab/backend bug tracked on its own with a
/// failing test, not the flag-delivery contract this harness proves, so the amount is normalised
/// here.
async fn found_in_invoice_lab(r: &Run) -> Option<String> {
    let url = r.status().await.ok()?.url?;
    let body: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/api/invoices/INV-20507", url.trim_end_matches('/')))
        .header("X-Portal-Key", "vendor-demo-key")
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    Some(format!("{:.2}", body.get("amount")?.as_f64()?))
}

/// What the player finds in lab `kind` (`flags` or `invoice`), as (objective, answer) pairs.
async fn found(r: &Run, kind: Kind) -> Vec<(&'static str, Option<String>)> {
    match kind {
        Kind::Flags => found_in_flag_lab(&r.id).await,
        Kind::Invoice => vec![("main", found_in_invoice_lab(r).await)],
    }
}

/// Which lab folder a case runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The flag lab written here.
    Flags,
    /// `CYBERCTF_TEST_LAB`.
    Invoice,
}

/// The lab folder of `kind`, installed as the launcher installs one, under id `id`.
fn install(kind: Kind, id: &str) -> PathBuf {
    match kind {
        Kind::Flags => {
            let src = std::env::temp_dir().join(format!("cyberctf-flag-lab-{id}"));
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("isoloom.yml"), FLAG_LAB).unwrap();
            let dir = install_copy(&src, id);
            let _ = std::fs::remove_dir_all(src);
            dir
        }
        Kind::Invoice => install_copy(Path::new(&std::env::var("CYBERCTF_TEST_LAB").unwrap()), id),
    }
}

/// Launches backend lab `lab` for player `player` through the runner, finds what the lab holds,
/// parks and resumes it (everything still there, unchanged), checks no value leaked, and stops it.
async fn play(backend: &Backend, player: &str, (lab, kind): (&str, Kind)) -> Played {
    let mut p = Played::default();
    let lab_id = backend.lab(lab).to_owned();
    let env = crate::labs::launch_values_at(backend.url(), backend.token(player), &lab_id).await.unwrap();
    p.secrets.extend(env.iter().filter(|(k, _)| k != "CTF_API_URL").map(|(_, v)| v.clone()));
    let launch = Launch { lab_id, env };
    let id = format!("ci-flag-{lab}-{}", rand::random::<u16>());
    let r = Run { dir: install(kind, &id), home: std::env::temp_dir().join(format!("cyberctf-{id}-home")), id };
    match r.start(&launch.env, &mut |l| p.log.push(l)).await {
        Ok(()) => exercise(&r, kind, &launch, &mut p).await,
        Err(e) => check(false, format!("{lab} starts through the runner: {e}"), &mut p.failures),
    }
    let stopped = r.stop(|l| p.log.push(l)).await;
    check(stopped.is_ok(), format!("{lab} stops ({:?})", stopped.err()), &mut p.failures);
    check(leftovers(&r.id).await.is_empty(), format!("nothing of {lab} is left"), &mut p.failures);
    let leaked = leaks::places_holding(&p.secrets, &p.log, &[&r.dir, &r.home], &docker::project(&r.id)).await;
    check(leaked.is_empty(), format!("no value of {lab}'s launch shows anywhere after the stop: {leaked:?}"), &mut p.failures);
    let _ = std::fs::remove_dir_all(&r.dir);
    let _ = std::fs::remove_dir_all(&r.home);
    p
}

/// The lab while it is up: what the player finds, the same after a park and resume, and no value
/// in a log, a file or a container's description.
async fn exercise(r: &Run, kind: Kind, launch: &Launch, p: &mut Played) {
    let before = found(r, kind).await;
    let lab_id = launch.lab_id.as_str();
    match kind {
        Kind::Flags => {
            check_files(&launch.env, &before, p);
            flag_lab_answers(lab_id, &before, p);
        }
        Kind::Invoice => {
            let amount = before[0].1.clone();
            check(amount.is_some(), format!("the invoice's amount is read from the lab's API ({amount:?})"), &mut p.failures);
            if let Some(a) = amount {
                p.secrets.push(a.clone());
                p.submissions.push(Submission { lab_id: lab_id.to_owned(), key: "main".into(), answer: a, expect: Expect::Correct });
            }
        }
    }
    let leaked = leaks::places_holding(&p.secrets, &p.log, &[&r.dir, &r.home], &docker::project(&r.id)).await;
    check(leaked.is_empty(), format!("no value of the launch shows while the lab runs: {leaked:?}"), &mut p.failures);
    let parked = r.park(&mut |l| p.log.push(l)).await;
    check(parked.is_ok(), format!("the lab parks ({:?})", parked.err()), &mut p.failures);
    let resumed = r.resume(&mut |l| p.log.push(l)).await;
    check(resumed.is_ok() && paths::parked(&r.dir).is_none(), format!("the lab resumes ({:?})", resumed.err()), &mut p.failures);
    let after = found(r, kind).await;
    check(after == before, "after a park and resume, the lab holds the same values".into(), &mut p.failures);
}

/// Every case for player `player` through `via`: the flag lab against a lab with objectives and
/// one without (its implicit `main` alone), then the invoice lab when given; then the player's
/// answers, from the lab page.
async fn contract(backend: &Backend, web: &Path, player: &str) -> Vec<String> {
    let mut cases = vec![("flags", Kind::Flags), ("flags-main", Kind::Flags)];
    if std::env::var_os("CYBERCTF_TEST_LAB").is_some() {
        cases.push(("invoice", Kind::Invoice));
    }
    let mut all = Played::default();
    for case in cases {
        let p = play(backend, player, case).await;
        all.submissions.extend(p.submissions);
        all.secrets.extend(p.secrets);
        all.failures.extend(p.failures);
    }
    let (judged, output) = services::submit_from_the_lab_page(web, backend.url(), backend.token(player), &all.submissions);
    check(judged, format!("cyberctf-web's lab page gets the expected answer to each of {} submissions", all.submissions.len()), &mut all.failures);
    if !judged {
        println!("{output}");
    }
    let leaked = all.secrets.iter().filter(|s| !s.is_empty() && output.contains(s.as_str())).count();
    check(leaked == 0, "cyberctf-web's test output shows no answer".into(), &mut all.failures);
    let backend_log = backend.output();
    let leaked = all.secrets.iter().filter(|s| !s.is_empty() && backend_log.contains(s.as_str())).count();
    check(leaked == 0, format!("the backend's output shows none of the {} values", all.secrets.len()), &mut all.failures);
    all.failures
}

#[tokio::test]
#[ignore = "needs Docker, CyberBackend and cyberctf-web checkouts (their opt-in e2e files), and optionally the invoice-portal-api lab"]
async fn real_flag_labs_the_flag_contract_holds_on_the_runner() {
    let var = |name: &str| PathBuf::from(std::env::var(name).unwrap_or_else(|_| panic!("set {name}")));
    let (backend, web) = (Backend::start(&var("CYBERCTF_TEST_BACKEND")), var("CYBERCTF_TEST_WEB"));
    let failures = contract(&backend, &web, "ada").await;
    drop(backend);
    assert!(failures.is_empty(), "the flag contract failed:\n{}", failures.join("\n"));
}

#[test]
fn a_keyed_flag_is_rfc_4231s_hmac_sha256() {
    // RFC 4231, test cases 2 and 6 (a key longer than the block).
    assert_eq!(hmac_sha256_hex(b"Jefe", b"what do ya want for nothing?"), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    assert_eq!(
        hmac_sha256_hex(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First"),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}
