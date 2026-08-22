use std::io::Write;
use std::process::{Command, Stdio};

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < 5 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

#[allow(dead_code)]
pub fn human_bytes_short(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < 5 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}", bytes)
    } else {
        format!("{:.0}{}", value, UNITS[unit])
    }
}

// Launch a command without waiting (fire and forget), detached from the
// terminal. Returns the pid, or -1 on failure.
pub fn fork_exec(command: &str) -> i32 {
    use std::os::unix::process::CommandExt;
    match Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    {
        Ok(child) => child.id() as i32,
        Err(_) => -1,
    }
}

// Run a command, feeding `input` lines to its stdin, capturing stdout+stderr.
// Returns Some((stdout, stderr)) on success (exit code 0), None otherwise.
pub fn run_capture(command: &str, input: &[String]) -> Option<(String, String)> {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    {
        let mut stdin = child.stdin.take()?;
        for line in input {
            if stdin.write_all(line.as_bytes()).is_err() {
                break;
            }
            if stdin.write_all(b"\n").is_err() {
                break;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some((
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

// Filesystem usage for a mount point via statvfs. Also reports read-only state.
pub fn statvfs_usage(mountpoint: &str) -> Option<(u64, u64, u64, bool)> {
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let c_path = std::ffi::CString::new(mountpoint).ok()?;
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let frsize = if st.f_frsize > 0 {
        st.f_frsize as u64
    } else {
        512
    };
    let total = (st.f_blocks as u64).wrapping_mul(frsize);
    let free = (st.f_bavail as u64).wrapping_mul(frsize);
    let used = total.saturating_sub(free);
    let read_only = (st.f_flag as u64 & libc::ST_RDONLY) != 0;
    Some((total, used, free, read_only))
}

// Desktop notification via notify-send (best effort).
pub fn notify(summary: &str, body: &str, critical: bool) {
    let mut cmd = String::from("notify-send --app-name=argvus-storage ");
    cmd += if critical {
        "-u critical "
    } else {
        "-u normal "
    };
    cmd += &shell_quote(summary);
    if !body.is_empty() {
        cmd.push(' ');
        cmd += &shell_quote(body);
    }
    fork_exec(&cmd);
}

pub fn trim(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
}

pub fn shell_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_values() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(512 * 1024 * 1024), "536.9 MB");
        assert_eq!(human_bytes_short(12 * 1024 * 1024 * 1024), "13G");
    }

    #[test]
    fn trim_whitespace() {
        assert_eq!(trim("  foo \n"), "foo");
        assert_eq!(trim("\t\r\n"), "");
    }

    #[test]
    fn shell_quotes() {
        assert_eq!(shell_quote("abc"), "'abc'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }
}
