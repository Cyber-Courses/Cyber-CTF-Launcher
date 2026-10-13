// Typed wrappers around the Rust commands in src-tauri, one module per area. The argument and
// result types come from the Rust structs (src-tauri/src/ts_bindings.rs writes them into
// ./generated on `cargo test`); only the command names and argument lists are written here.
export * from "@/lib/tauri/account";
export * from "@/lib/tauri/app";
export * from "@/lib/tauri/cloud";
export * from "@/lib/tauri/labs";
export * from "@/lib/tauri/machine";
export * from "@/lib/tauri/servers";
