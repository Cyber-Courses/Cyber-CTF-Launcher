//! The runner service this app hosts: its own executable, run as `<exe> serve --home <home>
//! [--detach]` (internal, never listed), is the Isothrum runner service of a home. The app and the
//! `cyberctf` command line connect with `isothrum_client::Client::connect_exact`, which starts this
//! executable as the service when none of this release runs, so the service a client meets is
//! always of the client's own release: whichever starts it first serves the other, and an older one
//! left by a partial update is replaced (or, still busy, said so). One service per home, whatever
//! starts it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use isothrum_runner::{Runner, RunnerConfig, SystemProcesses};
use isothrum_service::{RunnerEngine, ServeOptions, Service};

/// How long a detached service waits with no client and no operation before it exits: it doesn't
/// outlive the app (or the command line) that started it for long.
const IDLE: Duration = Duration::from_secs(300);

/// What `serve` was asked: the home, and whether it runs detached (started by a client).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Serve {
    /// The runner's home.
    pub(crate) home: PathBuf,
    /// Started by a client, its terminal not its own: it idles out.
    pub(crate) detach: bool,
}

/// The `serve` request in `args` (the program's arguments, its name first): none when the
/// program isn't run as the service; an error when it is but names no home (it never guesses one).
pub(crate) fn request(args: impl IntoIterator<Item = String>) -> Option<std::result::Result<Serve, String>> {
    let args: Vec<String> = args.into_iter().skip(1).collect();
    if args.first().map(String::as_str) != Some("serve") {
        return None;
    }
    let mut home = None;
    let mut detach = false;
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--home" => home = rest.next().map(PathBuf::from),
            "--detach" => detach = true,
            other => return Some(Err(format!("serve: unknown argument `{other}`"))),
        }
    }
    Some(match home {
        Some(home) if home.is_absolute() => Ok(Serve { home, detach }),
        Some(home) => Err(format!("serve: --home must be an absolute path, not `{}`", home.display())),
        None => Err("serve: --home <home> is required".into()),
    })
}

/// This process's exit code as the runner service, when `args` (the program's, its name first)
/// ask for it (`serve`, see [`request`]): served until it ends, or 2 when the request is wrong (said
/// on stderr). None when the program isn't run as the service. Both executables answer it: each
/// client starts its own executable as the service (`Client::connect_exact`).
pub(crate) fn serve_if_asked(args: impl IntoIterator<Item = String>) -> Option<i32> {
    Some(match request(args)? {
        Ok(serve) => main(serve),
        Err(why) => {
            eprintln!("{why}");
            2
        }
    })
}

/// Runs the service `serve` asks for, in this process, until it is asked to stop (a client's
/// `shutdown`, Ctrl-C) or, detached, idles out: the process's exit code.
pub(crate) fn main(serve: Serve) -> i32 {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("serve: no runtime: {e}");
            return 1;
        }
    };
    match runtime.block_on(run(serve)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("serve: {e}");
            1
        }
    }
}

/// The runner the service of `home` runs, on this host's tools (`processes`): a run's docker
/// calls go to the Docker context its start recorded (the runner's own).
pub(crate) fn runner_config(home: &std::path::Path, processes: &SystemProcesses) -> RunnerConfig {
    let mut config = RunnerConfig::new(home);
    config.processes = Arc::new(processes.clone());
    config
}

/// Serves `serve.home` (see [`main`]).
async fn run(serve: Serve) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let service = Service::hold(serve.home)?;
    let processes = SystemProcesses::default();
    let runner = Arc::new(Runner::new(runner_config(service.home(), &processes))?);
    let stop: isothrum_service::StopSignal = Box::new(|| {
        Box::pin(async {
            if tokio::signal::ctrl_c().await.is_err() {
                // No signal handler: a client's `shutdown` or the idle time stop the service.
                std::future::pending::<()>().await;
            }
        })
    });
    let mut options = ServeOptions::new(Arc::new(RunnerEngine::new(runner)), stop);
    options.tools = Some(processes);
    options.idle = serve.detach.then_some(IDLE);
    // The launcher reaps its own expired labs (the app's reaper, with the account's credentials a
    // cloud lab's `down` needs, which the service has none of): the service's reaper never runs.
    options.reap_every = None;
    service.serve(options).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Serve, request};
    use std::path::PathBuf;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("cyberctf-desktop").chain(list.iter().copied()).map(str::to_owned).collect()
    }

    #[test]
    fn serve_is_asked_for_by_name_with_an_absolute_home_and_never_guesses_one() {
        assert_eq!(request(args(&[])), None, "the app itself");
        assert_eq!(request(args(&["cyberctf://lab/x"])), None, "a deep link");
        assert_eq!(request(args(&["labs", "serve"])), None, "serve only as the first argument");
        let home = if cfg!(windows) { r"C:\data\isothrum" } else { "/data/isothrum" };
        assert_eq!(request(args(&["serve", "--home", home, "--detach"])), Some(Ok(Serve { home: PathBuf::from(home), detach: true })));
        assert_eq!(request(args(&["serve", "--home", home])), Some(Ok(Serve { home: PathBuf::from(home), detach: false })));
        assert!(request(args(&["serve"])).unwrap().unwrap_err().contains("--home <home> is required"));
        assert!(request(args(&["serve", "--home", "relative"])).unwrap().unwrap_err().contains("absolute"));
        assert!(request(args(&["serve", "--home", home, "--port", "1"])).unwrap().unwrap_err().contains("unknown argument `--port`"));
    }
}
