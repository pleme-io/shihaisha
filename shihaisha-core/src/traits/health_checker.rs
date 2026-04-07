use crate::types::health_check::{HealthCheckResult, HealthCheckSpec};
use crate::Result;
use async_trait::async_trait;

/// Executes health checks against a running service.
///
/// Implementations handle the actual probing (HTTP requests, TCP connects,
/// command execution, file checks).
#[async_trait]
pub trait HealthChecker: Send + Sync {
    /// Run the health check and return a [`HealthCheckResult`] with diagnostics.
    async fn check(&self, spec: &HealthCheckSpec) -> Result<HealthCheckResult>;

    /// Name of this health checker implementation.
    fn name(&self) -> &'static str;
}
