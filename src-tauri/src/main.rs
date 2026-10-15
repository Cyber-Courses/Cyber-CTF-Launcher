//! The Cyber CTF launcher's executable: everything is in the library (`cyberctf_lib::run`).

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    match cyberctf_lib::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Cyber CTF couldn't start: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
