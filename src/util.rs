// SPDX-License-Identifier: GPL-3.0-or-later
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Output beyond this is discarded (protects against runaway panel commands).
const MAX_OUTPUT: u64 = 4 * 1024 * 1024;

pub struct Output {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// Short human-readable reason for a failed command.
    pub fn failure(&self) -> String {
        let msg = self.stderr.trim();
        let msg = if msg.is_empty() { self.stdout.trim() } else { msg };
        let first = msg.lines().next().unwrap_or("");
        match self.code {
            Some(c) if first.is_empty() => format!("exited with status {c}"),
            Some(c) => format!("exited with status {c}: {first}"),
            None => "killed by signal".into(),
        }
    }
}

/// Run a command with a timeout. The child gets its own process group so a
/// timeout kills everything it spawned (e.g. a whole `sh -c` pipeline).
pub fn run(mut cmd: Command, timeout: Duration) -> Result<Output, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);

    let mut child = cmd.spawn().map_err(|e| format!("cannot run {:?}: {e}", cmd.get_program()))?;
    let mut out = child.stdout.take().expect("piped stdout");
    let mut err = child.stderr.take().expect("piped stderr");
    // Read past the cap into a sink so the child never blocks on a full pipe.
    let drain = |r: &mut dyn Read| {
        let mut b = Vec::new();
        let _ = r.take(MAX_OUTPUT).read_to_end(&mut b);
        let _ = std::io::copy(r, &mut std::io::sink());
        b
    };
    let out_t = thread::spawn(move || drain(&mut out));
    let err_t = thread::spawn(move || drain(&mut err));
    let kill = |child: &mut std::process::Child| {
        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
        let _ = child.wait();
    };

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if start.elapsed() >= timeout => {
                kill(&mut child);
                return Err(format!("timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => thread::sleep(Duration::from_millis(15)),
            Err(e) => {
                kill(&mut child);
                return Err(e.to_string());
            }
        }
    };

    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    Ok(Output {
        success: status.success(),
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

pub fn sh(cmdline: &str, timeout: Duration) -> Result<Output, String> {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmdline);
    run(c, timeout)
}

/// First dotted version number (`1.2`, `0.4.9.12`, `2026.08.19`) in `s`.
pub fn extract_version(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let digits = |mut i: usize| {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let mut i = 0;
    while i < b.len() {
        let boundary = i == 0 || !(b[i - 1].is_ascii_digit() || b[i - 1] == b'.');
        if b[i].is_ascii_digit() && boundary {
            let mut end = digits(i);
            let mut parts = 0;
            while parts < 3 && end + 1 < b.len() && b[end] == b'.' && b[end + 1].is_ascii_digit() {
                end = digits(end + 1);
                parts += 1;
            }
            if parts > 0 {
                return Some(s[i..end].to_string());
            }
            i = end;
        } else {
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::extract_version;

    #[test]
    fn versions() {
        assert_eq!(extract_version("Tor version 0.4.9.12."), Some("0.4.9.12".into()));
        assert_eq!(extract_version("nginx version: nginx/1.26.3"), Some("1.26.3".into()));
        assert_eq!(extract_version("Bitcoin Knots daemon version v29.4.2.knots20260101"), Some("29.4.2".into()));
        assert_eq!(extract_version("2:4.22.11+dfsg-0+deb13u1"), Some("4.22.11".into()));
        assert_eq!(extract_version("257.13-1"), Some("257.13".into()));
        assert_eq!(extract_version("2026.08.19"), Some("2026.08.19".into()));
        assert_eq!(extract_version("build 42 only"), None);
    }
}
