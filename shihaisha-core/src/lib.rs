pub mod diff;
pub mod error;
pub mod graph;
pub mod merge;
pub mod traits;
pub mod types;

pub use diff::{Change, diff};
pub use error::{Error, Result};
pub use graph::{resolve_order, validate_references};
pub use merge::Merge;
pub use traits::config_translator::{ConfigEmitter, ConfigParser};
pub use traits::health_checker::HealthChecker;
pub use traits::init_backend::InitBackend;
pub use types::backend_overrides::BackendOverrides;
pub use types::service_group::{GroupRestartStrategy, ServiceGroup};
pub use types::health_check::{HealthCheckResult, HealthCheckSpec};
pub use types::logging::{LogTarget, LoggingSpec};
pub use types::resource_limits::{MemorySize, NiceValue, ResourceLimits, Weight};
pub use types::service_spec::{
    DependencyCondition, DependencySpec, RestartPolicy, RestartStrategy, ServiceSpec, ServiceType,
};
pub use types::service_status::{HealthState, ServicePhase, ServiceState, ServiceStatus};
pub use types::socket_spec::{SocketSpec, SocketType};

/// Mock backend for testing -- records all calls and returns sensible defaults.
///
/// Available behind the `test-support` feature or in test builds of this crate.
#[cfg(any(test, feature = "test-support"))]
pub mod mock {
    use crate::traits::init_backend::InitBackend;
    use crate::types::service_spec::ServiceSpec;
    use crate::types::service_status::{ServiceState, ServiceStatus};
    use crate::Result;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    /// Recorded method call on the mock backend.
    #[derive(Debug, Clone)]
    pub enum Call {
        Install(String),
        Uninstall(String),
        Start(String),
        Stop(String),
        Restart(String),
        Reload(String),
        Status(String),
        Logs(String, u32),
        Enable(String),
        Disable(String),
        List,
        DaemonReload,
    }

    /// A mock `InitBackend` that records all calls and returns configurable defaults.
    pub struct MockBackend {
        /// Recorded calls.
        pub calls: Arc<Mutex<Vec<Call>>>,
        /// Whether this backend reports itself as available.
        pub available: bool,
    }

    impl MockBackend {
        /// Create a new mock backend that is available.
        #[must_use]
        pub fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                available: true,
            }
        }

        /// Return all recorded calls.
        pub async fn call_log(&self) -> Vec<Call> {
            self.calls.lock().await.clone()
        }
    }

    impl Default for MockBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl InitBackend for MockBackend {
        async fn install(&self, spec: &ServiceSpec) -> Result<()> {
            self.calls.lock().await.push(Call::Install(spec.name.clone()));
            Ok(())
        }

        async fn uninstall(&self, name: &str) -> Result<()> {
            self.calls.lock().await.push(Call::Uninstall(name.to_owned()));
            Ok(())
        }

        async fn start(&self, name: &str) -> Result<()> {
            self.calls.lock().await.push(Call::Start(name.to_owned()));
            Ok(())
        }

        async fn stop(&self, name: &str) -> Result<()> {
            self.calls.lock().await.push(Call::Stop(name.to_owned()));
            Ok(())
        }

        async fn restart(&self, name: &str) -> Result<()> {
            self.calls.lock().await.push(Call::Restart(name.to_owned()));
            Ok(())
        }

        async fn reload(&self, name: &str) -> Result<()> {
            self.calls.lock().await.push(Call::Reload(name.to_owned()));
            Ok(())
        }

        async fn status(&self, name: &str) -> Result<ServiceStatus> {
            self.calls.lock().await.push(Call::Status(name.to_owned()));
            Ok(ServiceStatus::new(name, ServiceState::Inactive, "mock"))
        }

        async fn logs(&self, name: &str, lines: u32) -> Result<Vec<String>> {
            self.calls.lock().await.push(Call::Logs(name.to_owned(), lines));
            Ok(vec![])
        }

        async fn enable(&self, name: &str) -> Result<()> {
            self.calls.lock().await.push(Call::Enable(name.to_owned()));
            Ok(())
        }

        async fn disable(&self, name: &str) -> Result<()> {
            self.calls.lock().await.push(Call::Disable(name.to_owned()));
            Ok(())
        }

        async fn list(&self) -> Result<Vec<ServiceStatus>> {
            self.calls.lock().await.push(Call::List);
            Ok(vec![])
        }

        async fn daemon_reload(&self) -> Result<()> {
            self.calls.lock().await.push(Call::DaemonReload);
            Ok(())
        }

        fn available(&self) -> bool {
            self.available
        }

        fn name(&self) -> &str {
            "mock"
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::mock::{Call, MockBackend};
    use crate::traits::init_backend::InitBackend;

    #[tokio::test]
    async fn mock_backend_records_all_call_types() {
        let mock = MockBackend::new();
        let spec = crate::ServiceSpec::new("test-svc", "/bin/echo");

        mock.install(&spec).await.unwrap();
        mock.start("test-svc").await.unwrap();
        mock.stop("test-svc").await.unwrap();
        mock.restart("test-svc").await.unwrap();
        mock.reload("test-svc").await.unwrap();
        mock.status("test-svc").await.unwrap();
        mock.logs("test-svc", 50).await.unwrap();
        mock.enable("test-svc").await.unwrap();
        mock.disable("test-svc").await.unwrap();
        mock.list().await.unwrap();
        mock.daemon_reload().await.unwrap();
        mock.uninstall("test-svc").await.unwrap();

        let calls = mock.call_log().await;
        assert_eq!(calls.len(), 12);
        assert!(matches!(&calls[0], Call::Install(n) if n == "test-svc"));
        assert!(matches!(&calls[1], Call::Start(n) if n == "test-svc"));
        assert!(matches!(&calls[2], Call::Stop(n) if n == "test-svc"));
        assert!(matches!(&calls[3], Call::Restart(n) if n == "test-svc"));
        assert!(matches!(&calls[4], Call::Reload(n) if n == "test-svc"));
        assert!(matches!(&calls[5], Call::Status(n) if n == "test-svc"));
        assert!(matches!(&calls[6], Call::Logs(n, 50) if n == "test-svc"));
        assert!(matches!(&calls[7], Call::Enable(n) if n == "test-svc"));
        assert!(matches!(&calls[8], Call::Disable(n) if n == "test-svc"));
        assert!(matches!(&calls[9], Call::List));
        assert!(matches!(&calls[10], Call::DaemonReload));
        assert!(matches!(&calls[11], Call::Uninstall(n) if n == "test-svc"));
    }

    #[test]
    fn mock_backend_available_flag() {
        let mock = MockBackend::new();
        assert!(mock.available());
        assert_eq!(mock.name(), "mock");

        let unavailable = MockBackend {
            calls: Default::default(),
            available: false,
        };
        assert!(!unavailable.available());
    }

    #[test]
    fn mock_backend_default_trait() {
        let mock = MockBackend::default();
        assert!(mock.available());
    }

    #[tokio::test]
    async fn mock_status_returns_inactive() {
        let mock = MockBackend::new();
        let status = mock.status("my-svc").await.unwrap();
        assert_eq!(status.name, "my-svc");
        assert_eq!(status.state, crate::ServiceState::Inactive);
        assert_eq!(status.backend, "mock");
    }

    #[tokio::test]
    async fn mock_logs_returns_empty() {
        let mock = MockBackend::new();
        let logs = mock.logs("svc", 100).await.unwrap();
        assert!(logs.is_empty());
    }

    #[tokio::test]
    async fn mock_list_returns_empty() {
        let mock = MockBackend::new();
        let list = mock.list().await.unwrap();
        assert!(list.is_empty());
    }
}
