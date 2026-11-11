use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

#[derive(Default, Debug)]
pub struct Options {
    pub paths: Vec<PathBuf>,
    pub foreground: bool,
    pub help: bool,
    pub version: bool,
}

pub fn parse(args: &[OsString]) -> Options {
    let mut options = Options::default();
    let mut paths_only = false;
    for arg in args {
        if !paths_only && arg == "--" {
            paths_only = true;
        } else if !paths_only && (arg == "--help" || arg == "-h") {
            options.help = true;
        } else if !paths_only && (arg == "--version" || arg == "-V") {
            options.version = true;
        } else if !paths_only && arg == "--foreground" {
            options.foreground = true;
        } else {
            options.paths.push(PathBuf::from(arg));
        }
    }
    options
}

pub fn launch(args: &[OsString]) -> io::Result<Child> {
    detached_command(&std::env::current_exe()?, args).spawn()
}

fn detached_command(executable: &Path, args: &[OsString]) -> Command {
    let mut command = Command::new(executable);
    // The child runs the GUI directly rather than launching another child.
    // Preserve cwd and raw OS arguments, including spaces and non-UTF-8 paths.
    command.arg("--foreground").args(args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Separate from the shell's foreground process group and terminal signals.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn default_launch_detaches_and_preserves_file_order() {
        let options = parse(&args(&["first.md", "папка с пробелами/второй.md", "."]));
        assert!(!options.foreground);
        assert_eq!(
            options.paths,
            [
                PathBuf::from("first.md"),
                PathBuf::from("папка с пробелами/второй.md"),
                PathBuf::from(".")
            ]
        );
    }

    #[test]
    fn explicit_foreground_does_not_relaunch_and_separator_preserves_flag_names() {
        let options = parse(&args(&[
            "--foreground",
            "--",
            "--foreground",
            "--help",
            "--version",
        ]));
        assert!(options.foreground);
        assert!(!options.help);
        assert!(!options.version);
        assert_eq!(
            options.paths,
            [
                PathBuf::from("--foreground"),
                PathBuf::from("--help"),
                PathBuf::from("--version")
            ]
        );
        assert!(parse(&args(&["--help"])).help);
        assert!(parse(&args(&["--version"])).version);
    }

    #[cfg(unix)]
    #[test]
    fn detached_child_keeps_running_and_receives_exact_arguments() {
        use std::{
            fs,
            os::unix::fs::PermissionsExt,
            time::{Duration, Instant},
        };
        struct Probe {
            dir: PathBuf,
            child: Option<Child>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                if let Some(child) = &mut self.child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                let _ = fs::remove_dir_all(&self.dir);
            }
        }
        let mut probe = Probe {
            dir: std::env::temp_dir().join(format!("mde-launch-test-{}", std::process::id())),
            child: None,
        };
        fs::create_dir(&probe.dir).unwrap();
        let script = probe.dir.join("probe.sh");
        let output = probe.dir.join("args");
        fs::write(
            &script,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$MDE_LAUNCH_TEST_OUTPUT\"\nexec /bin/sleep 30\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        probe.child = Some(
            detached_command(&script, &args(&["путь с пробелами.md", "--", "--help"]))
                .env("MDE_LAUNCH_TEST_OUTPUT", &output)
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let expected = "--foreground\nпуть с пробелами.md\n--\n--help\n";
        while fs::read_to_string(&output).ok().as_deref() != Some(expected)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fs::read_to_string(output).unwrap(), expected);
        assert!(probe.child.as_mut().unwrap().try_wait().unwrap().is_none());
    }
}
