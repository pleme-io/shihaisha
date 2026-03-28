use crate::types::service_spec::ServiceSpec;
use crate::Result;

/// Translate a canonical `ServiceSpec` into a backend-native config format.
///
/// Every backend that can generate native configuration (systemd unit files,
/// launchd plists, supervisord INI sections, etc.) implements this trait.
pub trait ConfigEmitter: Send + Sync {
    /// Generate the native configuration string from a `ServiceSpec`.
    fn emit(&self, spec: &ServiceSpec) -> Result<String>;

    /// File extension for the native format (e.g., `"service"`, `"plist"`, `"conf"`).
    fn extension(&self) -> &str;

    /// Backend name (e.g., `"systemd"`, `"launchd"`, `"supervisord"`).
    fn name(&self) -> &str;
}

/// Parse a backend-native config format back into a `ServiceSpec`.
///
/// This is an optional capability for backends that support importing
/// existing native configurations. Not every backend needs to implement this.
pub trait ConfigParser: Send + Sync {
    /// Parse a native configuration string into a `ServiceSpec` (best-effort).
    fn parse(&self, content: &str) -> Result<ServiceSpec>;

    /// Backend name (e.g., `"systemd"`, `"launchd"`, `"supervisord"`).
    fn name(&self) -> &str;
}
