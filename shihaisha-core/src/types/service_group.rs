use serde::{Deserialize, Serialize};
use std::fmt;

/// Strategy for handling failures within a service group.
///
/// Inspired by Erlang OTP supervision trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GroupRestartStrategy {
    /// Restart only the failed service (default).
    #[default]
    OneForOne,
    /// Restart all services in the group when one fails.
    OneForAll,
    /// Restart the failed service and all services started after it.
    RestForOne,
}

impl fmt::Display for GroupRestartStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OneForOne => write!(f, "one_for_one"),
            Self::OneForAll => write!(f, "one_for_all"),
            Self::RestForOne => write!(f, "rest_for_one"),
        }
    }
}

fn default_max_intensity() -> u32 {
    5
}

fn default_intensity_period() -> u64 {
    60
}

/// A named group of related services with shared restart behavior.
///
/// Members are ordered -- order matters for `RestForOne` strategy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceGroup {
    /// Group name.
    pub name: String,
    /// Member service names (order matters for `RestForOne`).
    pub members: Vec<String>,
    /// Restart strategy when a member fails.
    #[serde(default)]
    pub strategy: GroupRestartStrategy,
    /// Max restarts within the period before the group itself fails.
    #[serde(default = "default_max_intensity")]
    pub max_intensity: u32,
    /// Period in seconds for counting restarts.
    #[serde(default = "default_intensity_period")]
    pub intensity_period_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_restart_strategy_default_is_one_for_one() {
        assert_eq!(
            GroupRestartStrategy::default(),
            GroupRestartStrategy::OneForOne,
        );
    }

    #[test]
    fn group_restart_strategy_serializes_snake_case() {
        let json =
            serde_json::to_string(&GroupRestartStrategy::OneForAll).expect("serialize");
        assert_eq!(json, "\"one_for_all\"");

        let parsed: GroupRestartStrategy =
            serde_json::from_str("\"rest_for_one\"").expect("parse");
        assert_eq!(parsed, GroupRestartStrategy::RestForOne);
    }

    #[test]
    fn group_restart_strategy_display() {
        assert_eq!(
            GroupRestartStrategy::OneForOne.to_string(),
            "one_for_one",
        );
        assert_eq!(
            GroupRestartStrategy::OneForAll.to_string(),
            "one_for_all",
        );
        assert_eq!(
            GroupRestartStrategy::RestForOne.to_string(),
            "rest_for_one",
        );
    }

    #[test]
    fn service_group_yaml_roundtrip() {
        let group = ServiceGroup {
            name: "db-group".to_owned(),
            members: vec!["postgres".to_owned(), "pgbouncer".to_owned()],
            strategy: GroupRestartStrategy::OneForAll,
            max_intensity: 10,
            intensity_period_secs: 120,
        };

        let yaml = serde_yaml_ng::to_string(&group).expect("serialize");
        let parsed: ServiceGroup = serde_yaml_ng::from_str(&yaml).expect("deserialize");

        assert_eq!(parsed.name, "db-group");
        assert_eq!(parsed.members.len(), 2);
        assert_eq!(parsed.strategy, GroupRestartStrategy::OneForAll);
        assert_eq!(parsed.max_intensity, 10);
        assert_eq!(parsed.intensity_period_secs, 120);
    }

    #[test]
    fn service_group_defaults() {
        let yaml = r#"
name: test-group
members:
  - svc-a
  - svc-b
"#;
        let group: ServiceGroup = serde_yaml_ng::from_str(yaml).expect("parse");
        assert_eq!(group.strategy, GroupRestartStrategy::OneForOne);
        assert_eq!(group.max_intensity, 5);
        assert_eq!(group.intensity_period_secs, 60);
    }
}
