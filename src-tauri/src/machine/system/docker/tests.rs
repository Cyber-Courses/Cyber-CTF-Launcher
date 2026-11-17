//! The launcher's side of the engines: isothrum-machine's answers in its errors, and starting an
//! engine. How engines and contexts are read is isothrum-machine's, tested there.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use isothrum_runner::fake::{FakeProcesses, Reply};

use super::*;
use crate::platform::exec::fake::{Tools, ok};
use crate::runtime::providers::testing::{Machine, host};

const DENIED: &str = "permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock";
const DOWN: &str = "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?";

/// isothrum-machine's Docker calls answered by `tools` on this test's thread, until dropped.
fn docker(tools: FakeProcesses) -> (Machine, Arc<FakeProcesses>) {
    let tools = Arc::new(tools);
    let runner: Arc<dyn isothrum_runner::ProcessRunner> = Arc::<FakeProcesses>::clone(&tools);
    (Machine::set(runner, host(std::path::Path::new("/nonexistent/cyberctf-test-machine"))), tools)
}

#[tokio::test]
async fn a_daemon_that_refuses_this_account_is_the_denied_error_and_a_stopped_one_isnt() {
    let _machine = docker(FakeProcesses::script().expect("docker info", Reply::fail(1, DENIED)).build());
    assert!(docker_denied(&daemon_os().await.unwrap_err()));
    let _machine = docker(FakeProcesses::script().expect("docker info", Reply::fail(1, DOWN)).build());
    let err = daemon_os().await.unwrap_err();
    assert!(!docker_denied(&err) && err.to_string().contains("Cannot connect to the Docker daemon"), "{err}");
    let _machine = docker(FakeProcesses::script().expect("docker", Reply::missing()).build());
    assert!(matches!(daemon_os().await.unwrap_err(), Error::ToolMissing { tool: "docker" }));
}

#[tokio::test]
async fn an_engines_context_is_resolved_for_pinning_a_lab_to_it() {
    let _machine = docker(FakeProcesses::script().allow("docker context ls", Reply::ok("default *\norbstack\ncolima-dev\n")).build());
    assert_eq!(engine_context("colima").await.unwrap(), "colima-dev");
    assert_eq!(engine_context("podman").await.unwrap_err().to_string(), "no Docker context for podman");
    assert_eq!(engine_context("some-engine").await.unwrap_err().to_string(), "no Docker context for some-engine");
}

#[tokio::test]
async fn contexts_that_cant_be_listed_say_so_not_that_the_engine_has_none() {
    let _machine = docker(FakeProcesses::script().expect("docker context ls", Reply::fail(1, "error: invalid config file")).build());
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
    let _machine = docker(script.build());
    docker_use_engine("colima".into()).await.unwrap();
    assert_eq!(docker_use_engine("podman".into()).await.unwrap_err().to_string(), "no Docker context for podman");
}

/// Docker with colima installed: `colima start` (the launcher's) brings its daemon up, which then
/// answers `info` as `daemon` does, and its context shows up.
fn colima(daemon: Reply) -> (Tools, Machine, Arc<AtomicBool>) {
    let up = Arc::new(AtomicBool::new(false));
    let started = Arc::clone(&up);
    let tools = Tools::new(move |program, args| match (program, args.first().copied()) {
        ("colima", Some("start")) => {
            started.store(true, Ordering::SeqCst);
            ok("")
        }
        _ => None,
    });
    let running = Arc::clone(&up);
    let machine = docker(FakeProcesses::model(move |call| {
        let up = running.load(Ordering::SeqCst);
        match call.args.first().map(String::as_str) {
            Some("info") if !up => Reply::fail(1, DOWN),
            Some("info") => daemon.clone(),
            Some("context") if call.args.get(1).map(String::as_str) == Some("ls") => Reply::ok(if up { "default *\ncolima\n" } else { "default *\n" }),
            _ => Reply::ok(""),
        }
    }));
    (tools, machine.0, up)
}

#[tokio::test]
async fn an_engine_is_started_docker_pointed_at_it_and_its_daemon_waited_for() {
    let (_tools, _machine, up) = colima(Reply::ok("Ubuntu 24.04 LTS"));
    docker_start_engine("colima".into()).await.unwrap();
    assert!(up.load(Ordering::SeqCst));
}

#[tokio::test]
async fn a_daemon_this_account_cannot_use_is_reported_without_waiting() {
    // Running already, for another account: not started again.
    let tools = Tools::new(|_, _| None);
    let _machine = docker(FakeProcesses::script().expect("docker info", Reply::fail(1, DENIED)).build());
    let err = docker_start_engine("colima".into()).await.unwrap_err();
    assert!(matches!(err.code(), crate::error::Code::DockerDenied { .. } | crate::error::Code::DockerDeniedWindows { .. }), "{err}");
    assert!(!tools.called("colima start"));
    drop(tools);
    // Up once started, but denied: said at once, not after 3 minutes.
    let (_tools, _machine, up) = colima(Reply::fail(1, DENIED));
    let err = docker_start_engine("colima".into()).await.unwrap_err();
    assert!(up.load(Ordering::SeqCst));
    assert!(matches!(err.code(), crate::error::Code::DockerDenied { .. } | crate::error::Code::DockerDeniedWindows { .. }), "{err}");
}

#[tokio::test]
async fn an_engine_that_cannot_be_started_here_is_refused() {
    let tools = Tools::new(|_, _| None);
    let _machine = docker(FakeProcesses::script().expect("docker info", Reply::fail(1, DOWN)).build());
    let err = docker_start_engine("some-engine".into()).await.unwrap_err();
    assert!(err.to_string().contains("can't start some-engine here"), "{err}");
    assert_eq!(tools.calls(), [] as [String; 0]);
    assert!(launch("some-engine").await.unwrap_err().to_string().contains("can't start some-engine here"));
}
