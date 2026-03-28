use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Backend-specific overrides (escape hatch for features not in the canonical spec).
///
/// These allow users to inject raw directives into the backend-native config
/// when the canonical `ServiceSpec` does not cover a specific feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BackendOverrides {
    /// Raw systemd unit directives to add, keyed by section
    /// (e.g., `{"Service": {"LimitNOFILE": "65536"}}`).
    #[serde(default)]
    pub systemd: HashMap<String, HashMap<String, String>>,

    /// Raw launchd plist keys to add.
    #[serde(default)]
    pub launchd: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let overrides = BackendOverrides::default();
        assert!(overrides.systemd.is_empty());
        assert!(overrides.launchd.is_empty());
    }

    #[test]
    fn systemd_overrides_from_yaml() {
        let yaml = r#"
systemd:
  Service:
    LimitNOFILE: "65536"
    ProtectHome: "yes"
  Unit:
    Documentation: "man:myapp(1)"
launchd: {}
"#;
        let overrides: BackendOverrides = serde_yaml_ng::from_str(yaml).expect("parse");
        let service = overrides.systemd.get("Service").expect("Service section");
        assert_eq!(service.get("LimitNOFILE").unwrap(), "65536");
        assert_eq!(service.get("ProtectHome").unwrap(), "yes");
    }

    #[test]
    fn launchd_overrides_from_yaml() {
        let yaml = r#"
systemd: {}
launchd:
  ThrottleInterval: 10
  Nice: -5
  ProcessType: Interactive
"#;
        let overrides: BackendOverrides = serde_yaml_ng::from_str(yaml).expect("parse");
        assert_eq!(overrides.launchd.get("ThrottleInterval").unwrap(), 10);
        assert_eq!(overrides.launchd.get("Nice").unwrap(), -5);
        assert_eq!(
            overrides.launchd.get("ProcessType").unwrap(),
            "Interactive"
        );
    }
}
