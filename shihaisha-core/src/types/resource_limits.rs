use serde::{Deserialize, Serialize};

/// Resource limits for a service (maps to systemd cgroup directives / launchd
/// `HardResourceLimits`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceLimits {
    /// Maximum memory (e.g., `"512M"`, `"1G"`). Maps to `MemoryMax=`.
    #[serde(default)]
    pub memory_max: Option<String>,

    /// Memory high watermark (throttling threshold). Maps to `MemoryHigh=`.
    #[serde(default)]
    pub memory_high: Option<String>,

    /// CPU weight (1-10000). Maps to `CPUWeight=`.
    #[serde(default)]
    pub cpu_weight: Option<u64>,

    /// CPU quota (e.g., `"50%"` or `"200%"` for multi-core). Maps to `CPUQuota=`.
    #[serde(default)]
    pub cpu_quota: Option<String>,

    /// Maximum number of tasks/threads. Maps to `TasksMax=`.
    #[serde(default)]
    pub tasks_max: Option<u64>,

    /// I/O weight (1-10000). Maps to `IOWeight=`.
    #[serde(default)]
    pub io_weight: Option<u64>,

    /// Nice value (-20 to 19). Maps to `Nice=`.
    #[serde(default)]
    pub nice: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_none() {
        let limits = ResourceLimits::default();
        assert!(limits.memory_max.is_none());
        assert!(limits.memory_high.is_none());
        assert!(limits.cpu_weight.is_none());
        assert!(limits.cpu_quota.is_none());
        assert!(limits.tasks_max.is_none());
        assert!(limits.io_weight.is_none());
        assert!(limits.nice.is_none());
    }

    #[test]
    fn partial_yaml_deserializes() {
        let yaml = r"
memory_max: 512M
cpu_quota: '50%'
nice: -5
";
        let limits: ResourceLimits = serde_yaml_ng::from_str(yaml).expect("parse");
        assert_eq!(limits.memory_max.as_deref(), Some("512M"));
        assert_eq!(limits.cpu_quota.as_deref(), Some("50%"));
        assert_eq!(limits.nice, Some(-5));
        assert!(limits.memory_high.is_none());
        assert!(limits.cpu_weight.is_none());
    }

    #[test]
    fn full_roundtrip() {
        let limits = ResourceLimits {
            memory_max: Some("1G".to_owned()),
            memory_high: Some("768M".to_owned()),
            cpu_weight: Some(500),
            cpu_quota: Some("200%".to_owned()),
            tasks_max: Some(1024),
            io_weight: Some(100),
            nice: Some(10),
        };
        let yaml = serde_yaml_ng::to_string(&limits).expect("serialize");
        let parsed: ResourceLimits = serde_yaml_ng::from_str(&yaml).expect("deserialize");
        assert_eq!(parsed.memory_max.as_deref(), Some("1G"));
        assert_eq!(parsed.cpu_weight, Some(500));
        assert_eq!(parsed.tasks_max, Some(1024));
    }
}
