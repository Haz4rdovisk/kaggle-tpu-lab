//! Small, safe process helpers.
//!
//! There is intentionally NO generic "run arbitrary command" bridge: the
//! frontend only reaches named commands (start_tpu, stop_tpu, ...) whose
//! argument vectors are assembled here, inside Rust, from validated settings.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("failed to start process: {0}")]
    Io(#[from] std::io::Error),
    #[error("process timed out after {0:?}")]
    Timeout(Duration),
}

const CREATE_NO_WINDOW: u32 = 0x08000000;

fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// Start a detached, window-less process (the launcher's own `serve` flow).
/// stdout/stderr go nowhere: progress is observed via ntfy + Kaggle status,
/// exactly like a Ctrl-C detach in the terminal flow.
pub fn spawn_detached(program: &Path, args: &[String], cwd: &Path) -> std::io::Result<Child> {
    let mut cmd = Command::new(program);
    cmd.current_dir(cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut cmd);
    cmd.spawn()
}

/// Run a process to completion (or timeout), capturing combined output.
/// `input` is piped to stdin (launch.py stop confirms the kernel delete).
pub fn run_capture(
    program: &Path,
    args: &[String],
    cwd: &Path,
    input: Option<&str>,
    timeout: Duration,
) -> Result<(i32, String), ProcessError> {
    let mut child = {
        let mut cmd = Command::new(program);
        cmd.current_dir(cwd)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        no_window(&mut cmd);
        cmd.spawn()?
    };

    if let Some(txt) = input {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(txt.as_bytes());
        }
    }

    // Drain stdout/stderr on helper threads so a chatty child can never
    // deadlock us (pipe buffer full -> child blocks -> we time out wrongly).
    let mut handles = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        handles.push(std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        }));
    }
    if let Some(mut pipe) = child.stderr.take() {
        handles.push(std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        }));
    }

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => {
                let code = status.code().unwrap_or(-1);
                let mut output = String::new();
                for h in handles {
                    if let Ok(b) = h.join() {
                        output.push_str(&String::from_utf8_lossy(&b));
                    }
                }
                return Ok((code, output));
            }
            None => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ProcessError::Timeout(timeout));
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_capture_captures_output() {
        #[cfg(windows)]
        let (prog, args, input): (&str, Vec<String>, Option<&str>) = (
            "cmd.exe",
            vec!["/c".into(), "echo hello-ktl".into()],
            None,
        );
        #[cfg(not(windows))]
        let (prog, args, input): (&str, Vec<String>, Option<&str>) = (
            "sh",
            vec!["-c".into(), "echo hello-ktl".into()],
            None,
        );
        let (code, out) = run_capture(
            Path::new(prog),
            &args,
            Path::new("."),
            input,
            Duration::from_secs(10),
        )
        .expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("hello-ktl"));
    }
}
