use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::backend_overrides::BackendOverrides;
use super::health_check::HealthCheckSpec;
use super::logging::LoggingSpec;
use super::resource_limits::ResourceLimits;
use super::socket_spec::SocketSpec;

/// The canonical service specification.
/// Written as YAML by users, translated to backend-native formats at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSpec {
    /// Unique service name (used as systemd unit name / launchd Label).
    pub name: String,

    /// Human-readable description.
    #[serde(default)]
    pub description: String,

    /// Command to execute (program path).
    pub command: String,

    /// Command arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// Service type.
    #[serde(default)]
    pub service_type: ServiceType,

    /// Working directory.
    #[serde(default)]
    pub working_directory: Option<PathBuf>,

    /// User to run as.
    #[serde(default)]
    pub user: Option<String>,

    /// Group to run as.
    #[serde(default)]
    pub group: Option<String>,

    /// Environment variables.
    #[serde(default)]
    pub environment: HashMap<String, String>,

    /// Restart policy.
    #[serde(default)]
    pub restart: RestartPolicy,

    /// Dependency ordering.
    #[serde(default)]
    pub depends_on: DependencySpec,

    /// Health check configuration.
    #[serde(default)]
    pub health: Option<HealthCheckSpec>,

    /// Socket activation.
    #[serde(default)]
    pub sockets: Vec<SocketSpec>,

    /// Resource limits.
    #[serde(default)]
    pub resources: Option<ResourceLimits>,

    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingSpec,

    /// Notification protocol (for Type=notify services).
    #[serde(default)]
    pub notify: bool,

    /// Watchdog interval (seconds, 0 = disabled).
    #[serde(default)]
    pub watchdog_sec: u64,

    /// Timeout for starting (seconds).
    #[serde(default = "default_timeout")]
    pub timeout_start_sec: u64,

    /// Timeout for stopping (seconds).
    #[serde(default = "default_timeout")]
    pub timeout_stop_sec: u64,

    /// Backend-specific overrides (escape hatch).
    #[serde(default)]
    pub overrides: BackendOverrides,
}

fn default_timeout() -> u64 {
    90
}

/// Type of service (maps to systemd Type= / launchd `KeepAlive`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    /// Long-running process (default).
    #[default]
    Simple,
    /// Run-once task.
    Oneshot,
    /// Long-running with sd_notify readiness.
    Notify,
    /// Traditional daemon that forks.
    Forking,
    /// Timer-triggered task.
    Timer,
    /// Socket-activated service.
    Socket,
}

/// How and when to restart the service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartPolicy {
    /// Restart strategy.
    #[serde(default)]
    pub strategy: RestartStrategy,

    /// Delay between restarts in seconds.
    #[serde(default = "default_restart_delay")]
    pub delay_secs: u64,

    /// Maximum restart attempts (0 = unlimited).
    #[serde(default)]
    pub max_retries: u32,

    /// Reset retry counter after this many seconds of successful running.
    #[serde(default = "default_reset_after")]
    pub reset_after_secs: u64,
}

fn default_restart_delay() -> u64 {
    5
}

fn default_reset_after() -> u64 {
    300
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            strategy: RestartStrategy::OnFailure,
            delay_secs: default_restart_delay(),
            max_retries: 0,
            reset_after_secs: default_reset_after(),
        }
    }
}

/// When to restart the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RestartStrategy {
    /// Always restart regardless of exit code.
    Always,
    /// Restart only on non-zero exit (default).
    #[default]
    OnFailure,
    /// Restart only on clean exit (exit code 0).
    OnSuccess,
    /// Never restart.
    Never,
}

/// Dependency and ordering specification.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DependencySpec {
    /// Services that must start before this one.
    #[serde(default)]
    pub after: Vec<String>,

    /// Services that must start after this one.
    #[serde(default)]
    pub before: Vec<String>,

    /// Services that are required (pulled into the transaction).
    #[serde(default)]
    pub requires: Vec<String>,

    /// Services that are wanted (weak requirement, no failure on missing).
    #[serde(default)]
    pub wants: Vec<String>,

    /// Conflicting services (stopped when this starts).
    #[serde(default)]
    pub conflicts: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_spec() -> ServiceSpec {
        ServiceSpec {
            name: "test-service".to_owned(),
            description: String::new(),
            command: "/usr/bin/test".to_owned(),
            args: vec![],
            service_type: ServiceType::default(),
            working_directory: None,
            user: None,
            group: None,
            environment: HashMap::new(),
            restart: RestartPolicy::default(),
            depends_on: DependencySpec::default(),
            health: None,
            sockets: vec![],
            resources: None,
            logging: LoggingSpec::default(),
            notify: false,
            watchdog_sec: 0,
            timeout_start_sec: default_timeout(),
            timeout_stop_sec: default_timeout(),
            overrides: BackendOverrides::default(),
        }
    }

    #[test]
    fn yaml_roundtrip() {
        let spec = minimal_spec();
        let yaml = serde_yaml_ng::to_string(&spec).expect("serialize");
        let parsed: ServiceSpec = serde_yaml_ng::from_str(&yaml).expect("deserialize");
        assert_eq!(parsed.name, "test-service");
        assert_eq!(parsed.command, "/usr/bin/test");
    }

    #[test]
    fn defaults_applied_from_yaml() {
        let yaml = r"
name: myapp
command: /usr/bin/myapp
";
        let spec: ServiceSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert_eq!(spec.name, "myapp");
        assert_eq!(spec.service_type, ServiceType::Simple);
        assert_eq!(spec.restart.strategy, RestartStrategy::OnFailure);
        assert_eq!(spec.restart.delay_secs, 5);
        assert_eq!(spec.timeout_start_sec, 90);
        assert_eq!(spec.timeout_stop_sec, 90);
        assert!(spec.args.is_empty());
        assert!(spec.environment.is_empty());
    }

    #[test]
    fn restart_policy_default() {
        let policy = RestartPolicy::default();
        assert_eq!(policy.strategy, RestartStrategy::OnFailure);
        assert_eq!(policy.delay_secs, 5);
        assert_eq!(policy.max_retries, 0);
        assert_eq!(policy.reset_after_secs, 300);
    }

    #[test]
    fn dependency_spec_default_is_empty() {
        let dep = DependencySpec::default();
        assert!(dep.after.is_empty());
        assert!(dep.before.is_empty());
        assert!(dep.requires.is_empty());
        assert!(dep.wants.is_empty());
        assert!(dep.conflicts.is_empty());
    }

    #[test]
    fn service_type_serializes_lowercase() {
        let json = serde_json::to_string(&ServiceType::Notify).expect("serialize");
        assert_eq!(json, "\"notify\"");
        let parsed: ServiceType = serde_json::from_str("\"oneshot\"").expect("parse");
        assert_eq!(parsed, ServiceType::Oneshot);
    }

    #[test]
    fn restart_strategy_serializes_kebab() {
        let json = serde_json::to_string(&RestartStrategy::OnFailure).expect("serialize");
        assert_eq!(json, "\"on-failure\"");
        let parsed: RestartStrategy = serde_json::from_str("\"on-success\"").expect("parse");
        assert_eq!(parsed, RestartStrategy::OnSuccess);
    }

    #[test]
    fn full_spec_from_yaml() {
        let yaml = r#"
name: web-server
description: Production web server
command: /usr/bin/web-server
args:
  - --port
  - "8080"
service_type: notify
working_directory: /var/www
user: www-data
group: www-data
environment:
  RUST_LOG: info
  PORT: "8080"
restart:
  strategy: always
  delay_secs: 3
  max_retries: 5
depends_on:
  after:
    - database
  requires:
    - database
notify: true
watchdog_sec: 30
timeout_start_sec: 120
timeout_stop_sec: 60
"#;
        let spec: ServiceSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert_eq!(spec.name, "web-server");
        assert_eq!(spec.service_type, ServiceType::Notify);
        assert_eq!(spec.args.len(), 2);
        assert_eq!(spec.user.as_deref(), Some("www-data"));
        assert_eq!(spec.environment.get("RUST_LOG").unwrap(), "info");
        assert_eq!(spec.restart.strategy, RestartStrategy::Always);
        assert_eq!(spec.restart.max_retries, 5);
        assert_eq!(spec.depends_on.after, vec!["database"]);
        assert!(spec.notify);
        assert_eq!(spec.watchdog_sec, 30);
        assert_eq!(spec.timeout_start_sec, 120);
    }
}
