use crate::types::service_spec::ServiceSpec;
use crate::Result;

/// Translates canonical `ServiceSpec` to backend-native format and back.
///
/// Each backend implements this to generate its native config (systemd unit
/// files, launchd plists, etc.) from the universal service specification.
pub trait ConfigTranslator: Send + Sync {
    /// Generate the native configuration (systemd unit, launchd plist, etc.).
    fn translate(&self, spec: &ServiceSpec) -> Result<String>;

    /// Parse a native configuration back into a `ServiceSpec` (best-effort).
    fn parse_native(&self, content: &str) -> Result<ServiceSpec>;

    /// File extension for the native format (e.g., `"service"`, `"plist"`).
    fn extension(&self) -> &str;

    /// Backend name.
    fn name(&self) -> &str;
}
