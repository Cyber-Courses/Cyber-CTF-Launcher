//! The Cyber CTF launcher's executable: everything is in the library (`cyberctf_lib::run`).

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    cyberctf_lib::run();
}
