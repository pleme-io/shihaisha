/// Errors that can occur in shihaisha service management.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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

    /// An error occurred in a specific backend operation.
    #[error("{backend}: {operation} failed: {detail}")]
    BackendError {
        /// Which backend produced the error (e.g., `"systemd"`, `"launchd"`).
        backend: String,
        /// The operation that failed (e.g., `"install"`, `"start"`, `"systemctl"`).
        operation: String,
        /// Human-readable detail about the failure.
        detail: String,
    },

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
        matches!(
            self,
            Self::Io(_) | Self::BackendError { .. } | Self::TimeoutError { .. }
        )
    }
}

// Allow `Error` to be compared for testing purposes.
impl PartialEq for Error {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            // Io errors: compare by ErrorKind (the inner error doesn't impl PartialEq).
            (Self::Io(a), Self::Io(b)) => a.kind() == b.kind(),
            // String-carrying variants: compare payloads directly.
            (Self::ServiceNotFound(a), Self::ServiceNotFound(b))
            | (Self::ServiceAlreadyExists(a), Self::ServiceAlreadyExists(b))
            | (Self::BackendUnavailable(a), Self::BackendUnavailable(b))
            | (Self::ConfigError(a), Self::ConfigError(b))
            | (Self::DependencyError(a), Self::DependencyError(b))
            | (Self::HealthCheckFailed(a), Self::HealthCheckFailed(b))
            | (Self::Serialization(a), Self::Serialization(b)) => a == b,
            // Struct variants: compare all fields.
            (
                Self::BackendError {
                    backend: b1,
                    operation: o1,
                    detail: d1,
                },
                Self::BackendError {
                    backend: b2,
                    operation: o2,
                    detail: d2,
                },
            ) => b1 == b2 && o1 == o2 && d1 == d2,
            (
                Self::TimeoutError {
                    service: s1,
                    timeout_secs: t1,
                },
                Self::TimeoutError {
                    service: s2,
                    timeout_secs: t2,
                },
            ) => s1 == s2 && t1 == t2,
            // Different variant discriminants are never equal.
            _ => false,
        }
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
        assert!(Error::BackendError {
            backend: "test".to_owned(),
            operation: "op".to_owned(),
            detail: "x".to_owned(),
        }
        .is_retryable());
        assert!(
            Error::TimeoutError {
                service: "x".to_owned(),
                timeout_secs: 1,
            }
            .is_retryable()
        );
        assert!(!Error::ServiceNotFound("x".to_owned()).is_retryable());
        assert!(!Error::ConfigError("x".to_owned()).is_retryable());
    }

    #[test]
    fn backend_error_display_format() {
        let err = Error::BackendError {
            backend: "systemd".to_owned(),
            operation: "install".to_owned(),
            detail: "unit file write denied".to_owned(),
        };
        assert_eq!(
            err.to_string(),
            "systemd: install failed: unit file write denied"
        );
    }

    #[test]
    fn backend_error_fields_accessible() {
        let err = Error::BackendError {
            backend: "launchd".to_owned(),
            operation: "start".to_owned(),
            detail: "bootstrap rejected".to_owned(),
        };
        match &err {
            Error::BackendError {
                backend,
                operation,
                detail,
            } => {
                assert_eq!(backend, "launchd");
                assert_eq!(operation, "start");
                assert_eq!(detail, "bootstrap rejected");
            }
            _ => panic!("expected BackendError variant"),
        }
    }

    #[test]
    fn matching_variants_equal() {
        assert_eq!(
            Error::ServiceNotFound("x".to_owned()),
            Error::ServiceNotFound("x".to_owned()),
        );
        assert_eq!(
            Error::ConfigError("cfg".to_owned()),
            Error::ConfigError("cfg".to_owned()),
        );
        // Io errors with the same ErrorKind are equal
        assert_eq!(
            Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "a")),
            Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "b")),
        );
    }

    #[test]
    fn different_variants_not_equal() {
        assert_ne!(
            Error::ServiceNotFound("x".to_owned()),
            Error::ConfigError("x".to_owned()),
        );
        assert_ne!(
            Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "a")),
            Error::Io(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "a")),
        );
        assert_ne!(
            Error::ServiceNotFound("x".to_owned()),
            Error::Io(std::io::Error::new(std::io::ErrorKind::Other, "x")),
        );
    }

    #[test]
    fn error_display_service_already_exists() {
        let err = Error::ServiceAlreadyExists("myapp".to_owned());
        assert_eq!(err.to_string(), "service already exists: myapp");
    }

    #[test]
    fn error_display_backend_unavailable() {
        let err = Error::BackendUnavailable("supervisord".to_owned());
        assert_eq!(err.to_string(), "backend unavailable: supervisord");
    }

    #[test]
    fn error_display_config_error() {
        let err = Error::ConfigError("bad yaml".to_owned());
        assert_eq!(err.to_string(), "config error: bad yaml");
    }

    #[test]
    fn error_display_dependency_error() {
        let err = Error::DependencyError("missing dep X".to_owned());
        assert_eq!(err.to_string(), "dependency error: missing dep X");
    }

    #[test]
    fn error_display_health_check_failed() {
        let err = Error::HealthCheckFailed("tcp timeout".to_owned());
        assert_eq!(err.to_string(), "health check failed: tcp timeout");
    }

    #[test]
    fn error_display_io() {
        let err = Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert!(err.to_string().starts_with("io error:"));
        assert!(err.to_string().contains("gone"));
    }

    #[test]
    fn error_display_serialization() {
        let err = Error::Serialization("bad json".to_owned());
        assert_eq!(err.to_string(), "serialization error: bad json");
    }

    #[test]
    fn is_retryable_exhaustive_false_cases() {
        assert!(!Error::ServiceAlreadyExists("x".to_owned()).is_retryable());
        assert!(!Error::BackendUnavailable("x".to_owned()).is_retryable());
        assert!(!Error::DependencyError("x".to_owned()).is_retryable());
        assert!(!Error::HealthCheckFailed("x".to_owned()).is_retryable());
        assert!(!Error::Serialization("x".to_owned()).is_retryable());
    }

    #[test]
    fn partial_eq_same_payload_all_string_variants() {
        let pairs: Vec<(Error, Error)> = vec![
            (
                Error::ServiceAlreadyExists("a".to_owned()),
                Error::ServiceAlreadyExists("a".to_owned()),
            ),
            (
                Error::BackendUnavailable("b".to_owned()),
                Error::BackendUnavailable("b".to_owned()),
            ),
            (
                Error::DependencyError("c".to_owned()),
                Error::DependencyError("c".to_owned()),
            ),
            (
                Error::HealthCheckFailed("d".to_owned()),
                Error::HealthCheckFailed("d".to_owned()),
            ),
            (
                Error::Serialization("e".to_owned()),
                Error::Serialization("e".to_owned()),
            ),
        ];
        for (a, b) in &pairs {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn partial_eq_different_payload_not_equal() {
        assert_ne!(
            Error::ServiceAlreadyExists("a".to_owned()),
            Error::ServiceAlreadyExists("b".to_owned()),
        );
        assert_ne!(
            Error::Serialization("x".to_owned()),
            Error::Serialization("y".to_owned()),
        );
    }

    #[test]
    fn partial_eq_timeout_different_fields() {
        assert_ne!(
            Error::TimeoutError {
                service: "a".to_owned(),
                timeout_secs: 10,
            },
            Error::TimeoutError {
                service: "a".to_owned(),
                timeout_secs: 20,
            },
        );
        assert_ne!(
            Error::TimeoutError {
                service: "a".to_owned(),
                timeout_secs: 10,
            },
            Error::TimeoutError {
                service: "b".to_owned(),
                timeout_secs: 10,
            },
        );
    }

    #[test]
    fn partial_eq_backend_error_different_fields() {
        let base = Error::BackendError {
            backend: "systemd".to_owned(),
            operation: "start".to_owned(),
            detail: "fail".to_owned(),
        };
        let diff_backend = Error::BackendError {
            backend: "launchd".to_owned(),
            operation: "start".to_owned(),
            detail: "fail".to_owned(),
        };
        let diff_op = Error::BackendError {
            backend: "systemd".to_owned(),
            operation: "stop".to_owned(),
            detail: "fail".to_owned(),
        };
        let diff_detail = Error::BackendError {
            backend: "systemd".to_owned(),
            operation: "start".to_owned(),
            detail: "other".to_owned(),
        };
        assert_ne!(base, diff_backend);
        assert_ne!(base, diff_op);
        assert_ne!(base, diff_detail);
    }
}
