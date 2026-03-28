use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Logging configuration for a service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSpec {
    /// Where to send stdout output.
    #[serde(default)]
    pub stdout: LogTarget,

    /// Where to send stderr output.
    #[serde(default)]
    pub stderr: LogTarget,
}

impl Default for LoggingSpec {
    fn default() -> Self {
        Self {
            stdout: LogTarget::Journal,
            stderr: LogTarget::Journal,
        }
    }
}

/// Where to direct log output.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogTarget {
    /// Send to the system journal (journald on Linux, os_log on macOS).
    #[default]
    Journal,
    /// Write to a file at the given path.
    File(PathBuf),
    /// Discard output.
    Null,
    /// Inherit the parent process's file descriptor.
    Inherit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_journal() {
        let logging = LoggingSpec::default();
        assert!(matches!(logging.stdout, LogTarget::Journal));
        assert!(matches!(logging.stderr, LogTarget::Journal));
    }

    #[test]
    fn file_target_from_yaml() {
        // serde_yaml_ng uses YAML tags for externally-tagged enums with data.
        let yaml = "
stdout: !file /var/log/myapp/stdout.log
stderr: !file /var/log/myapp/stderr.log
";
        let logging: LoggingSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        match &logging.stdout {
            LogTarget::File(p) => assert_eq!(p, &PathBuf::from("/var/log/myapp/stdout.log")),
            other => panic!("expected File, got {other:?}"),
        }
        match &logging.stderr {
            LogTarget::File(p) => assert_eq!(p, &PathBuf::from("/var/log/myapp/stderr.log")),
            other => panic!("expected File, got {other:?}"),
        }
    }

    #[test]
    fn null_and_inherit_targets() {
        let yaml = r"
stdout: null
stderr: inherit
";
        let logging: LoggingSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert!(matches!(logging.stdout, LogTarget::Null));
        assert!(matches!(logging.stderr, LogTarget::Inherit));
    }

    #[test]
    fn roundtrip() {
        let logging = LoggingSpec {
            stdout: LogTarget::File(PathBuf::from("/tmp/out.log")),
            stderr: LogTarget::Null,
        };
        let yaml = serde_yaml_ng::to_string(&logging).expect("serialize");
        let parsed: LoggingSpec = serde_yaml_ng::from_str(&yaml).expect("deserialize");
        assert!(matches!(parsed.stdout, LogTarget::File(_)));
        assert!(matches!(parsed.stderr, LogTarget::Null));
    }
}
