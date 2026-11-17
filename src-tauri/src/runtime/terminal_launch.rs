//! Opening the OS terminal on the attack box shell: the runner builds the command (program,
//! arguments and the run's folder — see [`isothrum_runner::Running::console_command`]), and
//! isothrum-machine opens it in this computer's terminal ([`isothrum_machine::terminal::open`]),
//! its window titled "Cyber CTF attack box". The in-app pseudo-terminal ([`crate::terminal`])
//! runs the same command without a terminal window; this is the "open in a system terminal" path,
//! so the player gets a real terminal they fully control.

use isothrum_machine::terminal::{self, NotOpened};
use isothrum_runner::ShellCommand;

use crate::error::{Error, Result};

/// The terminal window's title.
const TITLE: &str = "Cyber CTF attack box";

/// The way out when macOS doesn't let the app drive Terminal (the Automation permission).
const AUTOMATION_HINT: &str = "Allow Cyber CTF to control Terminal in System Settings > Privacy & Security > Automation, then try again.";

/// Opens the OS terminal on `command`. The Windows console's script goes to the temp folder.
pub async fn open_terminal_async(command: ShellCommand) -> Result<()> {
    let cx = crate::runtime::providers::machine();
    terminal::open(&cx, TITLE, &command, &std::env::temp_dir()).await.map_err(not_opened)
}

/// Why no terminal opened, in the player's words.
fn not_opened(e: NotOpened) -> Error {
    match e {
        NotOpened::Automation { said } => Error::Invalid(format!("couldn't open Terminal: {said}. {AUTOMATION_HINT}")),
        NotOpened::Failed { program, said } if program == "osascript" => Error::Invalid(format!("couldn't open Terminal: {said}.")),
        NotOpened::NoTerminal { line, .. } => Error::Invalid(format!("couldn't open a terminal; run this yourself: {line}")),
        other => Error::Invalid(format!("couldn't open a terminal: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use isothrum_machine::{Arch, HostOs};
    use isothrum_runner::fake::{FakeProcesses, Reply};

    use super::*;
    use crate::runtime::providers::testing::{Machine, host_on};

    fn shell() -> ShellCommand {
        ShellCommand::new("docker", ["compose", "exec", "kali", "bash"], PathBuf::from("/runs/x y"))
    }

    /// Regression: osascript was only spawned, so a refusal (the Automation permission for
    /// Terminal denied) left the button doing nothing, silently.
    #[tokio::test]
    async fn a_terminal_that_macos_wont_let_the_app_open_says_why() {
        let tools = FakeProcesses::model(|_| Reply::fail(1, "execution error: Not authorized to send Apple events to Terminal. (-1743)"));
        let _machine = Machine::set(Arc::new(tools), host_on(HostOs::Macos, Arch::Arm64, None));
        let said = open_terminal_async(shell()).await.unwrap_err().to_string();
        assert!(said.contains("Not authorized to send Apple events to Terminal") && said.contains("Privacy & Security > Automation"), "{said}");
    }

    /// No terminal on a Linux desktop: the command to run by hand is said.
    #[tokio::test]
    async fn no_terminal_says_the_command_to_run_by_hand() {
        let _machine = Machine::set(Arc::new(FakeProcesses::model(|_| Reply::missing())), host_on(HostOs::Linux, Arch::Amd64, None));
        let said = open_terminal_async(shell()).await.unwrap_err().to_string();
        assert!(said.starts_with("couldn't open a terminal; run this yourself: ") && said.contains("docker compose exec kali bash"), "{said}");
    }

    /// The window is titled as the launcher's.
    #[tokio::test]
    async fn the_window_is_titled_as_the_attack_box() {
        let tools = Arc::new(FakeProcesses::model(|_| Reply::ok("")));
        let _machine = Machine::set(Arc::<FakeProcesses>::clone(&tools), host_on(HostOs::Macos, Arch::Arm64, None));
        open_terminal_async(shell()).await.unwrap();
        let calls = tools.calls();
        assert!(calls.iter().any(|c| c.program == "osascript" && c.args.iter().any(|a| a.contains(TITLE))), "{calls:?}");
    }
}
