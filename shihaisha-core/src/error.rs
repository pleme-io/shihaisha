use std::fmt;

/// Errors that can occur in shihaisha service management.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Service with the given name was not found.
    #[error("service not found: {0}")]
    ServiceNotFound(String),

    /// A service with the given name already exists.
    #[error("service already exists: {0}")]
    ServiceAlreadyExists(String),

    /// The requested backend is not available on this system.
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),

    /// An error occurred in the backend.
    #[error("backend error: {0}")]
    BackendError(String),

    /// Configuration error.
    #[error("config error: {0}")]
    ConfigError(String),

    /// A dependency requirement could not be satisfied.
    #[error("dependency error: {0}")]
    DependencyError(String),

    /// A health check failed.
    #[error("health check failed: {0}")]
    HealthCheckFailed(String),

    /// An operation timed out.
    #[error("timeout: service '{service}' exceeded {timeout_secs}s")]
    TimeoutError {
        /// The service that timed out.
        service: String,
        /// The timeout duration in seconds.
        timeout_secs: u64,
    },

    /// An I/O error occurred.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A serialization or deserialization error occurred.
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Result type alias for shihaisha operations.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Returns `true` if this is a retryable error.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Io(_) | Self::BackendError(_) | Self::TimeoutError { .. })
    }
}

// Allow `Error` to be compared for testing purposes.
impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        // Compare by Debug representation since Io errors don't implement PartialEq.
        fmt::format(format_args!("{self:?}")) == fmt::format(format_args!("{other:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_service_not_found() {
        let err = Error::ServiceNotFound("myapp".to_owned());
        assert_eq!(err.to_string(), "service not found: myapp");
    }

    #[test]
    fn error_display_timeout() {
        let err = Error::TimeoutError {
            service: "myapp".to_owned(),
            timeout_secs: 30,
        };
        assert_eq!(err.to_string(), "timeout: service 'myapp' exceeded 30s");
    }

    #[test]
    fn io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn retryable_errors() {
        assert!(Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "x")).is_retryable());
        assert!(Error::BackendError("x".to_owned()).is_retryable());
        assert!(Error::TimeoutError { service: "x".to_owned(), timeout_secs: 1 }.is_retryable());
        assert!(!Error::ServiceNotFound("x".to_owned()).is_retryable());
        assert!(!Error::ConfigError("x".to_owned()).is_retryable());
    }
}
