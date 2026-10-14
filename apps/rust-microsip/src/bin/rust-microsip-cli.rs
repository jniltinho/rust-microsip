//! `rust-microsip-cli`: the same program as `rust-microsip` in the console
//! subsystem, so on Windows the CLI, `mcp --stdio` and `service` keep their
//! standard streams (the window binary is built without a console).
// The included file's `windows_subsystem` is a crate-level attribute meant
// for the window binary only.
#![cfg_attr(
    all(windows, not(test)),
    expect(unused_attributes, reason = "subsystem applies to rust-microsip.exe")
)]

#[path = "../main.rs"]
mod app;

fn main() -> std::process::ExitCode {
    app::main()
}
