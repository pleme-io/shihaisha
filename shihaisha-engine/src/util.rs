use std::path::PathBuf;

/// Get the user's home directory from the `HOME` environment variable,
/// falling back to `/tmp` if unset.
#[must_use]
pub fn home_dir() -> PathBuf {
    std::env::var("HOME").map_or_else(|_| PathBuf::from("/tmp"), PathBuf::from)
}

/// Check if the current process is running as root (UID 0).
#[must_use]
pub fn is_root() -> bool {
    std::process::Command::new("id")
        .args(["-u"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .is_some_and(|s| s.trim() == "0")
}

/// Get the current user's UID, defaulting to 501 (macOS first user).
#[must_use]
pub fn current_uid() -> u32 {
    std::process::Command::new("id")
        .args(["-u"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(501)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_dir_returns_something() {
        let home = home_dir();
        assert!(!home.as_os_str().is_empty());
    }

    #[test]
    fn is_root_returns_false_in_tests() {
        // Tests generally do not run as root.
        assert!(!is_root());
    }

    #[test]
    fn current_uid_returns_nonzero() {
        // In normal test environments, UID is > 0.
        assert!(current_uid() > 0);
    }
}
