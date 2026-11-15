//! The command line, `cyberctf`: the launcher's labs, attack boxes, machine setup, servers,
//! cloud accounts and account without the app's window. A test tool first (most of the launcher
//! driven and checked without a UI) and a feature for players and scripts. Every command calls
//! the functions the app's commands call (the shared layer under `commands/`, the run service,
//! the domain): no logic of its own beyond reading arguments and printing. It needs no window
//! system: it never builds a Tauri app, so it runs where no display is (a server, a fresh VM, CI),
//! and the deploy workers it starts run headless too (`deploy_worker::headless_main`). Beside an
//! open app it shares its data folder through the same files, locks and store.

mod account;
mod args;
mod labs;
mod machine;
mod output;
#[cfg(test)]
mod tests;

use std::ffi::OsString;
use std::io::{BufRead, IsTerminal};

use clap::{CommandFactory, FromArgMatches};
use serde_json::Value;

use crate::platform::AppDirs;
use args::{Cli, Command};
pub(crate) use output::{Answer, Exit, Failure, Output};

/// What a command runs with: the app's folders, and whether a person is at a terminal.
pub(crate) struct Context {
    /// The app's folders (the app's own, or `--data-dir`).
    pub dirs: AppDirs,
    /// Whether stdin is a terminal a person answers prompts on.
    pub tty: bool,
    /// The confirmation `--yes` gives.
    pub yes: bool,
}

impl Context {
    /// Asks the person to confirm `what` (a destructive or installing command): yes with `--yes`;
    /// on a terminal, their answer; off one, refused, as a usage error: a script says `--yes`.
    pub fn confirm(&self, out: &mut Output<'_>, what: &str) -> Result<(), Failure> {
        if self.yes {
            return Ok(());
        }
        if !self.tty {
            return Err(Failure::usage(format!("{what}: this needs --yes when not run on a terminal")));
        }
        out.note(&format!("{what}? [y/N]"));
        let mut answer = String::new();
        let _ = std::io::stdin().lock().read_line(&mut answer);
        if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") { Ok(()) } else { Err(Failure::usage("cancelled: nothing was changed")) }
    }

    /// Whether nobody can answer a prompt: `--yes` off a terminal. An elevation then never shows
    /// one (`platform::elevation`).
    pub fn unattended(&self) -> bool {
        self.yes && !self.tty
    }
}

/// The command line's entry (`cyberctf`, see the module), or a deploy worker it started
/// (`cyberctf deploy --job <file>`, headless). Returns the process's exit code.
pub fn main() -> std::process::ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    // A worker on Windows interrupts Terraform through this same executable (see the app's `run`).
    #[cfg(windows)]
    if let Some(pid) = crate::platform::console::interrupt_pid(std::env::args()) {
        return std::process::ExitCode::from(u8::from(!crate::platform::console::send_ctrl_c(pid)));
    }
    // As the app does first (see `crate::run`): rustls's provider, the PATH of the tools it places.
    let _ = rustls::crypto::ring::default_provider().install_default();
    {
        // SAFETY: before the runtime below, the first thread this process starts.
        #[allow(unsafe_code, reason = "the start's claim to be the only thread, as the app's own start makes it")]
        let only = unsafe { crate::platform::sys::SingleThreaded::assume() };
        let own = AppDirs::standard().data().map(crate::machine::tools::ovftool::managed_dir).into_iter().collect::<Vec<_>>();
        crate::platform::env_path::augment(&only, &own);
    }
    let Ok(runtime) = tokio::runtime::Runtime::new() else { return std::process::ExitCode::from(Exit::Failed.code()) };
    // The run service and the runner spawn on the app's async runtime (`crate::spawn`): this one, no app needed.
    crate::use_runtime(runtime.handle().clone());
    let (mut stdout, mut stderr) = (std::io::stdout(), std::io::stderr());
    let tty = std::io::stdin().is_terminal();
    let exit = runtime.block_on(run(args, tty, None, &mut stdout, &mut stderr));
    std::process::ExitCode::from(exit.code())
}

/// Runs the command line `args` (the program's name first), writing to `stdout` and `stderr`;
/// `tty`: whether a person answers prompts; `dirs`: the folders to use instead of the ones the
/// arguments and the OS say (a test's). How it exits.
pub(crate) async fn run(args: Vec<OsString>, tty: bool, dirs: Option<AppDirs>, stdout: &mut dyn std::io::Write, stderr: &mut dyn std::io::Write) -> Exit {
    let json = args.iter().any(|a| a == "--json");
    let parsed = Cli::command().try_get_matches_from(&args).and_then(|m| Ok((name(&m), Cli::from_arg_matches(&m)?)));
    let (command, cli) = match parsed {
        Ok(parsed) => parsed,
        Err(e) => return usage(&e, json, stdout, stderr),
    };
    let mut out = Output::new(cli.json, stdout, stderr);
    out.command = command;
    if cli.verbose {
        log_to_stderr();
    }
    let dirs = dirs.unwrap_or_else(|| cli.data_dir.as_ref().map_or_else(AppDirs::standard, |d| own_dirs(d)));
    let ctx = Context { dirs, tty, yes: cli.yes };
    crate::platform::elevation::set_unattended(ctx.unattended());
    // Boxed: every command's future is held in it, and the callers keep theirs small.
    Box::pin(dispatch(cli.command, &ctx, &mut out)).await
}

/// The folders under `data` (`--data-dir`): the data there, the cache in its `caches/`.
fn own_dirs(data: &std::path::Path) -> AppDirs {
    AppDirs::new(Some(data.to_path_buf()), Some(data.join("caches")))
}

/// A command line clap refused, or its help or version: printed, with its exit.
fn usage(e: &clap::Error, json: bool, stdout: &mut dyn std::io::Write, stderr: &mut dyn std::io::Write) -> Exit {
    use clap::error::ErrorKind;
    if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand) && !json {
        let _ = write!(stdout, "{}", e.render());
        return if e.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand { Exit::Usage } else { Exit::Ok };
    }
    let mut out = Output::new(json, stdout, stderr);
    if json {
        out.fail(&Failure::usage(e.render().to_string().trim().to_owned()))
    } else {
        let _ = write!(stderr, "{}", e.render());
        Exit::Usage
    }
}

/// The command's name in the JSON lines: its subcommands joined by dots (`labs.start`).
fn name(matches: &clap::ArgMatches) -> String {
    let mut words = Vec::new();
    let mut at = matches;
    while let Some((word, sub)) = at.subcommand() {
        words.push(word.to_owned());
        at = sub;
    }
    words.join(".")
}

/// The launcher's log on stderr (`--verbose`): what it does, as the app's log file says it.
fn log_to_stderr() {
    /// A logger writing each record as one line on stderr.
    struct Stderr;
    impl log::Log for Stderr {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() <= log::Level::Info
        }
        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                eprintln!("[{}] {}", record.level(), record.args());
            }
        }
        fn flush(&self) {}
    }
    if log::set_boxed_logger(Box::new(Stderr)).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// Runs `command`.
async fn dispatch(command: Command, ctx: &Context, out: &mut Output<'_>) -> Exit {
    match command {
        Command::Labs(c) => labs::labs(c, ctx, out).await,
        Command::AttackBox(c) => labs::attack_box(c, ctx, out).await,
        Command::Machine(c) => machine::machine(c, ctx, out).await,
        Command::Storage(c) => machine::storage(c, ctx, out).await,
        Command::Doctor => machine::doctor(ctx, out).await,
        Command::Servers(c) => account::servers(c, ctx, out).await,
        Command::Cloud(c) => account::cloud(c, out).await,
        Command::Account(c) => account::account(c, out).await,
        Command::Version => out.finish(Ok(version()), |v| format!("cyberctf {}", v["version"].as_str().unwrap_or_default())),
        Command::Completions { shell } => {
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), "cyberctf", &mut script);
            out.finish(Ok(Value::String(String::from_utf8_lossy(&script).into_owned())), |s| s.as_str().unwrap_or_default().to_owned())
        }
    }
}

/// This launcher's version, with the engines it bundles.
fn version() -> Value {
    serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "isoloom": crate::version::ISOLOOM, "isothrum": crate::version::ISOTHRUM })
}

/// `value` as JSON (a serializable result the shape carries as is).
pub(crate) fn to_json(value: &impl serde::Serialize) -> Answer {
    serde_json::to_value(value).map_err(|e| Failure::from(crate::error::Error::Invalid(format!("couldn't encode the answer: {e}"))))
}

/// The value named `id` of a kind the app names by id (a tool, an engine): `what` names the kind
/// in the usage error for an unknown one.
pub(crate) fn by_id<T: serde::de::DeserializeOwned>(what: &str, id: &str) -> Result<T, Failure> {
    serde_json::from_value(Value::String(id.to_owned())).map_err(|_| Failure::usage(format!("unknown {what} `{id}`")))
}
