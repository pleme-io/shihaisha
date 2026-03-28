use crate::types::health_check::HealthCheckSpec;
use crate::Result;
use async_trait::async_trait;

/// Executes health checks against a running service.
///
/// Implementations handle the actual probing (HTTP requests, TCP connects,
/// command execution, file checks).
#[async_trait]
pub trait HealthChecker: Send + Sync {
    /// Run the health check and return whether the service is healthy.
    async fn check(&self, spec: &HealthCheckSpec) -> Result<bool>;

    /// Name of this health checker implementation.
    fn name(&self) -> &str;
}
