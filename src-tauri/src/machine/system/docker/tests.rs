use super::*;

#[test]
fn names_the_engine_from_the_daemon_and_the_context() {
    assert_eq!(engine_named("OrbStack", "default").id(), "orbstack");
    assert_eq!(engine_named("Ubuntu 24.04", "colima").id(), "colima");
    assert_eq!(engine_named("Rancher Desktop WSL Distribution", "default").id(), "rancher-desktop");
    assert_eq!(engine_named("Docker Desktop", "default").id(), "docker-desktop");
    assert_eq!(engine_named("Debian", "desktop-linux").id(), "docker-desktop");
    assert_eq!(engine_named("Ubuntu 24.04 LTS", "default").id(), "docker-engine");
}

#[test]
fn contexts_are_read_with_the_current_one_marked_by_true_or_a_star() {
    assert_eq!(
        contexts_from("default\ttrue\ndesktop-linux\tfalse\norbstack\tfalse\n"),
        [("default".to_string(), true), ("desktop-linux".into(), false), ("orbstack".into(), false)]
    );
    assert_eq!(contexts_from("default *\ncolima-dev\n\n"), [("default".to_string(), true), ("colima-dev".into(), false)]);
    assert_eq!(contexts_from(""), [] as [(String, bool); 0]);
}

#[test]
fn context_names_map_to_engines_and_default_only_counts_on_linux() {
    assert_eq!(context_engine("desktop-linux", Os::Macos).map(Engine::id), Some("docker-desktop"));
    assert_eq!(context_engine("colima-dev", Os::Macos).map(Engine::id), Some("colima"));
    assert_eq!(context_engine("my-remote", Os::Macos).map(Engine::id), None);
    assert_eq!(context_engine("default", Os::Linux).map(Engine::id), Some("docker-engine"));
    assert_eq!(context_engine("podman-machine-default", Os::Macos).map(Engine::id), Some("podman"));
    assert_eq!(context_engine("rancher-desktop", Os::Windows).map(Engine::id), Some("rancher-desktop"));
}

/// Several engines side by side: each context is asked with `docker --context <c> info`, so
/// each engine has its own state, and the current context is marked; one that doesn't
/// answer is still listed from its name, stopped; a context that names no engine and
/// doesn't answer is left out.
#[tokio::test]
async fn every_engine_is_listed_with_its_own_state_and_the_current_one_marked() {
    let tools = Tools::new(|program, args| match (program, args) {
        ("docker", ["context", "ls", ..]) => ok("desktop-linux\tfalse\norbstack\ttrue\ncolima\tfalse\nmy-remote\tfalse\n"),
        ("docker", ["--context", "orbstack", "info", ..]) => ok("OrbStack\n"),
        ("docker", ["--context", "desktop-linux", "info", ..]) => ok("Docker Desktop\n"),
        ("docker", ["--context", ctx, "info", ..]) => fail(format!("Cannot connect to the Docker daemon for {ctx}")),
        _ => ok(""),
    });
    let found = engines().await;
    let brief: Vec<(&str, &str, bool, bool)> = found.iter().map(|e| (e.engine.id(), e.context.as_str(), e.running, e.current)).collect();
    // The current context first, every context probed once.
    assert_eq!(brief, [("orbstack", "orbstack", true, true), ("docker-desktop", "desktop-linux", true, false), ("colima", "colima", false, false)]);
    assert_eq!(running_of(&found), [Engine::Orbstack, Engine::DockerDesktop]);
    assert_eq!(tools.calls().iter().filter(|c| c.contains(" info ")).count(), 4, "{:?}", tools.calls());
}

/// A context that says nothing by its name (`default` on a Mac) is known by the daemon it
/// reaches; two contexts to one engine count once.
#[tokio::test]
async fn a_context_without_a_telling_name_is_known_by_its_daemon() {
    let _tools = Tools::new(|program, args| match (program, args) {
        ("docker", ["context", "ls", ..]) => ok("default\ttrue\ndesktop-linux\tfalse\n"),
        ("docker", ["--context", _, "info", ..]) => ok("Docker Desktop\n"),
        _ => ok(""),
    });
    let found = engines().await;
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].engine.id(), found[0].context.as_str(), found[0].running, found[0].current), ("docker-desktop", "default", true, true));
}

/// Regression (Debian 13's `docker.io` 26.1.5): with the daemon unreachable, `docker info --format
/// …` printed an empty line and exited 0, so the machine check read Docker as running ("Docker
/// Engine 26.1.5") while every lab call failed.
#[tokio::test]
async fn an_empty_docker_info_is_a_daemon_that_doesnt_answer() {
    let tools = Tools::new(move |program, args| match (program, args) {
        ("docker", ["context", "ls", ..]) => ok("default\ttrue\n"),
        ("docker", ["version", ..]) => fail(DOWN),
        _ => ok("\n"),
    });
    let err = daemon_os().await.unwrap_err();
    assert!(err.to_string().contains("Cannot connect to the Docker daemon"), "{err}");
    assert!(!docker_denied(&err), "{err}");
    assert!(tools.called("docker version"), "the reason is asked of docker version: {:?}", tools.calls());
    let found = engines().await;
    assert!(found.iter().all(|e| !e.running), "{found:?}");
    drop(tools);
    // Refused to this account: the same reply tells it apart.
    let _tools = Tools::new(|program, args| match (program, args) {
        ("docker", ["version", ..]) => fail(DENIED),
        _ => ok(""),
    });
    assert!(docker_denied(&daemon_os().await.unwrap_err()));
}

/// A daemon that answers `docker version` but leaves `docker info`'s field empty isn't read
/// as up either: there is no answer to go on.
#[tokio::test]
async fn an_empty_docker_info_from_a_daemon_that_answers_version_is_still_no_answer() {
    let _tools = Tools::new(|program, args| match (program, args) {
        ("docker", ["version", ..]) => ok("linux\n"),
        _ => ok(" \n"),
    });
    let err = daemon_info("{{.ServerVersion}}", &[]).await.unwrap_err();
    assert_eq!(err.to_string(), "docker info --format {{.ServerVersion}} answered nothing, though the Docker daemon answers");
}

#[tokio::test]
async fn an_engine_the_launcher_cannot_start_is_refused_without_running_anything() {
    let err = launch("some-engine").await.unwrap_err();
    assert!(err.to_string().contains("can't start some-engine here"));
}

use std::cell::Cell;
use std::rc::Rc;

use crate::platform::exec::fake::{Reply, Tools, fail, ok};

const DENIED: &str = "permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock";
const DOWN: &str = "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?";

#[tokio::test]
async fn an_engines_context_is_resolved_for_pinning_a_lab_to_it() {
    let _tools = Tools::strict_rules(&[("context ls", ok("default *\norbstack\ncolima-dev\n"))]);
    assert_eq!(engine_context("colima").await.unwrap(), "colima-dev");
    assert_eq!(engine_context("orbstack").await.unwrap(), "orbstack");
    assert_eq!(engine_context("podman").await.unwrap_err().to_string(), "no Docker context for podman");
}

#[tokio::test]
async fn contexts_that_cant_be_listed_say_so_not_that_the_engine_has_none() {
    // Regression: a failed `docker context ls` read as no contexts, and a launch on a chosen
    // engine failed with "no Docker context for orbstack".
    let _tools = Tools::strict_rules(&[("context ls", fail("error: invalid config file"))]);
    let err = engine_context("orbstack").await.unwrap_err().to_string();
    assert!(err.starts_with("couldn't list the Docker contexts to find orbstack's: "), "{err}");
    assert!(err.contains("invalid config file"), "{err}");
}

#[tokio::test]
async fn the_cli_is_pointed_at_the_context_of_the_engine_asked_for() {
    let tools = Tools::strict_rules(&[("docker context use", crate::platform::exec::fake::ok("")), ("context ls", ok("default *\norbstack\ncolima-dev\n"))]);
    docker_use_engine("colima".into()).await.unwrap();
    assert!(tools.called("docker context use colima-dev"), "{:?}", tools.calls());
    let err = docker_use_engine("podman".into()).await.unwrap_err();
    assert_eq!(err.to_string(), "no Docker context for podman");
    assert_eq!(tools.calls().iter().filter(|c| c.contains("context use")).count(), 1);
}

/// Docker with colima installed: the daemon answers `info` (as `daemon`, once colima
/// started) and colima's context shows up once it runs.
fn colima(daemon: impl Fn() -> Option<Reply> + 'static) -> (Tools, Rc<Cell<bool>>) {
    let up = Rc::new(Cell::new(false));
    let started = up.clone();
    let tools = Tools::new(move |program, args| match (program, args.first().copied()) {
        ("colima", Some("start")) => {
            started.set(true);
            ok("")
        }
        ("docker", Some("info")) if !started.get() => fail(DOWN),
        ("docker", Some("info")) => daemon(),
        ("docker", Some("context")) if args.get(1) == Some(&"ls") => ok(if started.get() { "default *\ncolima\n" } else { "default *\n" }),
        _ => ok(""),
    });
    (tools, up)
}

#[tokio::test]
async fn an_engine_is_started_docker_pointed_at_it_and_its_daemon_waited_for() {
    let (tools, up) = colima(|| ok("Ubuntu 24.04 LTS"));
    docker_start_engine("colima".into()).await.unwrap();
    assert!(up.get());
    assert!(tools.called("docker context use colima"), "{:?}", tools.calls());
}

#[tokio::test]
async fn a_daemon_this_account_cannot_use_is_reported_without_waiting() {
    // Running already, for another account: not started again.
    let tools = Tools::strict_rules(&[("docker info", fail(DENIED))]);
    let err = docker_start_engine("colima".into()).await.unwrap_err();
    assert!(matches!(err.code(), crate::error::Code::DockerDenied { .. } | crate::error::Code::DockerDeniedWindows { .. }), "{err}");
    assert!(!tools.called("colima start"));
    drop(tools);
    // Up once started, but denied: said at once, not after 3 minutes.
    let (_tools, up) = colima(|| fail(DENIED));
    let err = docker_start_engine("colima".into()).await.unwrap_err();
    assert!(up.get());
    assert!(matches!(err.code(), crate::error::Code::DockerDenied { .. } | crate::error::Code::DockerDeniedWindows { .. }), "{err}");
}

#[tokio::test]
async fn an_engine_that_cannot_be_started_here_is_refused() {
    let tools = Tools::strict_rules(&[("docker info", fail(DOWN))]);
    let err = docker_start_engine("some-engine".into()).await.unwrap_err();
    assert!(err.to_string().contains("can't start some-engine here"), "{err}");
    assert_eq!(tools.calls(), ["docker info --format {{.OperatingSystem}}"]);
}

#[test]
fn more_engines_by_name() {
    assert_eq!(engine_named("podman", "default").id(), "podman");
    assert_eq!(engine_named("Ubuntu", "rancher-desktop").id(), "rancher-desktop");
    assert_eq!(context_engine("colima", Os::Macos).map(Engine::id), Some("colima"));
    assert_eq!(context_engine("default", Os::Macos).map(Engine::id), None);
}
