//! The runner service this app's executable hosts (`serve --home <home>`, internal): whichever
//! client starts it, the app or the command line, serves the other, one service per home.

use std::path::Path;
use std::process::Command;

use isothrum_client::Client;

/// The app's executable, as the clients start it.
const EXE: &str = env!("CARGO_BIN_EXE_cyberctf-desktop");

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
    let home = tempfile::tempdir().unwrap();
    let app = Client::connect_exact(home.path(), Some(Path::new(EXE))).await.unwrap();
    let cli = Client::connect_exact(home.path(), Some(Path::new("/nonexistent/cyberctf"))).await.unwrap();
    assert_eq!(app.service().pid, cli.service().pid, "one service");
    assert_eq!(app.service().version, isothrum_client::VERSION, "of the clients' release");
    app.shutdown().await.unwrap();

    // The other way round: a service another program started (as `cyberctf` starts it) serves
    // the app, which starts none.
    let home = tempfile::tempdir().unwrap();
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
