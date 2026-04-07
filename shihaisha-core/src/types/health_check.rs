use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Result of a single health check execution, carrying diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthCheckResult {
    /// Whether the service is considered healthy.
    pub healthy: bool,
    /// How long the check took to complete.
    pub latency: Duration,
    /// Optional diagnostic message (typically set on failure).
    pub message: Option<String>,
}

impl HealthCheckResult {
    /// Create a healthy result with the given latency.
    #[must_use]
    pub fn healthy(latency: Duration) -> Self {
        Self {
            healthy: true,
            latency,
            message: None,
        }
    }

    /// Create an unhealthy result with the given latency and diagnostic message.
    #[must_use]
    pub fn unhealthy(latency: Duration, message: impl Into<String>) -> Self {
        Self {
            healthy: false,
            latency,
            message: Some(message.into()),
        }
    }
}

/// Health check specification — determines how to verify a service is healthy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[non_exhaustive]
pub enum HealthCheckSpec {
    /// HTTP endpoint health check.
    Http {
        /// URL to probe (e.g., `http://localhost:8080/health`).
        endpoint: String,
        /// Interval between checks in seconds.
        #[serde(default = "default_interval")]
        interval_secs: u64,
        /// Timeout for each check in seconds.
        #[serde(default = "default_timeout")]
        timeout_secs: u64,
        /// Number of consecutive failures before marking unhealthy.
        #[serde(default = "default_max_failures")]
        max_failures: u32,
    },
    /// TCP connect health check.
    Tcp {
        /// Address to connect to (e.g., `127.0.0.1:5432`).
        address: String,
        /// Interval between checks in seconds.
        #[serde(default = "default_interval")]
        interval_secs: u64,
        /// Number of consecutive failures before marking unhealthy.
        #[serde(default = "default_max_failures")]
        max_failures: u32,
    },
    /// Command execution health check.
    Command {
        /// Command to execute.
        command: String,
        /// Command arguments.
        #[serde(default)]
        args: Vec<String>,
        /// Interval between checks in seconds.
        #[serde(default = "default_interval")]
        interval_secs: u64,
        /// Number of consecutive failures before marking unhealthy.
        #[serde(default = "default_max_failures")]
        max_failures: u32,
    },
    /// File existence health check.
    File {
        /// Path to check for existence.
        path: PathBuf,
        /// Interval between checks in seconds.
        #[serde(default = "default_interval")]
        interval_secs: u64,
        /// Number of consecutive failures before marking unhealthy.
        #[serde(default = "default_max_failures")]
        max_failures: u32,
    },
}

fn default_interval() -> u64 {
    30
}

fn default_timeout() -> u64 {
    5
}

fn default_max_failures() -> u32 {
    3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_health_check_serializes_with_tag() {
        let check = HealthCheckSpec::Http {
            endpoint: "http://localhost:8080/health".to_owned(),
            interval_secs: 10,
            timeout_secs: 5,
            max_failures: 3,
        };
        let json = serde_json::to_value(&check).expect("serialize");
        assert_eq!(json["type"], "http");
        assert_eq!(json["endpoint"], "http://localhost:8080/health");
    }

    #[test]
    fn tcp_health_check_serializes_with_tag() {
        let check = HealthCheckSpec::Tcp {
            address: "127.0.0.1:5432".to_owned(),
            interval_secs: 15,
            max_failures: 5,
        };
        let json = serde_json::to_value(&check).expect("serialize");
        assert_eq!(json["type"], "tcp");
        assert_eq!(json["address"], "127.0.0.1:5432");
    }

    #[test]
    fn command_health_check_serializes_with_tag() {
        let check = HealthCheckSpec::Command {
            command: "/usr/bin/pg_isready".to_owned(),
            args: vec!["-h".to_owned(), "localhost".to_owned()],
            interval_secs: 30,
            max_failures: 3,
        };
        let json = serde_json::to_value(&check).expect("serialize");
        assert_eq!(json["type"], "command");
        assert_eq!(json["command"], "/usr/bin/pg_isready");
    }

    #[test]
    fn file_health_check_serializes_with_tag() {
        let check = HealthCheckSpec::File {
            path: PathBuf::from("/tmp/healthy"),
            interval_secs: 30,
            max_failures: 3,
        };
        let json = serde_json::to_value(&check).expect("serialize");
        assert_eq!(json["type"], "file");
        assert_eq!(json["path"], "/tmp/healthy");
    }

    #[test]
    fn http_health_check_defaults_from_yaml() {
        let yaml = r#"
type: http
endpoint: http://localhost:3000/ready
"#;
        let check: HealthCheckSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        match check {
            HealthCheckSpec::Http {
                endpoint,
                interval_secs,
                timeout_secs,
                max_failures,
            } => {
                assert_eq!(endpoint, "http://localhost:3000/ready");
                assert_eq!(interval_secs, 30);
                assert_eq!(timeout_secs, 5);
                assert_eq!(max_failures, 3);
            }
            _ => panic!("expected Http variant"),
        }
    }

    #[test]
    fn roundtrip_all_variants() {
        let checks = vec![
            HealthCheckSpec::Http {
                endpoint: "http://localhost/health".to_owned(),
                interval_secs: 10,
                timeout_secs: 2,
                max_failures: 5,
            },
            HealthCheckSpec::Tcp {
                address: "127.0.0.1:6379".to_owned(),
                interval_secs: 15,
                max_failures: 3,
            },
            HealthCheckSpec::Command {
                command: "check.sh".to_owned(),
                args: vec![],
                interval_secs: 60,
                max_failures: 1,
            },
            HealthCheckSpec::File {
                path: PathBuf::from("/var/run/ready"),
                interval_secs: 5,
                max_failures: 2,
            },
        ];

        for check in checks {
            let yaml = serde_yaml_ng::to_string(&check).expect("serialize");
            let parsed: HealthCheckSpec = serde_yaml_ng::from_str(&yaml).expect("deserialize");
            let reparsed_yaml = serde_yaml_ng::to_string(&parsed).expect("re-serialize");
            assert_eq!(yaml, reparsed_yaml);
        }
    }

    #[test]
    fn health_check_result_construction() {
        let result = HealthCheckResult {
            healthy: true,
            latency: Duration::from_millis(42),
            message: None,
        };
        assert!(result.healthy);
        assert_eq!(result.latency, Duration::from_millis(42));
        assert!(result.message.is_none());
    }

    #[test]
    fn health_check_result_unhealthy_with_message() {
        let result = HealthCheckResult {
            healthy: false,
            latency: Duration::from_secs(5),
            message: Some("connection refused".to_owned()),
        };
        assert!(!result.healthy);
        assert_eq!(result.latency, Duration::from_secs(5));
        assert_eq!(result.message.as_deref(), Some("connection refused"));
    }

    #[test]
    fn healthy_constructor() {
        let result = HealthCheckResult::healthy(Duration::from_millis(10));
        assert!(result.healthy);
        assert_eq!(result.latency, Duration::from_millis(10));
        assert!(result.message.is_none());
    }

    #[test]
    fn unhealthy_constructor() {
        let result = HealthCheckResult::unhealthy(Duration::from_secs(1), "tcp timeout");
        assert!(!result.healthy);
        assert_eq!(result.latency, Duration::from_secs(1));
        assert_eq!(result.message.as_deref(), Some("tcp timeout"));
    }

    #[test]
    fn health_check_result_clone_and_eq() {
        let result = HealthCheckResult {
            healthy: true,
            latency: Duration::from_millis(10),
            message: Some("ok".to_owned()),
        };
        let cloned = result.clone();
        assert_eq!(result, cloned);
    }

    #[test]
    fn tcp_health_check_defaults_from_yaml() {
        let yaml = r#"
type: tcp
address: 127.0.0.1:5432
"#;
        let check: HealthCheckSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        match check {
            HealthCheckSpec::Tcp {
                address,
                interval_secs,
                max_failures,
            } => {
                assert_eq!(address, "127.0.0.1:5432");
                assert_eq!(interval_secs, 30);
                assert_eq!(max_failures, 3);
            }
            _ => panic!("expected Tcp variant"),
        }
    }

    #[test]
    fn command_health_check_defaults_from_yaml() {
        let yaml = r#"
type: command
command: /usr/bin/check
"#;
        let check: HealthCheckSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        match check {
            HealthCheckSpec::Command {
                command,
                args,
                interval_secs,
                max_failures,
            } => {
                assert_eq!(command, "/usr/bin/check");
                assert!(args.is_empty());
                assert_eq!(interval_secs, 30);
                assert_eq!(max_failures, 3);
            }
            _ => panic!("expected Command variant"),
        }
    }
}
