//! The launcher's side of the engines: isothrum-machine's answers in its errors, and starting an
//! engine. How engines and contexts are read is isothrum-machine's, tested there.

use isothrum_runner::fake::{FakeProcesses, Reply};

use super::*;
use crate::runtime::providers::testing::{Faked, fake};
use crate::testing::Switch;

/// `docker info`'s operating system line, as the launcher's error when the daemon gives none.
async fn daemon_os() -> Result<String> {
    isothrum_machine::docker::daemon_os(&machine()).await.map_err(no_answer)
}

const DENIED: &str = "permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock";
const DOWN: &str = "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?";

#[tokio::test]
async fn a_daemon_that_refuses_this_account_is_the_denied_error_and_a_stopped_one_isnt() {
    let _machine = fake(FakeProcesses::script().expect("docker info", Reply::fail(1, DENIED)).build());
    // Told apart by isothrum-machine (`says_denied`), each in the CLI's own words.
    assert!(matches!(daemon_os().await.unwrap_err(), Error::CommandFailed { stderr, .. } if stderr.contains(DENIED)));
    let _machine = fake(FakeProcesses::script().expect("docker info", Reply::fail(1, DOWN)).build());
    let err = daemon_os().await.unwrap_err();
    assert!(
        matches!(&err, Error::CommandFailed { stderr, .. } if stderr.contains(DOWN)) && err.to_string().contains("Cannot connect to the Docker daemon"),
        "{err}"
    );
    let _machine = fake(FakeProcesses::script().expect("docker", Reply::missing()).build());
    assert!(matches!(daemon_os().await.unwrap_err(), Error::ToolMissing { tool: "docker" }));
}

#[tokio::test]
async fn an_engines_context_is_resolved_for_pinning_a_lab_to_it() {
    let _machine = fake(FakeProcesses::script().allow("docker context ls", Reply::ok("default *\norbstack\ncolima-dev\n")).build());
    assert_eq!(engine_context("colima").await.unwrap(), "colima-dev");
    assert_eq!(engine_context("podman").await.unwrap_err().to_string(), "no Docker context for podman");
    assert_eq!(engine_context("some-engine").await.unwrap_err().to_string(), "no Docker context for some-engine");
}

#[tokio::test]
async fn contexts_that_cant_be_listed_say_so_not_that_the_engine_has_none() {
    let _machine = fake(FakeProcesses::script().expect("docker context ls", Reply::fail(1, "error: invalid config file")).build());
    let err = engine_context("orbstack").await.unwrap_err().to_string();
    assert!(err.starts_with("couldn't list the Docker contexts to find orbstack's: "), "{err}");
    assert!(err.contains("invalid config file"), "{err}");
}

#[tokio::test]
async fn the_cli_is_pointed_at_the_context_of_the_engine_asked_for() {
    let script = FakeProcesses::script().allow("docker context ls", Reply::ok("default *\norbstack\ncolima-dev\n")).expect_times(
        "docker context use colima-dev",
        1,
        Reply::ok(""),
    );
    let _machine = fake(script.build());
    docker_use_engine("colima".into()).await.unwrap();
    assert_eq!(docker_use_engine("podman".into()).await.unwrap_err().to_string(), "no Docker context for podman");
}

/// An engine a setup starts on this computer: its id, the program its start runs, and the Docker
/// context it shows once up. Colima's CLI on macOS and Linux; on Windows, where colima doesn't
/// run, Docker Desktop's program (opened by PowerShell).
const ENGINE: (&str, &str, &str) = if cfg!(windows) { ("docker-desktop", "powershell", "desktop-linux") } else { ("colima", "colima", "colima") };

/// Docker with [`ENGINE`] installed: its start brings its daemon up, which then answers `info`
/// as `daemon` does, and its context shows up.
fn startable(daemon: Reply) -> (Faked, Switch) {
    let (_, program, context) = ENGINE;
    let up = Switch::default();
    let running = up.clone();
    let tools = fake(FakeProcesses::model(move |call| {
        let up = running.is_on();
        match (call.program.as_str(), call.args.first().map(String::as_str)) {
            (p, _) if p == program => {
                running.turn_on();
                Reply::ok("")
            }
            (_, Some("info")) if !up => Reply::fail(1, DOWN),
            (_, Some("info")) => daemon.clone(),
            (_, Some("context")) if call.args.get(1).map(String::as_str) == Some("ls") => {
                Reply::ok(&if up { format!("default *\n{context}\n") } else { "default *\n".to_owned() })
            }
            _ => Reply::ok(""),
        }
    }));
    (tools, up)
}

/// Whether `tools` ran [`ENGINE`]'s start.
fn started(tools: &FakeProcesses) -> bool {
    tools.calls().iter().any(|c| c.program == ENGINE.1)
}

#[tokio::test]
async fn an_engine_is_started_docker_pointed_at_it_and_its_daemon_waited_for() {
    let (tools, up) = startable(Reply::ok("Ubuntu 24.04 LTS"));
    docker_start_engine(ENGINE.0.into()).await.unwrap();
    assert!(up.is_on() && started(&tools));
}

#[tokio::test]
async fn a_daemon_this_account_cannot_use_is_reported_without_waiting() {
    // Running already, for another account: not started again.
    let tools = fake(FakeProcesses::model(|call| match call.args.first().map(String::as_str) {
        Some("info") => Reply::fail(1, DENIED),
        _ => Reply::ok(""),
    }));
    let err = docker_start_engine(ENGINE.0.into()).await.unwrap_err();
    assert!(matches!(err.code(), crate::error::Code::DockerDenied { .. } | crate::error::Code::DockerDeniedWindows { .. }), "{err}");
    assert!(!started(&tools));
    // Up once started, but denied: said at once, not after 3 minutes.
    let (tools, up) = startable(Reply::fail(1, DENIED));
    let err = docker_start_engine(ENGINE.0.into()).await.unwrap_err();
    assert!(up.is_on() && started(&tools));
    assert!(matches!(err.code(), crate::error::Code::DockerDenied { .. } | crate::error::Code::DockerDeniedWindows { .. }), "{err}");
}

#[tokio::test]
async fn an_engine_that_cannot_be_started_here_is_refused() {
    let tools = fake(FakeProcesses::model(|_| Reply::fail(1, DOWN)));
    for engine in ["some-engine", "podman"] {
        let err = docker_start_engine(engine.into()).await.unwrap_err();
        assert!(err.to_string().contains(&format!("can't start {engine} here")), "{err}");
    }
    assert_eq!(tools.calls().len(), 0, "nothing run");
}
