use shihaisha_core::traits::init_backend::InitBackend;
use std::collections::HashMap;

/// Registry of available init system backends.
///
/// Auto-detects which backends are available on the current platform
/// and provides access to them by name.
pub struct BackendRegistry {
    backends: HashMap<String, Box<dyn InitBackend>>,
    default: String,
}

impl BackendRegistry {
    /// Auto-detect available backends on the current platform.
    #[must_use]
    pub fn detect() -> Self {
        let mut backends: HashMap<String, Box<dyn InitBackend>> = HashMap::new();
        let mut default = String::from("native");

        #[cfg(feature = "launchd")]
        {
            let launchd = crate::launchd::LaunchdBackend::new();
            if launchd.available() {
                default = "launchd".into();
                backends.insert("launchd".into(), Box::new(launchd));
            }
        }

        #[cfg(feature = "systemd")]
        {
            let systemd = crate::systemd::SystemdBackend::new();
            if systemd.available() {
                default = "systemd".into();
                backends.insert("systemd".into(), Box::new(systemd));
            }
        }

        #[cfg(feature = "supervisord")]
        {
            let supervisord = crate::supervisord::SupervisordBackend::new();
            if supervisord.available() {
                backends.insert("supervisord".into(), Box::new(supervisord));
            }
        }

        #[cfg(feature = "native")]
        {
            let native = crate::native::NativeBackend::new();
            backends.insert("native".into(), Box::new(native));
        }

        Self { backends, default }
    }

    /// Create an empty registry (for testing).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            backends: HashMap::new(),
            default: String::new(),
        }
    }

    /// Create a registry with a single named backend (for testing).
    #[must_use]
    pub fn with_backend(name: impl Into<String>, backend: Box<dyn InitBackend>) -> Self {
        let name = name.into();
        let mut backends: HashMap<String, Box<dyn InitBackend>> = HashMap::new();
        backends.insert(name.clone(), backend);
        Self {
            backends,
            default: name,
        }
    }

    /// Get a backend by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&dyn InitBackend> {
        self.backends.get(name).map(AsRef::as_ref)
    }

    /// Get the default backend for this platform.
    #[must_use]
    pub fn default_backend(&self) -> Option<&dyn InitBackend> {
        self.backends.get(&self.default).map(AsRef::as_ref)
    }

    /// List all available backend names.
    #[must_use]
    pub fn available_backends(&self) -> Vec<&str> {
        self.backends.keys().map(String::as_str).collect()
    }

    /// Get the name of the default backend.
    #[must_use]
    pub fn default_name(&self) -> &str {
        &self.default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn detect_has_at_least_one_backend() {
        let registry = BackendRegistry::detect();
        assert!(
            !registry.available_backends().is_empty(),
            "should detect at least one backend"
        );
    }

    #[tokio::test]
    async fn default_backend_exists() {
        let registry = BackendRegistry::detect();
        assert!(
            registry.default_backend().is_some(),
            "default backend should exist"
        );
    }

    #[tokio::test]
    async fn get_nonexistent_returns_none() {
        let registry = BackendRegistry::detect();
        assert!(registry.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn default_name_matches_a_backend() {
        let registry = BackendRegistry::detect();
        let name = registry.default_name();
        assert!(
            registry.get(name).is_some(),
            "default_name should correspond to an available backend"
        );
    }

    #[test]
    fn empty_registry_has_no_backends() {
        let registry = BackendRegistry::empty();
        assert!(registry.available_backends().is_empty());
        assert!(registry.default_backend().is_none());
    }

    #[tokio::test]
    async fn with_backend_contains_the_backend() {
        let mock = shihaisha_core::mock::MockBackend::new();
        let registry = BackendRegistry::with_backend("mock", Box::new(mock));
        assert!(registry.get("mock").is_some());
        assert_eq!(registry.default_name(), "mock");
        assert_eq!(registry.available_backends().len(), 1);

        // Verify the backend works
        let backend = registry.default_backend().unwrap();
        let status = backend.status("test").await.unwrap();
        assert_eq!(status.backend, "mock");
    }

    #[test]
    fn empty_registry_default_name_is_empty() {
        let registry = BackendRegistry::empty();
        assert_eq!(registry.default_name(), "");
    }

    #[test]
    fn empty_registry_get_returns_none() {
        let registry = BackendRegistry::empty();
        assert!(registry.get("native").is_none());
        assert!(registry.get("").is_none());
    }

    #[test]
    fn with_backend_get_wrong_name_returns_none() {
        let mock = shihaisha_core::mock::MockBackend::new();
        let registry = BackendRegistry::with_backend("mock", Box::new(mock));
        assert!(registry.get("native").is_none());
        assert!(registry.get("systemd").is_none());
    }
}
