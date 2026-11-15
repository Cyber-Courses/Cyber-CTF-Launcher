//! The launcher's command line, `cyberctf`: everything is in the library (`cyberctf_lib::cli_main`).
//! A console program of its own, beside the app's executable: a Windows GUI-subsystem program
//! can't print to the console it was started from.

fn main() -> std::process::ExitCode {
    cyberctf_lib::cli_main()
}
