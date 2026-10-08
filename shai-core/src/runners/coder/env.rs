use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Get the current working directory
pub fn get_working_dir() -> String {
    env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "Unknown".to_string())
}

/// Check if the current directory is inside a git repository
pub fn is_git_repo() -> bool {
    find_git_root().is_some()
}

/// Get the platform (OS family)
pub fn get_platform() -> String {
    if cfg!(target_os = "windows") {
        "Windows".to_string()
    } else if cfg!(target_os = "macos") {
        "macOS".to_string()
    } else if cfg!(target_os = "linux") {
        "Linux".to_string()
    } else if cfg!(target_family = "unix") {
        "Unix".to_string()
    } else {
        "Unknown".to_string()
    }
}

/// Get OS version (simple approach)
pub fn get_os_version() -> String {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(&["/C", "ver"])
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "Unknown Windows version".to_string())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|s| format!("macOS {}", s.trim()))
            .unwrap_or_else(|| "Unknown macOS version".to_string())
    }

    #[cfg(target_os = "linux")]
    {
        use std::fs;
        // Try /etc/os-release first
        if let Ok(content) = fs::read_to_string("/etc/os-release") {
            for line in content.lines() {
                if line.starts_with("PRETTY_NAME=") {
                    return line
                        .split('=')
                        .nth(1)
                        .unwrap_or("Unknown Linux")
                        .trim_matches('"')
                        .to_string();
                }
            }
        }

        // Fallback to uname
        Command::new("uname")
            .args(["-sr"])
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "Unknown Linux".to_string())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "Unknown OS".to_string()
    }
}

/// Get today's date in YYYY-MM-DD format
pub fn get_today() -> String {
    use chrono::Local;

    Local::now().date_naive().format("%Y-%m-%d").to_string()
}

/// Get the current git branch
pub fn get_git_branch() -> String {
    Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Get git status
pub fn get_git_status() -> String {
    Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| {
            if s.trim().is_empty() {
                "Clean working directory".to_string()
            } else {
                s.trim().to_string()
            }
        })
        .unwrap_or_else(|| "Not a git repository or git not available".to_string())
}

/// Get recent git log (last 5 commits)
pub fn get_git_log() -> String {
    Command::new("git")
        .args(["log", "--oneline", "-5"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "No recent commits or not a git repository".to_string())
}

/// Walk up from the current directory to find the nearest `.git` directory.
/// Returns the repository root path if found, otherwise `None`.
pub fn find_git_root() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    let mut current: &Path = &cwd;
    loop {
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return None,
        }
    }
}

/// Get all environment variable keys (not values for security)
pub fn env_all_key() -> String {
    let mut keys: Vec<String> = env::vars().map(|(key, _)| key).collect();
    keys.sort();
    keys.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_working_dir() {
        let dir = get_working_dir();
        println!("{:?}", dir);
        assert!(!dir.is_empty());
        assert_ne!(dir, "Unknown");
    }

    #[test]
    fn test_get_platform() {
        let platform = get_platform();
        println!("{:?}", platform);
        assert!(["Windows", "macOS", "Linux", "Unix", "Unknown"].contains(&platform.as_str()));
    }

    #[test]
    fn test_get_today() {
        let today = get_today();
        println!("{:?}", today);
        assert!(today.len() == 10); // YYYY-MM-DD format
        assert!(today.contains('-'));
    }

    #[test]
    fn test_is_git_repo() {
        let result = is_git_repo();
        println!("{:?}", result);
        // This will be true if running in a git repo, false otherwise
        // We just test that it returns a boolean without panicking
        // result is a boolean - just verify it doesn't panic
    }

    #[test]
    fn test_get_git_branch() {
        let branch = get_git_branch();
        assert!(!branch.is_empty());
        println!("{:?}", branch);
        // If we're in a git repo, branch should not be "Unknown"
        // If not in git repo, it should be "Unknown"
        if is_git_repo() {
            // In a git repo, branch name should be reasonable
            assert!(!branch.is_empty());
            assert!(!branch.contains('\n')); // Should be single line
        } else {
            assert_eq!(branch, "Unknown");
        }
    }

    #[test]
    fn test_get_git_status() {
        let status = get_git_status();
        assert!(!status.is_empty());
        println!("{:?}", status);
        if is_git_repo() {
            // Should either be clean or show file changes
            assert!(status == "Clean working directory" || status.lines().count() > 0);
        } else {
            assert_eq!(status, "Not a git repository or git not available");
        }
    }

    #[test]
    fn test_get_git_log() {
        let log = get_git_log();
        println!("{:?}", log);
        assert!(!log.is_empty());
        if is_git_repo() {
            // Should have at least one commit or be a message about no commits
            if log != "No recent commits or not a git repository" {
                // Each line should have a commit hash (short) and message
                for line in log.lines() {
                    assert!(line.len() > 7); // At least hash + space + some message
                                             // First part should be hex characters (commit hash)
                    let parts: Vec<&str> = line.splitn(2, ' ').collect();
                    if !parts.is_empty() {
                        let hash = parts[0];
                        assert!(hash.len() >= 7); // Short hash is usually 7+ chars
                        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
                    }
                }
            }
        } else {
            assert_eq!(log, "No recent commits or not a git repository");
        }
    }
}
