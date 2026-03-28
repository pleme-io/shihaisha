use async_trait::async_trait;
use shihaisha_core::traits::health_checker::HealthChecker;
use shihaisha_core::types::health_check::HealthCheckSpec;
use shihaisha_core::Result;
use std::path::Path;
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
    async fn check(&self, spec: &HealthCheckSpec) -> Result<bool> {
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
                match tokio::time::timeout(timeout, check_http(endpoint)).await {
                    Ok(Ok(healthy)) => Ok(healthy),
                    Ok(Err(_)) | Err(_) => Ok(false),
                }
            }
            HealthCheckSpec::Tcp { address, .. } => {
                debug!("TCP health check: {address}");
                let timeout = std::time::Duration::from_secs(5);
                match tokio::time::timeout(timeout, TcpStream::connect(address)).await {
                    Ok(Ok(_)) => Ok(true),
                    _ => Ok(false),
                }
            }
            HealthCheckSpec::Command {
                command, args, ..
            } => {
                debug!("command health check: {command}");
                let output = Command::new(command).args(args).output().await;
                match output {
                    Ok(out) => Ok(out.status.success()),
                    Err(_) => Ok(false),
                }
            }
            HealthCheckSpec::File { path, .. } => {
                debug!("file health check: {}", path.display());
                Ok(Path::new(path).exists())
            }
        }
    }

    fn name(&self) -> &str {
        "default"
    }
}

async fn check_http(endpoint: &str) -> std::io::Result<bool> {
    // Parse the URL to extract host:port
    let url = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let (host_port, _path) = url.split_once('/').unwrap_or((url, ""));

    let stream = TcpStream::connect(host_port).await?;
    drop(stream);
    Ok(true)
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
        assert!(result, "/tmp should exist");
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
        assert!(!result);
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
        assert!(!result);
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
        assert!(result);
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
        assert!(!result);
    }
}
