use serde::{Deserialize, Serialize};

/// Runtime status of a managed service.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}
