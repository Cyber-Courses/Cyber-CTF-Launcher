//! Windows has no SIGINT: a console program's clean stop is a Ctrl+C on its console. Terraform
//! stops on one the way it does on SIGINT (it finishes what it is creating and saves its state).
//!
//! Sending it means attaching to the target's console, which changes the caller's own console
//! state for the whole process. So the app never does it itself: it runs its own executable as
//! `cyberctf-desktop interrupt --pid <pid>`, a throwaway process that attaches, sends, and exits.

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
}
