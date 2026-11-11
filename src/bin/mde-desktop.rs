#![cfg_attr(windows, windows_subsystem = "windows")]

/// GUI launcher for Windows. Keep mde.exe as a console application for CLI use.
#[cfg(windows)]
fn main() -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;

    // The sibling executable owns all application code and argument parsing.
    // CREATE_NO_WINDOW hides its console while still allowing the GUI window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let executable = std::env::current_exe()?.with_file_name("mde.exe");
    std::process::Command::new(executable)
        .arg("--foreground")
        .args(std::env::args_os().skip(1))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("mde-desktop is only needed on Windows; run mde instead");
}
