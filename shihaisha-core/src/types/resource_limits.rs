use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------
// MemorySize newtype
// ---------------------------------------------------------------------------

/// Memory size with human-readable format (e.g., "512M", "2G") or raw bytes.
///
/// Serializes to the original string representation (e.g. `"512M"`).
/// Deserializes from either a string like `"512M"` or a raw integer (bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySize {
    bytes: u64,
    /// Preserve the original string for lossless serialization roundtrips.
    display: String,
}

impl MemorySize {
    /// Create from a raw byte count.
    #[must_use]
    pub fn from_bytes(bytes: u64) -> Self {
        Self {
            bytes,
            display: bytes.to_string(),
        }
    }

    /// Parse a human-readable memory string (e.g. `"512M"`, `"2G"`, `"1024K"`,
    /// `"1T"`) or a plain number of bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        if s.is_empty() {
            return Err(Error::ConfigError("empty memory size string".to_owned()));
        }

        let (num_str, multiplier) = if let Some(n) = s.strip_suffix('T') {
            (n, 1024_u64 * 1024 * 1024 * 1024)
        } else if let Some(n) = s.strip_suffix('G') {
            (n, 1024_u64 * 1024 * 1024)
        } else if let Some(n) = s.strip_suffix('M') {
            (n, 1024_u64 * 1024)
        } else if let Some(n) = s.strip_suffix('K') {
            (n, 1024_u64)
        } else {
            (s, 1_u64)
        };

        let num: u64 = num_str
            .trim()
            .parse()
            .map_err(|_| Error::ConfigError(format!("invalid memory size: {s}")))?;

        Ok(Self {
            bytes: num
                .checked_mul(multiplier)
                .ok_or_else(|| Error::ConfigError(format!("memory size overflow: {s}")))?,
            display: s.to_owned(),
        })
    }

    /// Return the size in bytes.
    #[must_use]
    pub fn as_bytes(&self) -> u64 {
        self.bytes
    }
}

impl fmt::Display for MemorySize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display)
    }
}

impl FromStr for MemorySize {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

impl Serialize for MemorySize {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.display)
    }
}

impl<'de> Deserialize<'de> for MemorySize {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct MemorySizeVisitor;

        impl serde::de::Visitor<'_> for MemorySizeVisitor {
            type Value = MemorySize;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a memory size string (e.g. \"512M\") or integer (bytes)")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<MemorySize, E> {
                MemorySize::parse(v).map_err(E::custom)
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<MemorySize, E> {
                Ok(MemorySize::from_bytes(v))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<MemorySize, E> {
                if v < 0 {
                    return Err(E::custom("memory size cannot be negative"));
                }
                Ok(MemorySize::from_bytes(u64::try_from(v).map_err(|_| E::custom("memory size cannot be negative"))?))
            }
        }

        deserializer.deserialize_any(MemorySizeVisitor)
    }
}

// ---------------------------------------------------------------------------
// Weight newtype
// ---------------------------------------------------------------------------

/// Weight value constrained to 1..=10000 (systemd `CPUWeight=` / `IOWeight=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Weight(u64);

impl Weight {
    /// Create a new `Weight`, validating the range.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not in `1..=10000`.
    pub fn new(v: u64) -> Result<Self> {
        if (1..=10000).contains(&v) {
            Ok(Self(v))
        } else {
            Err(Error::ConfigError(format!(
                "weight must be 1-10000, got {v}"
            )))
        }
    }

    /// Return the inner value.
    #[must_use]
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl fmt::Display for Weight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<u64> for Weight {
    type Error = Error;

    fn try_from(v: u64) -> Result<Self> {
        Self::new(v)
    }
}

impl Serialize for Weight {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for Weight {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let v = u64::deserialize(deserializer)?;
        Weight::new(v).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// NiceValue newtype
// ---------------------------------------------------------------------------

/// Nice value constrained to -20..=19.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NiceValue(i32);

impl NiceValue {
    /// Create a new `NiceValue`, validating the range.
    ///
    /// # Errors
    ///
    /// Returns an error if the value is not in `-20..=19`.
    pub fn new(v: i32) -> Result<Self> {
        if (-20..=19).contains(&v) {
            Ok(Self(v))
        } else {
            Err(Error::ConfigError(format!(
                "nice must be -20..19, got {v}"
            )))
        }
    }

    /// Return the inner value.
    #[must_use]
    pub fn value(&self) -> i32 {
        self.0
    }
}

impl fmt::Display for NiceValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<i32> for NiceValue {
    type Error = Error;

    fn try_from(v: i32) -> Result<Self> {
        Self::new(v)
    }
}

impl Serialize for NiceValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_i32(self.0)
    }
}

impl<'de> Deserialize<'de> for NiceValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let v = i32::deserialize(deserializer)?;
        NiceValue::new(v).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// ResourceLimits
// ---------------------------------------------------------------------------

/// Resource limits for a service (maps to systemd cgroup directives / launchd
/// `HardResourceLimits`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ResourceLimits {
    /// Maximum memory (e.g., `"512M"`, `"1G"`). Maps to `MemoryMax=`.
    #[serde(default)]
    pub memory_max: Option<MemorySize>,

    /// Memory high watermark (throttling threshold). Maps to `MemoryHigh=`.
    #[serde(default)]
    pub memory_high: Option<MemorySize>,

    /// CPU weight (1-10000). Maps to `CPUWeight=`.
    #[serde(default)]
    pub cpu_weight: Option<Weight>,

    /// CPU quota (e.g., `"50%"` or `"200%"` for multi-core). Maps to `CPUQuota=`.
    #[serde(default)]
    pub cpu_quota: Option<String>,

    /// Maximum number of tasks/threads. Maps to `TasksMax=`.
    #[serde(default)]
    pub tasks_max: Option<u64>,

    /// I/O weight (1-10000). Maps to `IOWeight=`.
    #[serde(default)]
    pub io_weight: Option<Weight>,

    /// Nice value (-20 to 19). Maps to `Nice=`.
    #[serde(default)]
    pub nice: Option<NiceValue>,
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
        assert_eq!(limits.memory_max.as_ref().map(|m| m.to_string()).as_deref(), Some("512M"));
        assert_eq!(limits.cpu_quota.as_deref(), Some("50%"));
        assert_eq!(limits.nice.map(|n| n.value()), Some(-5));
        assert!(limits.memory_high.is_none());
        assert!(limits.cpu_weight.is_none());
    }

    #[test]
    fn full_roundtrip() {
        let limits = ResourceLimits {
            memory_max: Some(MemorySize::parse("1G").unwrap()),
            memory_high: Some(MemorySize::parse("768M").unwrap()),
            cpu_weight: Some(Weight::new(500).unwrap()),
            cpu_quota: Some("200%".to_owned()),
            tasks_max: Some(1024),
            io_weight: Some(Weight::new(100).unwrap()),
            nice: Some(NiceValue::new(10).unwrap()),
        };
        let yaml = serde_yaml_ng::to_string(&limits).expect("serialize");
        let parsed: ResourceLimits = serde_yaml_ng::from_str(&yaml).expect("deserialize");
        assert_eq!(parsed.memory_max.as_ref().map(|m| m.to_string()).as_deref(), Some("1G"));
        assert_eq!(parsed.cpu_weight.map(|w| w.value()), Some(500));
        assert_eq!(parsed.tasks_max, Some(1024));
    }

    // --- MemorySize tests ---

    #[test]
    fn parse_memory_512m() {
        let m = MemorySize::parse("512M").unwrap();
        assert_eq!(m.as_bytes(), 512 * 1024 * 1024);
    }

    #[test]
    fn parse_memory_2g() {
        let m = MemorySize::parse("2G").unwrap();
        assert_eq!(m.as_bytes(), 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_memory_1t() {
        let m = MemorySize::parse("1T").unwrap();
        assert_eq!(m.as_bytes(), 1024_u64 * 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_memory_1024k() {
        let m = MemorySize::parse("1024K").unwrap();
        assert_eq!(m.as_bytes(), 1024 * 1024);
    }

    #[test]
    fn parse_memory_invalid() {
        assert!(MemorySize::parse("abc").is_err());
        assert!(MemorySize::parse("").is_err());
    }

    #[test]
    fn memory_size_serde_roundtrip() {
        let m = MemorySize::parse("512M").unwrap();
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, "\"512M\"");
        let parsed: MemorySize = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_bytes(), m.as_bytes());
    }

    #[test]
    fn memory_size_deserialize_from_integer() {
        let parsed: MemorySize = serde_json::from_str("1048576").unwrap();
        assert_eq!(parsed.as_bytes(), 1_048_576);
    }

    // --- Weight tests ---

    #[test]
    fn weight_valid() {
        assert!(Weight::new(1).is_ok());
        assert!(Weight::new(5000).is_ok());
        assert!(Weight::new(10000).is_ok());
    }

    #[test]
    fn weight_invalid() {
        assert!(Weight::new(0).is_err());
        assert!(Weight::new(10001).is_err());
    }

    #[test]
    fn weight_serde_roundtrip() {
        let w = Weight::new(500).unwrap();
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(json, "500");
        let parsed: Weight = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.value(), 500);
    }

    // --- NiceValue tests ---

    #[test]
    fn nice_valid() {
        assert!(NiceValue::new(-20).is_ok());
        assert!(NiceValue::new(0).is_ok());
        assert!(NiceValue::new(19).is_ok());
    }

    #[test]
    fn nice_invalid() {
        assert!(NiceValue::new(-21).is_err());
        assert!(NiceValue::new(20).is_err());
    }

    #[test]
    fn nice_serde_roundtrip() {
        let n = NiceValue::new(-5).unwrap();
        let json = serde_json::to_string(&n).unwrap();
        assert_eq!(json, "-5");
        let parsed: NiceValue = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.value(), -5);
    }

    #[test]
    fn memory_size_overflow_error() {
        let result = MemorySize::parse("999999999999T");
        assert!(result.is_err(), "should fail on overflow");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("overflow"), "error: {err}");
    }

    #[test]
    fn memory_size_deserialize_negative_i64_rejected() {
        let result: std::result::Result<MemorySize, _> = serde_json::from_str("-100");
        assert!(result.is_err(), "negative memory should be rejected");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("negative"), "error: {err}");
    }

    #[test]
    fn memory_size_display() {
        let m = MemorySize::parse("512M").unwrap();
        assert_eq!(m.to_string(), "512M");

        let m = MemorySize::from_bytes(1024);
        assert_eq!(m.to_string(), "1024");
    }

    #[test]
    fn weight_display() {
        let w = Weight::new(100).unwrap();
        assert_eq!(w.to_string(), "100");

        let w = Weight::new(10000).unwrap();
        assert_eq!(w.to_string(), "10000");
    }

    #[test]
    fn nice_value_display() {
        let n = NiceValue::new(-20).unwrap();
        assert_eq!(n.to_string(), "-20");

        let n = NiceValue::new(0).unwrap();
        assert_eq!(n.to_string(), "0");

        let n = NiceValue::new(19).unwrap();
        assert_eq!(n.to_string(), "19");
    }

    #[test]
    fn memory_size_from_bytes_preserves_value() {
        let m = MemorySize::from_bytes(0);
        assert_eq!(m.as_bytes(), 0);

        let m = MemorySize::from_bytes(u64::MAX);
        assert_eq!(m.as_bytes(), u64::MAX);
    }

    #[test]
    fn memory_size_parse_plain_number() {
        let m = MemorySize::parse("4096").unwrap();
        assert_eq!(m.as_bytes(), 4096);
    }

    #[test]
    fn memory_size_parse_whitespace_trimmed() {
        let m = MemorySize::parse("  512M  ").unwrap();
        assert_eq!(m.as_bytes(), 512 * 1024 * 1024);
    }

    #[test]
    fn weight_boundary_values() {
        assert!(Weight::new(1).is_ok());
        assert!(Weight::new(10000).is_ok());
        assert!(Weight::new(0).is_err());
        assert!(Weight::new(10001).is_err());
        assert!(Weight::new(u64::MAX).is_err());
    }

    #[test]
    fn nice_boundary_values() {
        assert!(NiceValue::new(-20).is_ok());
        assert!(NiceValue::new(19).is_ok());
        assert!(NiceValue::new(-21).is_err());
        assert!(NiceValue::new(20).is_err());
        assert!(NiceValue::new(i32::MAX).is_err());
        assert!(NiceValue::new(i32::MIN).is_err());
    }

    #[test]
    fn weight_deserialize_invalid_rejected() {
        let result: std::result::Result<Weight, _> = serde_json::from_str("0");
        assert!(result.is_err());

        let result: std::result::Result<Weight, _> = serde_json::from_str("10001");
        assert!(result.is_err());
    }

    #[test]
    fn nice_deserialize_invalid_rejected() {
        let result: std::result::Result<NiceValue, _> = serde_json::from_str("-21");
        assert!(result.is_err());

        let result: std::result::Result<NiceValue, _> = serde_json::from_str("20");
        assert!(result.is_err());
    }

    #[test]
    fn memory_size_fromstr_roundtrip() {
        for input in ["512M", "2G", "1T", "1024K", "4096"] {
            let m: MemorySize = input.parse().expect("parse");
            assert_eq!(m.to_string(), input);
        }
    }

    #[test]
    fn memory_size_fromstr_invalid() {
        let result: Result<MemorySize> = "abc".parse();
        assert!(result.is_err());
    }

    #[test]
    fn weight_try_from_u64() {
        let w: Weight = 500u64.try_into().expect("valid weight");
        assert_eq!(w.value(), 500);

        let result: Result<Weight> = 0u64.try_into();
        assert!(result.is_err());
    }

    #[test]
    fn nice_value_try_from_i32() {
        let n: NiceValue = (-5i32).try_into().expect("valid nice");
        assert_eq!(n.value(), -5);

        let result: Result<NiceValue> = 20i32.try_into();
        assert!(result.is_err());
    }
}
