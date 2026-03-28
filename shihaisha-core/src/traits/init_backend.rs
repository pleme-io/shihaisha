use crate::types::service_spec::ServiceSpec;
use crate::types::service_status::ServiceStatus;
use crate::Result;
use async_trait::async_trait;

/// Backend for a specific init system (systemd, launchd, or native).
///
/// Each backend translates the canonical `ServiceSpec` into its native format
/// and provides lifecycle management for services.
#[async_trait]
pub trait InitBackend: Send + Sync {
    /// Install a service definition (write unit file / plist / config).
    async fn install(&self, spec: &ServiceSpec) -> Result<()>;

    /// Remove a service definition.
    async fn uninstall(&self, name: &str) -> Result<()>;

    /// Start a service.
    async fn start(&self, name: &str) -> Result<()>;

    /// Stop a service.
    async fn stop(&self, name: &str) -> Result<()>;

    /// Restart a service.
    async fn restart(&self, name: &str) -> Result<()>;

    /// Reload a service's configuration (SIGHUP or equivalent).
    async fn reload(&self, name: &str) -> Result<()>;

    /// Get the current status of a service.
    async fn status(&self, name: &str) -> Result<ServiceStatus>;

    /// Get recent log lines for a service.
    async fn logs(&self, name: &str, lines: u32) -> Result<Vec<String>>;

    /// Enable a service to start on boot.
    async fn enable(&self, name: &str) -> Result<()>;

    /// Disable a service from starting on boot.
    async fn disable(&self, name: &str) -> Result<()>;

    /// List all managed services.
    async fn list(&self) -> Result<Vec<ServiceStatus>>;

    /// Reload the init system's configuration (daemon-reload / bootout+bootstrap).
    async fn daemon_reload(&self) -> Result<()>;

    /// Check if this backend is available on the current system.
    fn available(&self) -> bool;

    /// Backend name for display.
    fn name(&self) -> &str;
}
