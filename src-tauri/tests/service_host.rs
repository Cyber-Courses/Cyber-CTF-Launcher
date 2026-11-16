//! The runner service this app's executable hosts (`serve --home <home>`, internal): whichever
//! client starts it, the app or the command line, serves the other, one service per home.
#![expect(clippy::disallowed_methods, reason = "it runs the executable it tests, as a client does")]
#![allow(clippy::panic, reason = "a test's helpers fail it by panicking, as its assertions do")]

use std::path::Path;
use std::process::Command;

use isothrum_client::Client;

/// The app's executable, as the clients start it.
const EXE: &str = env!("CARGO_BIN_EXE_cyberctf-desktop");

/// A home of the test's own, whose service is shut down when it drops, failed assertion or not:
/// detached, it would serve a deleted folder until it idled out, minutes after the test.
struct Home(tempfile::TempDir);

impl Home {
    fn new() -> Home {
        match tempfile::tempdir() {
            Ok(dir) => Home(dir),
            Err(e) => panic!("no temporary folder: {e}"),
        }
    }

    fn path(&self) -> &Path {
        self.0.path()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        // The test's runtime is busy dropping it: the shutdown runs on a runtime of its own.
        let home = self.0.path().to_path_buf();
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
            runtime.block_on(async {
                let client = Client::connect_running(&home).await.ok()?;
                client.shutdown().await.ok()
            })
        })
        .join();
    }
}

/// The client of `home`'s service, started from executable `exe` when none runs. The client gives
/// a service it started 10 s to answer; a loaded machine can take longer to start one, and a later
/// try then meets the service the first started: tried until it answers, within a minute.
async fn connect_starting(home: &Path, exe: &str) -> Client {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        match Client::connect_exact(home, Some(Path::new(exe))).await {
            Ok(client) => return client,
            Err(isothrum_client::ClientError::Unavailable(why)) if std::time::Instant::now() < deadline => eprintln!("not answering yet: {why}"),
            Err(e) => panic!("the service never answered: {e}"),
        }
    }
}

#[test]
fn serve_refuses_to_run_without_an_absolute_home() {
    let status = Command::new(EXE).arg("serve").status().unwrap();
    assert_eq!(status.code(), Some(2), "no home, no service");
    let status = Command::new(EXE).args(["serve", "--home", "relative"]).status().unwrap();
    assert_eq!(status.code(), Some(2));
}

#[tokio::test]
async fn whichever_starts_the_service_serves_the_other_one_service_per_home() {
    // The app's client starts it; another (the command line's) finds it and starts none: its
    // binary is never run.
    let home = Home::new();
    let app = connect_starting(home.path(), EXE).await;
    let cli = Client::connect_exact(home.path(), Some(Path::new("/nonexistent/cyberctf"))).await.unwrap();
    assert_eq!(app.service().pid, cli.service().pid, "one service");
    assert_eq!(app.service().version, isothrum_client::VERSION, "of the clients' release");
    app.shutdown().await.unwrap();

    // The other way round: a service another program started (as `cyberctf` starts it) serves
    // the app, which starts none.
    let home = Home::new();
    let mut started = Command::new(EXE).args(["serve", "--home"]).arg(home.path()).arg("--detach").spawn().unwrap();
    let mut app = None;
    for _ in 0..200 {
        if let Ok(client) = Client::connect_running(home.path()).await {
            app = Some(client);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let running = app.expect("the service the command line started answers");
    let app = Client::connect_exact(home.path(), Some(Path::new("/nonexistent/cyberctf-desktop"))).await.unwrap();
    assert_eq!(app.service().pid, started.id(), "the command line's service, not a second one");
    drop(running);
    app.shutdown().await.unwrap();
    let _ = started.wait();
}

#[tokio::test]
async fn a_second_service_of_a_served_home_refuses_to_start_and_a_shutdown_ends_the_first() {
    let home = Home::new();
    let app = connect_starting(home.path(), EXE).await;
    let second = Command::new(EXE).args(["serve", "--home"]).arg(home.path()).output().unwrap();
    assert_eq!(second.status.code(), Some(1), "one service per home");
    let said = String::from_utf8_lossy(&second.stderr);
    assert!(said.starts_with("serve: "), "{said}");
    let pid = app.service().pid;
    app.shutdown().await.unwrap();
    // The service is gone: nothing answers for the home any more.
    let mut gone = false;
    for _ in 0..200 {
        if Client::connect_running(home.path()).await.is_err() {
            gone = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(gone, "service {pid} still answers after its shutdown");
}

/// The command line's executable, which its client starts as the service when none runs.
const CLI: &str = env!("CARGO_BIN_EXE_cyberctf");

#[tokio::test]
async fn the_command_lines_own_executable_serves_the_home_its_client_starts() {
    // Regression: `cyberctf serve --home <home>` was an unknown command: the command line's
    // client, finding no service, started it, waited out its start (10 s), and ran the operation
    // in its own process, which the operation died with.
    let refused = Command::new(CLI).args(["serve", "--home", "relative"]).output().unwrap();
    assert_eq!(refused.status.code(), Some(2));
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(said.starts_with("serve: --home must be an absolute path"), "the service's own refusal, not the command line's usage: {said}");

    let home = Home::new();
    let cli = connect_starting(home.path(), CLI).await;
    assert_eq!(cli.service().version, isothrum_client::VERSION);
    let app = Client::connect_exact(home.path(), Some(Path::new("/nonexistent/cyberctf-desktop"))).await.unwrap();
    assert_eq!(app.service().pid, cli.service().pid, "the app is served by the one the command line started");
    app.shutdown().await.unwrap();
}
