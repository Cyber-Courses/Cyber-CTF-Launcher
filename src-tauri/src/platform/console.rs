//! Windows has no SIGINT: a console program's clean stop is a Ctrl+C on its console. Terraform
//! stops on one the way it does on SIGINT (it finishes what it is creating and saves its state).
//!
//! Sending it means attaching to the target's console, which changes the caller's own console
//! state for the whole process. So the app never does it itself: it runs its own executable as
//! `cyberctf-desktop interrupt --pid <pid>`, a throwaway process that attaches, sends, and exits.

/// The pid when this process was started as an interrupter (`<exe> interrupt --pid <pid>`).
#[cfg_attr(not(windows), allow(dead_code))]
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
    #[ignore = "run as a child process by a_console_program_stops_on_ctrl_c"]
    fn ctrl_c_helper() {
        let pid: u32 = std::env::var("CYBERCTF_CTRL_C_PID").unwrap().parse().unwrap();
        assert!(send_ctrl_c(pid));
    }

    /// A console program started the way the worker starts its tools (own hidden console) stops
    /// on the Ctrl+C, without being killed.
    #[cfg(windows)]
    #[test]
    fn a_console_program_stops_on_ctrl_c() {
        use std::os::windows::process::CommandExt;
        // `ping -t` runs until Ctrl+C, then prints its statistics and exits.
        let mut target = std::process::Command::new("ping")
            .args(["-t", "127.0.0.1"])
            .creation_flags(crate::platform::exec::CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(800));
        let helper = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "platform::console::tests::ctrl_c_helper", "--ignored", "--nocapture"])
            .env("CYBERCTF_CTRL_C_PID", target.id().to_string())
            .creation_flags(crate::platform::exec::CREATE_NO_WINDOW)
            .output()
            .unwrap();
        assert!(helper.status.success(), "helper: {}", String::from_utf8_lossy(&helper.stdout));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let stopped = loop {
            if let Some(status) = target.try_wait().unwrap() {
                break Some(status);
            }
            if std::time::Instant::now() > deadline {
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        if stopped.is_none() {
            let _ = target.kill();
        }
        assert!(stopped.is_some(), "ping still running 10 s after Ctrl+C");
        let mut out = String::new();
        std::io::Read::read_to_string(target.stdout.as_mut().unwrap(), &mut out).unwrap();
        // Its statistics: it stopped on its own, as on Ctrl+C, not killed.
        assert!(out.contains("Packets: Sent"), "{out}");
    }
}
