//! `lauraw`: the program that the Laura icon runs on Windows (#165). A Windows program has no
//! console window, so `lauraw` starts `laura --serve --open` with none.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let Ok(exe) = std::env::current_exe() else {
        return ExitCode::FAILURE;
    };
    let mut cmd =
        Command::new(exe.with_file_name(format!("laura{}", std::env::consts::EXE_SUFFIX)));
    cmd.args(["--serve", "--open"]);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    match cmd.status().map(|s| s.code()) {
        Ok(Some(code)) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        _ => ExitCode::FAILURE,
    }
}
