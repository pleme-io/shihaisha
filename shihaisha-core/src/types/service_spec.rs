use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;

use super::backend_overrides::BackendOverrides;
use super::health_check::HealthCheckSpec;
use super::logging::LoggingSpec;
use super::resource_limits::ResourceLimits;
use super::socket_spec::SocketSpec;

/// The canonical service specification.
/// Written as YAML by users, translated to backend-native formats at runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

    /// Liveness probe -- is the process alive? If failing, restart it.
    /// Accepts the legacy `health` YAML key for backward compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "health")]
    pub liveness: Option<HealthCheckSpec>,

    /// Readiness probe -- is the process ready to serve?
    /// Dependents wait for this before starting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readiness: Option<HealthCheckSpec>,

    /// Startup probe -- has the process finished initializing?
    /// Suppresses liveness checks during slow startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup: Option<HealthCheckSpec>,

    /// If true, failure of this service triggers shutdown of all dependents.
    /// Use for critical infrastructure services (databases, message brokers).
    #[serde(default)]
    pub critical: bool,

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
    /// Long-running with `sd_notify` readiness.
    Notify,
    /// Traditional daemon that forks.
    Forking,
    /// Timer-triggered task.
    Timer,
    /// Socket-activated service.
    Socket,
}

impl fmt::Display for ServiceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Simple => write!(f, "simple"),
            Self::Oneshot => write!(f, "oneshot"),
            Self::Notify => write!(f, "notify"),
            Self::Forking => write!(f, "forking"),
            Self::Timer => write!(f, "timer"),
            Self::Socket => write!(f, "socket"),
        }
    }
}

/// How and when to restart the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

impl fmt::Display for RestartStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Always => write!(f, "always"),
            Self::OnFailure => write!(f, "on-failure"),
            Self::OnSuccess => write!(f, "on-success"),
            Self::Never => write!(f, "never"),
        }
    }
}

/// Condition that must be met before a dependency is considered satisfied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DependencyCondition {
    /// Dependency process has started (default).
    #[default]
    ServiceStarted,
    /// Dependency has passed its readiness probe.
    ServiceHealthy,
    /// Dependency has exited successfully (for oneshot services).
    ServiceCompletedSuccessfully,
}

impl fmt::Display for DependencyCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServiceStarted => write!(f, "service_started"),
            Self::ServiceHealthy => write!(f, "service_healthy"),
            Self::ServiceCompletedSuccessfully => write!(f, "service_completed_successfully"),
        }
    }
}

/// Dependency and ordering specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
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

    /// Conditions for each dependency in `requires` and `wants`.
    /// Map from service name to condition. Services not in the map default
    /// to `ServiceStarted`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub conditions: BTreeMap<String, DependencyCondition>,

    /// Services that must be stopped BEFORE this service stops.
    /// Enables ordered graceful shutdown (reverse of startup).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_before: Vec<String>,

    /// Services that must be stopped AFTER this service stops.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_after: Vec<String>,
}

impl ServiceSpec {
    /// Create a new `ServiceSpec` with the given name and command, filling
    /// sensible defaults for all other fields.
    ///
    /// # Examples
    ///
    /// ```
    /// use shihaisha_core::ServiceSpec;
    ///
    /// let spec = ServiceSpec::new("my-app", "/usr/bin/my-app");
    /// assert_eq!(spec.name, "my-app");
    /// assert_eq!(spec.command, "/usr/bin/my-app");
    /// assert!(spec.args.is_empty());
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            description: String::new(),
            command: command.into(),
            args: vec![],
            service_type: ServiceType::default(),
            working_directory: None,
            user: None,
            group: None,
            environment: HashMap::new(),
            restart: RestartPolicy::default(),
            depends_on: DependencySpec::default(),
            liveness: None,
            readiness: None,
            startup: None,
            critical: false,
            sockets: vec![],
            resources: None,
            logging: LoggingSpec::default(),
            notify: false,
            watchdog_sec: 0,
            timeout_start_sec: default_timeout(),
            timeout_stop_sec: default_timeout(),
            overrides: BackendOverrides::default(),
            name,
        }
    }

    /// Validate the spec, returning an error if any fields have invalid values.
    pub fn validate(&self) -> crate::Result<()> {
        if self.name.is_empty() {
            return Err(crate::Error::ConfigError(
                "service name must not be empty".to_owned(),
            ));
        }
        if self.command.is_empty() {
            return Err(crate::Error::ConfigError(
                "service command must not be empty".to_owned(),
            ));
        }
        if self.timeout_start_sec == 0 {
            return Err(crate::Error::ConfigError(
                "timeout_start_sec must be > 0".to_owned(),
            ));
        }
        if self.timeout_stop_sec == 0 {
            return Err(crate::Error::ConfigError(
                "timeout_stop_sec must be > 0".to_owned(),
            ));
        }
        // Note: cpu_weight, io_weight, and nice are validated at
        // construction time by their newtype wrappers (Weight, NiceValue).
        // No additional range checks needed here.
        if self.restart.strategy != RestartStrategy::Never && self.restart.delay_secs == 0 {
            return Err(crate::Error::ConfigError(
                "restart.delay_secs must be > 0 when strategy is not Never".to_owned(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_roundtrip() {
        let spec = ServiceSpec::new("test-service", "/usr/bin/test");
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
        assert!(dep.conditions.is_empty());
        assert!(dep.stop_before.is_empty());
        assert!(dep.stop_after.is_empty());
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

    #[test]
    fn new_constructor_defaults() {
        let spec = ServiceSpec::new("test", "/bin/true");
        assert_eq!(spec.name, "test");
        assert_eq!(spec.command, "/bin/true");
        assert_eq!(spec.service_type, ServiceType::Simple);
        assert_eq!(spec.restart.strategy, RestartStrategy::OnFailure);
        assert_eq!(spec.timeout_start_sec, 90);
        assert!(spec.args.is_empty());
        assert!(spec.environment.is_empty());
    }

    #[test]
    fn validate_valid_spec() {
        let spec = ServiceSpec::new("test", "/bin/true");
        spec.validate().expect("should be valid");
    }

    #[test]
    fn validate_empty_name() {
        let spec = ServiceSpec::new("", "/bin/true");
        let err = spec.validate().unwrap_err();
        assert!(err.to_string().contains("name must not be empty"));
    }

    #[test]
    fn validate_empty_command() {
        let spec = ServiceSpec::new("test", "");
        let err = spec.validate().unwrap_err();
        assert!(err.to_string().contains("command must not be empty"));
    }

    #[test]
    fn validate_zero_timeout_start() {
        let mut spec = ServiceSpec::new("test", "/bin/true");
        spec.timeout_start_sec = 0;
        let err = spec.validate().unwrap_err();
        assert!(err.to_string().contains("timeout_start_sec must be > 0"));
    }

    #[test]
    fn validate_zero_timeout_stop() {
        let mut spec = ServiceSpec::new("test", "/bin/true");
        spec.timeout_stop_sec = 0;
        let err = spec.validate().unwrap_err();
        assert!(err.to_string().contains("timeout_stop_sec must be > 0"));
    }

    #[test]
    fn weight_rejects_out_of_range() {
        // Weight and NiceValue newtypes self-validate at construction.
        use crate::types::resource_limits::Weight;
        let err = Weight::new(0).unwrap_err();
        assert!(err.to_string().contains("weight must be 1-10000"));
        let err = Weight::new(10001).unwrap_err();
        assert!(err.to_string().contains("weight must be 1-10000"));
    }

    #[test]
    fn nice_rejects_out_of_range() {
        use crate::types::resource_limits::NiceValue;
        let err = NiceValue::new(20).unwrap_err();
        assert!(err.to_string().contains("nice must be -20..19"));
        let err = NiceValue::new(-21).unwrap_err();
        assert!(err.to_string().contains("nice must be -20..19"));
    }

    #[test]
    fn validate_restart_delay_zero_with_strategy() {
        let mut spec = ServiceSpec::new("test", "/bin/true");
        spec.restart.strategy = RestartStrategy::Always;
        spec.restart.delay_secs = 0;
        let err = spec.validate().unwrap_err();
        assert!(err.to_string().contains("delay_secs must be > 0"));
    }

    #[test]
    fn validate_restart_delay_zero_with_never() {
        let mut spec = ServiceSpec::new("test", "/bin/true");
        spec.restart.strategy = RestartStrategy::Never;
        spec.restart.delay_secs = 0;
        spec.validate().expect("Never strategy allows delay_secs=0");
    }

    #[test]
    fn service_type_display() {
        assert_eq!(ServiceType::Simple.to_string(), "simple");
        assert_eq!(ServiceType::Oneshot.to_string(), "oneshot");
        assert_eq!(ServiceType::Forking.to_string(), "forking");
        assert_eq!(ServiceType::Notify.to_string(), "notify");
        assert_eq!(ServiceType::Timer.to_string(), "timer");
        assert_eq!(ServiceType::Socket.to_string(), "socket");
    }

    #[test]
    fn restart_strategy_display() {
        assert_eq!(RestartStrategy::Always.to_string(), "always");
        assert_eq!(RestartStrategy::OnFailure.to_string(), "on-failure");
        assert_eq!(RestartStrategy::OnSuccess.to_string(), "on-success");
        assert_eq!(RestartStrategy::Never.to_string(), "never");
    }

    // --- Liveness / readiness / startup probe tests ---

    #[test]
    fn legacy_health_field_deserializes_as_liveness() {
        let yaml = r#"
name: legacy-svc
command: /usr/bin/app
health:
  type: http
  endpoint: http://localhost:8080/health
"#;
        let spec: ServiceSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert!(spec.liveness.is_some(), "health alias should populate liveness");
        assert!(spec.readiness.is_none());
        assert!(spec.startup.is_none());
    }

    #[test]
    fn new_probe_fields_deserialize() {
        let yaml = r#"
name: probed-svc
command: /usr/bin/app
liveness:
  type: http
  endpoint: http://localhost:8080/live
readiness:
  type: tcp
  address: 127.0.0.1:5432
startup:
  type: command
  command: /usr/bin/check-init
"#;
        let spec: ServiceSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert!(spec.liveness.is_some());
        assert!(spec.readiness.is_some());
        assert!(spec.startup.is_some());
    }

    #[test]
    fn probes_default_to_none() {
        let spec = ServiceSpec::new("test", "/bin/true");
        assert!(spec.liveness.is_none());
        assert!(spec.readiness.is_none());
        assert!(spec.startup.is_none());
    }

    // --- critical field tests ---

    #[test]
    fn critical_defaults_to_false() {
        let spec = ServiceSpec::new("test", "/bin/true");
        assert!(!spec.critical);
    }

    #[test]
    fn critical_field_deserializes() {
        let yaml = r#"
name: db
command: /usr/bin/postgres
critical: true
"#;
        let spec: ServiceSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert!(spec.critical);
    }

    #[test]
    fn critical_field_roundtrip() {
        let mut spec = ServiceSpec::new("db", "/usr/bin/postgres");
        spec.critical = true;
        let yaml = serde_yaml_ng::to_string(&spec).expect("serialize");
        let parsed: ServiceSpec = serde_yaml_ng::from_str(&yaml).expect("deserialize");
        assert!(parsed.critical);
    }

    // --- DependencyCondition tests ---

    #[test]
    fn dependency_condition_serializes_snake_case() {
        let json = serde_json::to_string(&DependencyCondition::ServiceHealthy).expect("serialize");
        assert_eq!(json, "\"service_healthy\"");

        let parsed: DependencyCondition =
            serde_json::from_str("\"service_completed_successfully\"").expect("parse");
        assert_eq!(parsed, DependencyCondition::ServiceCompletedSuccessfully);
    }

    #[test]
    fn dependency_condition_default_is_service_started() {
        assert_eq!(
            DependencyCondition::default(),
            DependencyCondition::ServiceStarted,
        );
    }

    #[test]
    fn dependency_condition_display() {
        assert_eq!(
            DependencyCondition::ServiceStarted.to_string(),
            "service_started",
        );
        assert_eq!(
            DependencyCondition::ServiceHealthy.to_string(),
            "service_healthy",
        );
        assert_eq!(
            DependencyCondition::ServiceCompletedSuccessfully.to_string(),
            "service_completed_successfully",
        );
    }

    #[test]
    fn conditions_map_deserializes() {
        let yaml = r#"
name: app
command: /usr/bin/app
depends_on:
  requires:
    - database
    - cache
  conditions:
    database: service_healthy
    cache: service_completed_successfully
"#;
        let spec: ServiceSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert_eq!(
            spec.depends_on.conditions.get("database"),
            Some(&DependencyCondition::ServiceHealthy),
        );
        assert_eq!(
            spec.depends_on.conditions.get("cache"),
            Some(&DependencyCondition::ServiceCompletedSuccessfully),
        );
    }

    // --- Shutdown ordering tests ---

    #[test]
    fn shutdown_ordering_deserializes() {
        let yaml = r#"
name: app
command: /usr/bin/app
depends_on:
  stop_before:
    - cache
  stop_after:
    - database
"#;
        let spec: ServiceSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert_eq!(spec.depends_on.stop_before, vec!["cache"]);
        assert_eq!(spec.depends_on.stop_after, vec!["database"]);
    }

    #[test]
    fn shutdown_ordering_roundtrip() {
        let mut spec = ServiceSpec::new("app", "/usr/bin/app");
        spec.depends_on.stop_before = vec!["cache".to_owned()];
        spec.depends_on.stop_after = vec!["database".to_owned()];

        let yaml = serde_yaml_ng::to_string(&spec).expect("serialize");
        let parsed: ServiceSpec = serde_yaml_ng::from_str(&yaml).expect("deserialize");
        assert_eq!(parsed.depends_on.stop_before, vec!["cache"]);
        assert_eq!(parsed.depends_on.stop_after, vec!["database"]);
    }
}
