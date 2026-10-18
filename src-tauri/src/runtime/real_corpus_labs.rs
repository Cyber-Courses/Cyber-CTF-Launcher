//! Representative 0.11.0 CTF labs through the launcher's two local Docker paths, end to end, for
//! the v1 release QA: for each lab folder named in `CYBERCTF_TEST_DOCKER_LABS` (comma-separated),
//! and through both the launcher's own Docker code and the Isothrum runner, start it with every
//! input empty (the development value), assert it reaches ready (running, every non-infra machine
//! up), run the lab's own checks and assert they pass, park and resume it, then stop it and assert
//! nothing of it is left. The lab's own checks are the solve/graded proof the brief allows.
//!
//! Opt-in (it needs Docker and the labs' own images):
//!
//! ```text
//! CYBERCTF_TEST_DOCKER_LABS=/path/to/lab-a,/path/to/lab-b \
//! cargo test --lib -- --ignored --nocapture --test-threads=1 corpus_docker_labs
//! ```
//!
//! The lab content is private; only its folder path is named here, never its contents.

use std::path::Path;

use super::real_labs::{Run, check, install_copy, leftovers, log};
use super::{Place, docker, paths};
use crate::platform::exec::run;

/// The lab up: running, in containers here, every non-infra machine up.
async fn ready(r: &Run, f: &mut Vec<String>) {
    match r.status().await {
        Ok(s) => {
            check(s.running, format!("{}: it reports running", r.id), f);
            check(matches!(s.place, Some(Place::Container)), format!("{}: it runs in containers here", r.id), f);
            check(!s.machines.is_empty(), format!("{}: it has machines", r.id), f);
            for m in s.machines.iter().filter(|m| !m.infra) {
                check(m.state == "running", format!("{}: machine {} is running (state {})", r.id, m.name, m.state), f);
            }
        }
        Err(e) => check(false, format!("{}: its status reads: {e}", r.id), f),
    }
}

/// The lab's own checks all pass.
async fn checks_pass(r: &Run, f: &mut Vec<String>) {
    match r.check().await {
        Ok(c) => {
            for res in &c.results {
                println!("  check {} {} ({})", if res.ok { "passed" } else { "FAILED" }, res.name, res.from);
            }
            check(c.available && c.ok && !c.results.is_empty(), format!("{}: its {} checks pass", r.id, c.results.len()), f);
            if !c.ok {
                println!("{}", c.output);
            }
        }
        Err(e) => check(false, format!("{}: its checks run: {e}", r.id), f),
    }
}

/// Park the lab (its containers stopped, kept), then resume it; it reads as parked between, and
/// runs again after.
async fn park_and_resume(r: &Run, f: &mut Vec<String>) {
    let parked = r.park(&mut log(&r.id)).await;
    check(parked.is_ok(), format!("{}: it parks ({:?})", r.id, parked.err()), f);
    let s = r.status().await;
    check(s.as_ref().is_ok_and(|s| !s.running && s.parked.is_some()), format!("{}: parked, it reads as parked", r.id), f);
    let resumed = r.resume(&mut log(&r.id)).await;
    check(resumed.is_ok() && paths::parked(&r.dir).is_none(), format!("{}: it resumes ({:?})", r.id, resumed.err()), f);
    ready(r, f).await;
}

/// One lab through the runner, start to teardown; every failure is collected so the lab is always
/// stopped after. Launched with no inputs: each slot takes its manifest default.
async fn run_corpus_lab(dir: &Path, f: &mut Vec<String>) {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("lab");
    let id = format!("ci-corpus-{}", rand::random::<u32>());
    let r = Run { dir: install_copy(dir, &id), home: std::env::temp_dir().join(format!("cyberctf-{id}-home")), id };
    let started = std::time::Instant::now();
    match r.start(&[], &mut log(&r.id)).await {
        Ok(()) => {
            ready(&r, f).await;
            checks_pass(&r, f).await;
            park_and_resume(&r, f).await;
        }
        Err(e) => check(false, format!("{name}: it starts: {e}"), f),
    }
    teardown(&r, name, f).await;
    println!("{name}: {:.0?}", started.elapsed());
    let _ = std::fs::remove_dir_all(&r.dir);
    let _ = std::fs::remove_dir_all(&r.home);
}

/// Stop the lab and assert nothing of it is left, and that the runner records the run gone.
async fn teardown(r: &Run, name: &str, f: &mut Vec<String>) {
    if !f.is_empty() {
        let logs = run("docker", &["compose", "-p", &docker::project(&r.id), "logs", "--no-color", "--tail", "120"], None).await;
        println!("compose logs ({name}):\n{}", logs.unwrap_or_else(|e| e.to_string()));
    }
    let stopped = r.stop(log(&r.id)).await;
    check(stopped.is_ok(), format!("{name}: it stops ({:?})", stopped.err()), f);
    let left = leftovers(&r.id).await;
    check(left.is_empty(), format!("{name}: nothing of it is left ({left:?})"), f);
    // Gone means either recorded absent, or never recorded at all (a start that failed before
    // generation wrote no record).
    let record = std::fs::read_to_string(r.home.join("runs").join(docker::project(&r.id)).join("record.json")).unwrap_or_default();
    let gone = record.is_empty() || record.contains(r#""state": "absent""#);
    check(gone, format!("{name}: the runner records the run as gone"), f);
}

#[tokio::test]
#[ignore = "needs Docker and corpus lab folders (CYBERCTF_TEST_DOCKER_LABS)"]
async fn corpus_docker_labs_run_end_to_end_on_the_runner() {
    let Some(labs) = std::env::var_os("CYBERCTF_TEST_DOCKER_LABS") else { return };
    let labs = labs.to_string_lossy().into_owned();
    let dirs: Vec<&str> = labs.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let mut failures = Vec::new();
    for dir in dirs {
        run_corpus_lab(Path::new(dir), &mut failures).await;
    }
    assert!(failures.is_empty(), "corpus labs failed:\n{}", failures.join("\n"));
}
