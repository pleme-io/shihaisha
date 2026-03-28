use serde::{Deserialize, Serialize};
use std::fmt;

/// Runtime status of a managed service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceStatus {
    /// Service name.
    pub name: String,

    /// Current state.
    pub state: ServiceState,

    /// Process ID (if running).
    #[serde(default)]
    pub pid: Option<u32>,

    /// Last exit code (if stopped/failed).
    #[serde(default)]
    pub exit_code: Option<i32>,

    /// When the service was last started.
    #[serde(default)]
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,

    /// Uptime in seconds (if running).
    #[serde(default)]
    pub uptime_secs: Option<u64>,

    /// Number of times the service has been restarted.
    #[serde(default)]
    pub restart_count: u32,

    /// Health state.
    #[serde(default)]
    pub health: HealthState,

    /// Which backend is managing this service.
    pub backend: String,

    /// Resident memory in bytes (if available).
    #[serde(default)]
    pub memory_bytes: Option<u64>,

    /// CPU usage percentage (if available).
    #[serde(default)]
    pub cpu_usage_percent: Option<f64>,
}

/// Process lifecycle state of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    /// Not started / not loaded.
    Inactive,
    /// Starting up.
    Starting,
    /// Running and ready.
    Running,
    /// Running but health checks failing.
    Degraded,
    /// Reloading configuration.
    Reloading,
    /// Shutting down.
    Stopping,
    /// Cleanly stopped.
    Stopped,
    /// Exited with non-zero or was killed.
    Failed,
    /// State cannot be determined.
    Unknown,
}

/// High-level phase (Kubernetes-style summary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServicePhase {
    /// Waiting to start.
    Pending,
    /// Actively running (may be degraded).
    Running,
    /// Completed successfully.
    Succeeded,
    /// Crashed or exceeded restart limit.
    Failed,
    /// State cannot be determined.
    Unknown,
}

impl fmt::Display for ServicePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Succeeded => write!(f, "succeeded"),
            Self::Failed => write!(f, "failed"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl ServiceState {
    /// Map detailed state to high-level phase.
    #[must_use]
    pub fn phase(&self) -> ServicePhase {
        match self {
            Self::Inactive | Self::Stopped => ServicePhase::Pending,
            Self::Starting => ServicePhase::Pending,
            Self::Running | Self::Degraded | Self::Reloading => ServicePhase::Running,
            Self::Stopping => ServicePhase::Running,
            Self::Failed => ServicePhase::Failed,
            Self::Unknown => ServicePhase::Unknown,
        }
    }
}

impl fmt::Display for ServiceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inactive => write!(f, "inactive"),
            Self::Starting => write!(f, "starting"),
            Self::Running => write!(f, "running"),
            Self::Degraded => write!(f, "degraded"),
            Self::Reloading => write!(f, "reloading"),
            Self::Stopping => write!(f, "stopping"),
            Self::Stopped => write!(f, "stopped"),
            Self::Failed => write!(f, "failed"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Health state of a service (separate from process state).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    /// Health state not yet determined.
    #[default]
    Unknown,
    /// All health checks passing.
    Healthy,
    /// Health checks failing.
    Unhealthy,
    /// Some checks passing, some failing.
    Degraded,
}

impl fmt::Display for HealthState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => write!(f, "unknown"),
            Self::Healthy => write!(f, "healthy"),
            Self::Unhealthy => write!(f, "unhealthy"),
            Self::Degraded => write!(f, "degraded"),
        }
    }
}

impl ServiceStatus {
    /// Create a new `ServiceStatus` with the given name, state, and backend,
    /// defaulting all optional fields.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        state: ServiceState,
        backend: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            state,
            pid: None,
            exit_code: None,
            started_at: None,
            uptime_secs: None,
            restart_count: 0,
            health: HealthState::Unknown,
            backend: backend.into(),
            memory_bytes: None,
            cpu_usage_percent: None,
        }
    }

    /// Set the PID on this status (builder pattern).
    #[must_use]
    pub fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_state_serializes_lowercase() {
        let json = serde_json::to_string(&ServiceState::Running).expect("serialize");
        assert_eq!(json, "\"running\"");

        let parsed: ServiceState = serde_json::from_str("\"failed\"").expect("parse");
        assert_eq!(parsed, ServiceState::Failed);
    }

    #[test]
    fn health_state_default_is_unknown() {
        assert_eq!(HealthState::default(), HealthState::Unknown);
    }

    #[test]
    fn health_state_serializes_lowercase() {
        let json = serde_json::to_string(&HealthState::Healthy).expect("serialize");
        assert_eq!(json, "\"healthy\"");

        let json = serde_json::to_string(&HealthState::Degraded).expect("serialize");
        assert_eq!(json, "\"degraded\"");
    }

    #[test]
    fn service_status_from_json() {
        let json = r#"{
            "name": "web-server",
            "state": "running",
            "pid": 12345,
            "started_at": "2026-01-15T10:30:00Z",
            "uptime_secs": 3600,
            "restart_count": 2,
            "health": "healthy",
            "backend": "systemd",
            "memory_bytes": 104857600,
            "cpu_usage_percent": 12.5
        }"#;
        let status: ServiceStatus = serde_json::from_str(json).expect("parse");
        assert_eq!(status.name, "web-server");
        assert_eq!(status.state, ServiceState::Running);
        assert_eq!(status.pid, Some(12345));
        assert_eq!(status.uptime_secs, Some(3600));
        assert_eq!(status.health, HealthState::Healthy);
        assert_eq!(status.backend, "systemd");
        assert_eq!(status.memory_bytes, Some(104_857_600));
    }

    #[test]
    fn service_status_minimal_json() {
        let json = r#"{
            "name": "worker",
            "state": "stopped",
            "backend": "launchd"
        }"#;
        let status: ServiceStatus = serde_json::from_str(json).expect("parse");
        assert_eq!(status.name, "worker");
        assert_eq!(status.state, ServiceState::Stopped);
        assert!(status.pid.is_none());
        assert_eq!(status.restart_count, 0);
        assert_eq!(status.health, HealthState::Unknown);
    }

    #[test]
    fn all_service_states_roundtrip() {
        let states = [
            ServiceState::Inactive,
            ServiceState::Starting,
            ServiceState::Running,
            ServiceState::Degraded,
            ServiceState::Reloading,
            ServiceState::Stopping,
            ServiceState::Stopped,
            ServiceState::Failed,
            ServiceState::Unknown,
        ];
        for state in states {
            let json = serde_json::to_string(&state).expect("serialize");
            let parsed: ServiceState = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, state);
        }
    }

    #[test]
    fn new_constructor_defaults() {
        let status = ServiceStatus::new("test-svc", ServiceState::Running, "native");
        assert_eq!(status.name, "test-svc");
        assert_eq!(status.state, ServiceState::Running);
        assert_eq!(status.backend, "native");
        assert!(status.pid.is_none());
        assert!(status.exit_code.is_none());
        assert_eq!(status.restart_count, 0);
        assert_eq!(status.health, HealthState::Unknown);
        assert!(status.memory_bytes.is_none());
    }

    #[test]
    fn with_pid_builder() {
        let status = ServiceStatus::new("test", ServiceState::Running, "native")
            .with_pid(12345);
        assert_eq!(status.pid, Some(12345));
    }

    #[test]
    fn service_state_display() {
        assert_eq!(ServiceState::Inactive.to_string(), "inactive");
        assert_eq!(ServiceState::Running.to_string(), "running");
        assert_eq!(ServiceState::Degraded.to_string(), "degraded");
        assert_eq!(ServiceState::Failed.to_string(), "failed");
        assert_eq!(ServiceState::Starting.to_string(), "starting");
        assert_eq!(ServiceState::Stopping.to_string(), "stopping");
        assert_eq!(ServiceState::Stopped.to_string(), "stopped");
        assert_eq!(ServiceState::Reloading.to_string(), "reloading");
        assert_eq!(ServiceState::Unknown.to_string(), "unknown");
    }

    #[test]
    fn health_state_display() {
        assert_eq!(HealthState::Unknown.to_string(), "unknown");
        assert_eq!(HealthState::Healthy.to_string(), "healthy");
        assert_eq!(HealthState::Unhealthy.to_string(), "unhealthy");
        assert_eq!(HealthState::Degraded.to_string(), "degraded");
    }

    // --- ServicePhase tests ---

    #[test]
    fn phase_mapping() {
        assert_eq!(ServiceState::Inactive.phase(), ServicePhase::Pending);
        assert_eq!(ServiceState::Stopped.phase(), ServicePhase::Pending);
        assert_eq!(ServiceState::Starting.phase(), ServicePhase::Pending);
        assert_eq!(ServiceState::Running.phase(), ServicePhase::Running);
        assert_eq!(ServiceState::Degraded.phase(), ServicePhase::Running);
        assert_eq!(ServiceState::Reloading.phase(), ServicePhase::Running);
        assert_eq!(ServiceState::Stopping.phase(), ServicePhase::Running);
        assert_eq!(ServiceState::Failed.phase(), ServicePhase::Failed);
        assert_eq!(ServiceState::Unknown.phase(), ServicePhase::Unknown);
    }

    #[test]
    fn service_phase_display() {
        assert_eq!(ServicePhase::Pending.to_string(), "pending");
        assert_eq!(ServicePhase::Running.to_string(), "running");
        assert_eq!(ServicePhase::Succeeded.to_string(), "succeeded");
        assert_eq!(ServicePhase::Failed.to_string(), "failed");
        assert_eq!(ServicePhase::Unknown.to_string(), "unknown");
    }

    #[test]
    fn service_phase_serializes_lowercase() {
        let json = serde_json::to_string(&ServicePhase::Running).expect("serialize");
        assert_eq!(json, "\"running\"");

        let parsed: ServicePhase = serde_json::from_str("\"failed\"").expect("parse");
        assert_eq!(parsed, ServicePhase::Failed);
    }

    #[test]
    fn degraded_state_serializes_lowercase() {
        let json = serde_json::to_string(&ServiceState::Degraded).expect("serialize");
        assert_eq!(json, "\"degraded\"");
        let parsed: ServiceState = serde_json::from_str("\"degraded\"").expect("parse");
        assert_eq!(parsed, ServiceState::Degraded);
    }
}
