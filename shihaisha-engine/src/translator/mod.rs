/// Config translation utilities.
///
/// Implements the `ConfigTranslator` trait for each backend and provides
/// standalone translation functions for use outside the service lifecycle.

#[cfg(feature = "systemd")]
pub mod systemd_translator;

#[cfg(feature = "launchd")]
pub mod launchd_translator;

#[cfg(feature = "supervisord")]
pub mod supervisord_translator;
