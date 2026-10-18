//! Windows has no SIGINT: a console program's clean stop is a Ctrl+C on its console. Terraform
//! stops on one the way it does on SIGINT (it finishes what it is creating and saves its state).
//!
//! Sending it means attaching to the target's console, which changes the caller's own console
//! state for the whole process. So the app never does it itself: it runs its own executable as
//! `cyberctf-desktop interrupt --pid <pid>`, a throwaway process that attaches, sends, and exits.
//!
//! The target must take Ctrl+C. A process started in a process group of its own, as the deploy
//! worker is, starts with Ctrl+C ignored, and the programs it starts inherit that: the worker
//! takes it again first ([`let_tools_take_ctrl_c`]).

/// The pid when this process was started as an interrupter (`<exe> interrupt --pid <pid>`).
#[cfg(any(windows, test))]
pub fn interrupt_pid(args: impl IntoIterator<Item = String>) -> Option<u32> {
    let args: Vec<String> = args.into_iter().collect();
    match args.as_slice() {
        [_, cmd, flag, pid, ..] if cmd == "interrupt" && flag == "--pid" => pid.parse().ok().filter(|&p| p > 4),
        _ => None,
    }
}

/// Sends Ctrl+C to every process on `pid`'s console (the program and what it started there, e.g.
/// Terraform and its providers). Whether it was sent. Run in a process of its own (see above).
#[cfg(windows)]
pub fn send_ctrl_c(pid: u32) -> bool {
    crate::platform::sys::send_ctrl_c(pid)
}

/// Lets the console programs this process starts from now on stop on a Ctrl+C. The deploy worker
/// has no console, so it takes none itself; without this its Terraform ignored the stop's Ctrl+C.
#[cfg(windows)]
pub fn let_tools_take_ctrl_c() {
    if !crate::platform::sys::take_ctrl_c() {
        log::warn!("couldn't let this process's tools take Ctrl+C: a stop will end Terraform without its clean stop");
    }
}

/// Asks the throwaway interrupter to Ctrl+C `pid`; whether it reported success.
#[cfg(windows)]
pub fn interrupt(pid: u32) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["interrupt", "--pid", &pid.to_string()]);
    crate::platform::exec::headless_std(&mut cmd).status().is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn only_interrupt_pid_arguments_make_an_interrupter() {
        assert_eq!(interrupt_pid(args(&["app", "interrupt", "--pid", "4242"])), Some(4242));
        assert_eq!(interrupt_pid(args(&["app", "interrupt", "--pid"])), None);
        assert_eq!(interrupt_pid(args(&["app", "interrupt", "--pid", "abc"])), None);
        assert_eq!(interrupt_pid(args(&["app", "deploy", "--job", "x"])), None);
        // System processes (0 idle, 4 System) are never a target.
        assert_eq!(interrupt_pid(args(&["app", "interrupt", "--pid", "4"])), None);
        assert_eq!(interrupt_pid(args(&["app"])), None);
    }

    /// The interrupter side, run in a child process by the test below (it detaches from its
    /// console, which the test harness must keep): `CYBERCTF_CTRL_C_PID` names the target.
    #[cfg(windows)]
    #[test]
    #[ignore = "run as a child process by a_tool_of_the_deploy_worker_stops_on_ctrl_c"]
    fn ctrl_c_helper() {
        let pid: u32 = std::env::var("CYBERCTF_CTRL_C_PID").unwrap().parse().unwrap();
        assert!(send_ctrl_c(pid));
    }

    /// The test's own executable, running the ignored test `name` as a child process.
    #[cfg(windows)]
    fn helper(name: &str) -> std::process::Command {
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", &format!("platform::console::tests::{name}"), "--ignored", "--nocapture", "--test-threads", "1"]);
        cmd
    }

    /// A tool: a console program that runs until it is stopped (it waits for the end of its input,
    /// which its parent keeps open). On a Ctrl+C, Windows ends it with `STATUS_CONTROL_C_EXIT`.
    #[cfg(windows)]
    #[test]
    #[ignore = "run as a child process by tool_parent_helper"]
    fn tool_helper() {
        let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
    }

    /// The deploy worker's stand-in: started as the worker is, it takes Ctrl+C again as the worker
    /// does, starts a tool the way it starts its tools (with a hidden console of its own), and
    /// prints the tool's pid, then its exit code once it ended.
    #[cfg(windows)]
    #[test]
    #[ignore = "run as a child process by a_tool_of_the_deploy_worker_stops_on_ctrl_c"]
    fn tool_parent_helper() {
        let_tools_take_ctrl_c();
        let mut tool = helper("tool_helper");
        crate::platform::exec::headless_std(&mut tool).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null());
        let mut tool = tool.spawn().unwrap();
        // Kept open while the tool runs (`wait` would close it first).
        let _input = tool.stdin.take();
        println!("{TOOL_PID}{}", tool.id());
        let code = tool.wait().unwrap().code().map_or_else(|| "none".into(), |c| format!("{:#x}", c.cast_unsigned()));
        println!("{TOOL_EXIT}{code}");
    }

    #[cfg(windows)]
    const TOOL_PID: &str = "tool pid ";
    #[cfg(windows)]
    const TOOL_EXIT: &str = "tool exit ";

    /// The stand-in's process tree, ended however the test ends.
    #[cfg(windows)]
    struct Tree(std::process::Child);

    #[cfg(windows)]
    impl Drop for Tree {
        fn drop(&mut self) {
            if matches!(self.0.try_wait(), Ok(None)) {
                let _ = std::process::Command::new("taskkill").args(["/T", "/F", "/PID", &self.0.id().to_string()]).output();
            }
            let _ = self.0.wait();
        }
    }

    /// A tool of the deploy worker stops on the interrupter's Ctrl+C, rather than being killed. The
    /// worker is started in a process group of its own, which Windows starts with Ctrl+C ignored,
    /// and its tools inherited that: a stop's Ctrl+C never reached Terraform. The stand-in is
    /// started the same way here, so the test doesn't depend on how it was itself started (the
    /// processes of an SSH session ignore Ctrl+C too).
    #[cfg(windows)]
    #[test]
    fn a_tool_of_the_deploy_worker_stops_on_ctrl_c() {
        use std::io::BufRead;
        use std::os::windows::process::CommandExt;
        use std::time::{Duration, Instant};
        use windows_sys::Win32::Foundation::STATUS_CONTROL_C_EXIT;
        use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};
        let mut parent = Tree(
            helper("tool_parent_helper").creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP).stdout(std::process::Stdio::piped()).spawn().unwrap(),
        );
        let stdout = parent.0.stdout.take().unwrap();
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || std::io::BufReader::new(stdout).lines().map_while(Result::ok).try_for_each(|l| tx.send(l)));
        // What the stand-in prints after `prefix` (the harness's `test … ...` may come first on the
        // line), waiting at most until `deadline`.
        let next = |prefix: &str, deadline: Instant| loop {
            match lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(line) => {
                    if let Some((_, rest)) = line.split_once(prefix) {
                        break Some(rest.trim().to_string());
                    }
                }
                Err(_) => break None,
            }
        };
        let tool = next(TOOL_PID, Instant::now() + Duration::from_secs(60)).expect("the worker's stand-in started no tool");
        let sent = helper("ctrl_c_helper").env("CYBERCTF_CTRL_C_PID", &tool).creation_flags(crate::platform::exec::CREATE_NO_WINDOW).output().unwrap();
        assert!(sent.status.success(), "the interrupter failed: {}", String::from_utf8_lossy(&sent.stdout));
        let exit = next(TOOL_EXIT, Instant::now() + Duration::from_secs(10));
        // Ended by Windows' own Ctrl+C handling: not killed, not ended some other way.
        assert_eq!(exit, Some(format!("{:#x}", STATUS_CONTROL_C_EXIT.cast_unsigned())), "the tool didn't stop on the Ctrl+C");
    }
}
