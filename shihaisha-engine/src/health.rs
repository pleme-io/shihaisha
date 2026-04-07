use async_trait::async_trait;
use shihaisha_core::traits::health_checker::HealthChecker;
use shihaisha_core::types::health_check::{HealthCheckResult, HealthCheckSpec};
use shihaisha_core::Result;
use std::path::Path;
use std::time::Instant;
use tokio::net::TcpStream;
use tokio::process::Command;
use tracing::debug;

/// Default health checker that handles all check types.
pub struct DefaultHealthChecker;

impl DefaultHealthChecker {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultHealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HealthChecker for DefaultHealthChecker {
    async fn check(&self, spec: &HealthCheckSpec) -> Result<HealthCheckResult> {
        let start = Instant::now();

        match spec {
            HealthCheckSpec::Http {
                endpoint,
                timeout_secs,
                ..
            } => {
                debug!("HTTP health check: {endpoint}");
                let timeout = std::time::Duration::from_secs(*timeout_secs);
                // Simple HTTP check using a TCP connect to the endpoint
                // (avoids pulling in reqwest as a dependency)
                let (healthy, message) =
                    match tokio::time::timeout(timeout, check_http(endpoint)).await {
                        Ok(Ok(true)) => (true, None),
                        Ok(Ok(false)) => (false, Some("HTTP check returned non-2xx/3xx".to_owned())),
                        Ok(Err(e)) => (false, Some(format!("HTTP check error: {e}"))),
                        Err(_) => (false, Some("HTTP check timed out".to_owned())),
                    };
                Ok(HealthCheckResult {
                    healthy,
                    latency: start.elapsed(),
                    message,
                })
            }
            HealthCheckSpec::Tcp { address, .. } => {
                debug!("TCP health check: {address}");
                let timeout = std::time::Duration::from_secs(5);
                let (healthy, message) =
                    match tokio::time::timeout(timeout, TcpStream::connect(address)).await {
                        Ok(Ok(_)) => (true, None),
                        Ok(Err(e)) => (false, Some(format!("TCP connect failed: {e}"))),
                        Err(_) => (false, Some("TCP connect timed out".to_owned())),
                    };
                Ok(HealthCheckResult {
                    healthy,
                    latency: start.elapsed(),
                    message,
                })
            }
            HealthCheckSpec::Command {
                command, args, ..
            } => {
                debug!("command health check: {command}");
                let output = Command::new(command).args(args).output().await;
                let (healthy, message) = match output {
                    Ok(out) if out.status.success() => (true, None),
                    Ok(out) => (
                        false,
                        Some(format!(
                            "command exited with {}",
                            out.status.code().map_or("signal".to_owned(), |c| c.to_string())
                        )),
                    ),
                    Err(e) => (false, Some(format!("command failed: {e}"))),
                };
                Ok(HealthCheckResult {
                    healthy,
                    latency: start.elapsed(),
                    message,
                })
            }
            HealthCheckSpec::File { path, .. } => {
                debug!("file health check: {}", path.display());
                let exists = Path::new(path).exists();
                let message = if exists {
                    None
                } else {
                    Some(format!("file not found: {}", path.display()))
                };
                Ok(HealthCheckResult {
                    healthy: exists,
                    latency: start.elapsed(),
                    message,
                })
            }
        }
    }

    fn name(&self) -> &str {
        "default"
    }
}

async fn check_http(endpoint: &str) -> std::io::Result<bool> {
    // Parse the URL to extract host:port and path
    let url = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let (host_port, path) = url.split_once('/').unwrap_or((url, ""));
    let path = format!("/{path}");

    let mut stream = TcpStream::connect(host_port).await?;

    // Send a minimal HTTP/1.1 HEAD request
    let host = host_port.split(':').next().unwrap_or(host_port);
    let request = format!("HEAD {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream.write_all(request.as_bytes()).await?;

    // Read the response status line
    let mut buf = vec![0u8; 1024];
    let n = stream.read(&mut buf).await?;
    let response = String::from_utf8_lossy(&buf[..n]);

    // Check for HTTP/1.x 2xx or 3xx status
    if let Some(status_line) = response.lines().next() {
        if let Some(code_str) = status_line.split_whitespace().nth(1) {
            if let Ok(code) = code_str.parse::<u16>() {
                return Ok((200..400).contains(&code));
            }
        }
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn default_health_checker_name() {
        let checker = DefaultHealthChecker::new();
        assert_eq!(checker.name(), "default");
    }

    #[tokio::test]
    async fn file_health_check_existing_file() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::File {
            path: PathBuf::from("/tmp"),
            interval_secs: 30,
            max_failures: 3,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(result.healthy, "/tmp should exist");
        assert!(result.latency.as_nanos() > 0, "latency should be non-zero");
        assert!(result.message.is_none(), "healthy check should have no message");
    }

    #[tokio::test]
    async fn file_health_check_missing_file() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::File {
            path: PathBuf::from("/tmp/shihaisha-nonexistent-health-check-file"),
            interval_secs: 30,
            max_failures: 3,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(!result.healthy);
        assert!(result.message.is_some(), "unhealthy check should have a message");
        assert!(result.message.as_ref().unwrap().contains("file not found"));
    }

    #[tokio::test]
    async fn tcp_health_check_unreachable() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::Tcp {
            address: "127.0.0.1:1".to_owned(), // unlikely to be listening
            interval_secs: 5,
            max_failures: 1,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(!result.healthy);
    }

    #[tokio::test]
    async fn command_health_check_true() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::Command {
            command: "true".to_owned(),
            args: vec![],
            interval_secs: 5,
            max_failures: 1,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(result.healthy);
        assert!(result.latency.as_nanos() > 0);
    }

    #[tokio::test]
    async fn command_health_check_false() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::Command {
            command: "false".to_owned(),
            args: vec![],
            interval_secs: 5,
            max_failures: 1,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(!result.healthy);
        assert!(result.message.is_some());
    }

    #[tokio::test]
    async fn healthy_result_has_latency() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::Command {
            command: "true".to_owned(),
            args: vec![],
            interval_secs: 5,
            max_failures: 1,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(result.healthy);
        assert!(result.latency.as_nanos() > 0, "latency should be measured");
    }

    #[tokio::test]
    async fn unhealthy_result_has_message() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::File {
            path: PathBuf::from("/tmp/shihaisha-does-not-exist-qwerty"),
            interval_secs: 5,
            max_failures: 1,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(!result.healthy);
        assert!(
            result.message.is_some(),
            "unhealthy result must carry a diagnostic message"
        );
    }

    #[tokio::test]
    async fn file_check_result_latency() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::File {
            path: PathBuf::from("/tmp"),
            interval_secs: 5,
            max_failures: 1,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(result.healthy);
        assert!(result.latency.as_secs() < 5, "file check should be fast");
    }

    #[tokio::test]
    async fn command_health_check_spawn_failure() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::Command {
            command: "/nonexistent/binary/path".to_owned(),
            args: vec![],
            interval_secs: 5,
            max_failures: 1,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(!result.healthy);
        assert!(result.message.as_ref().unwrap().contains("command failed"));
    }

    #[tokio::test]
    async fn command_health_check_with_args() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::Command {
            command: "test".to_owned(),
            args: vec!["-d".to_owned(), "/tmp".to_owned()],
            interval_secs: 5,
            max_failures: 1,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(result.healthy, "/tmp should be a directory");
    }

    #[tokio::test]
    async fn command_health_check_nonzero_exit_has_code() {
        let checker = DefaultHealthChecker::new();
        let spec = HealthCheckSpec::Command {
            command: "sh".to_owned(),
            args: vec!["-c".to_owned(), "exit 42".to_owned()],
            interval_secs: 5,
            max_failures: 1,
        };
        let result = checker.check(&spec).await.expect("check");
        assert!(!result.healthy);
        let msg = result.message.unwrap();
        assert!(
            msg.contains("42"),
            "should include exit code: {msg}"
        );
    }

    #[test]
    fn default_health_checker_default_trait() {
        let checker = DefaultHealthChecker::default();
        assert_eq!(checker.name(), "default");
    }
}
