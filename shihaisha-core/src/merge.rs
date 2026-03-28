//! Per-type merge semantics following NixOS module system patterns.
//!
//! The merge system provides principled layering of `ServiceSpec` values:
//!
//! - **Scalars** (strings, enums, integers, booleans): overlay wins unconditionally.
//! - **`Option<T>`**: overlay `Some` wins; `None` falls through to base.
//! - **`Vec<T>`**: concatenate (deduplicated by value).
//! - **`HashMap<K,V>`**: recursive merge by key, overlay key wins on conflict.
//!
//! This enables a profile/override stacking model where a base spec provides
//! defaults and overlays progressively specialize, exactly like NixOS modules
//! composed with `lib.mkMerge`.

use std::collections::HashMap;
use std::hash::Hash;

use crate::types::backend_overrides::BackendOverrides;
use crate::types::logging::LoggingSpec;
use crate::types::resource_limits::ResourceLimits;
use crate::types::service_spec::{DependencySpec, RestartPolicy, ServiceSpec};

/// Per-type merge semantics.
///
/// Implementors define how two values of the same type are combined, with
/// `overlay` taking precedence over `base` where semantics allow.
pub trait Merge {
    /// Merge `base` and `overlay`, returning the combined value.
    ///
    /// The overlay takes precedence for scalar fields.  Collection fields
    /// are combined according to the type's natural merge strategy.
    #[must_use]
    fn merge(base: &Self, overlay: &Self) -> Self;
}

// ---------------------------------------------------------------------------
// Primitive / scalar merges
// ---------------------------------------------------------------------------

/// Merge two `Option<T>` values: overlay `Some` wins, otherwise base.
#[must_use]
fn merge_option<T: Clone>(base: &Option<T>, overlay: &Option<T>) -> Option<T> {
    overlay.as_ref().or(base.as_ref()).cloned()
}

/// Merge two `Vec<T>` values: concatenate, deduplicate preserving order.
#[must_use]
fn merge_vec_dedup<T: Clone + Eq + Hash>(base: &[T], overlay: &[T]) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for item in base.iter().chain(overlay.iter()) {
        if seen.insert(item) {
            result.push(item.clone());
        }
    }
    result
}

/// Merge two `HashMap<String, String>` maps: overlay keys win on conflict.
#[must_use]
fn merge_string_map(
    base: &HashMap<String, String>,
    overlay: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut result = base.clone();
    for (k, v) in overlay {
        result.insert(k.clone(), v.clone());
    }
    result
}

/// Merge two `HashMap<String, HashMap<String, String>>` maps recursively.
#[must_use]
fn merge_nested_string_map(
    base: &HashMap<String, HashMap<String, String>>,
    overlay: &HashMap<String, HashMap<String, String>>,
) -> HashMap<String, HashMap<String, String>> {
    let mut result = base.clone();
    for (section, overlay_entries) in overlay {
        let merged_section = result
            .entry(section.clone())
            .or_default();
        for (k, v) in overlay_entries {
            merged_section.insert(k.clone(), v.clone());
        }
    }
    result
}

/// Merge two `HashMap<String, serde_json::Value>` maps: overlay keys win.
#[must_use]
fn merge_json_map(
    base: &HashMap<String, serde_json::Value>,
    overlay: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    let mut result = base.clone();
    for (k, v) in overlay {
        result.insert(k.clone(), v.clone());
    }
    result
}

// ---------------------------------------------------------------------------
// Merge implementations
// ---------------------------------------------------------------------------

impl Merge for RestartPolicy {
    /// Scalar override: the overlay policy wins entirely.
    fn merge(_base: &Self, overlay: &Self) -> Self {
        overlay.clone()
    }
}

impl Merge for DependencySpec {
    /// Vec fields concatenate and deduplicate.
    fn merge(base: &Self, overlay: &Self) -> Self {
        Self {
            after: merge_vec_dedup(&base.after, &overlay.after),
            before: merge_vec_dedup(&base.before, &overlay.before),
            requires: merge_vec_dedup(&base.requires, &overlay.requires),
            wants: merge_vec_dedup(&base.wants, &overlay.wants),
            conflicts: merge_vec_dedup(&base.conflicts, &overlay.conflicts),
        }
    }
}

impl Merge for ResourceLimits {
    /// Option fields: overlay wins if `Some`, falls through otherwise.
    fn merge(base: &Self, overlay: &Self) -> Self {
        Self {
            memory_max: merge_option(&base.memory_max, &overlay.memory_max),
            memory_high: merge_option(&base.memory_high, &overlay.memory_high),
            cpu_weight: merge_option(&base.cpu_weight, &overlay.cpu_weight),
            cpu_quota: merge_option(&base.cpu_quota, &overlay.cpu_quota),
            tasks_max: merge_option(&base.tasks_max, &overlay.tasks_max),
            io_weight: merge_option(&base.io_weight, &overlay.io_weight),
            nice: merge_option(&base.nice, &overlay.nice),
        }
    }
}

impl Merge for LoggingSpec {
    /// Scalar override: the overlay logging spec wins entirely.
    fn merge(_base: &Self, overlay: &Self) -> Self {
        overlay.clone()
    }
}

impl Merge for BackendOverrides {
    /// Recursive map merge: overlay keys win at each level.
    fn merge(base: &Self, overlay: &Self) -> Self {
        Self {
            systemd: merge_nested_string_map(&base.systemd, &overlay.systemd),
            launchd: merge_json_map(&base.launchd, &overlay.launchd),
        }
    }
}

impl Merge for ServiceSpec {
    /// Field-by-field merge of the canonical service specification.
    ///
    /// Scalars: overlay wins.
    /// Options: overlay `Some` wins.
    /// Vecs: concatenate + deduplicate.
    /// Maps: merge by key.
    /// Nested structs: delegate to their `Merge` impl.
    fn merge(base: &Self, overlay: &Self) -> Self {
        Self {
            // Scalars: overlay wins
            name: overlay.name.clone(),
            description: if overlay.description.is_empty() {
                base.description.clone()
            } else {
                overlay.description.clone()
            },
            command: overlay.command.clone(),
            service_type: overlay.service_type,
            notify: overlay.notify,
            watchdog_sec: overlay.watchdog_sec,
            timeout_start_sec: overlay.timeout_start_sec,
            timeout_stop_sec: overlay.timeout_stop_sec,

            // Vec: concatenate + dedup
            args: merge_vec_dedup(&base.args, &overlay.args),
            sockets: {
                // Sockets don't implement Hash/Eq, so concat without dedup
                let mut combined = base.sockets.clone();
                combined.extend(overlay.sockets.iter().cloned());
                combined
            },

            // Option: overlay wins if Some
            working_directory: merge_option(&base.working_directory, &overlay.working_directory),
            user: merge_option(&base.user, &overlay.user),
            group: merge_option(&base.group, &overlay.group),
            health: merge_option(&base.health, &overlay.health),
            resources: match (&base.resources, &overlay.resources) {
                (Some(b), Some(o)) => Some(ResourceLimits::merge(b, o)),
                (_, Some(o)) => Some(o.clone()),
                (Some(b), None) => Some(b.clone()),
                (None, None) => None,
            },

            // Map: merge by key
            environment: merge_string_map(&base.environment, &overlay.environment),

            // Nested structs: delegate
            restart: RestartPolicy::merge(&base.restart, &overlay.restart),
            depends_on: DependencySpec::merge(&base.depends_on, &overlay.depends_on),
            logging: LoggingSpec::merge(&base.logging, &overlay.logging),
            overrides: BackendOverrides::merge(&base.overrides, &overlay.overrides),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::service_spec::{RestartStrategy, ServiceType};
    use std::path::PathBuf;

    fn base_spec() -> ServiceSpec {
        let mut spec = ServiceSpec::new("base-svc", "/usr/bin/base");
        spec.description = "Base service".to_owned();
        spec.args = vec!["--verbose".to_owned()];
        spec.working_directory = Some(PathBuf::from("/var/base"));
        spec.user = Some("baseuser".to_owned());
        spec.environment.insert("RUST_LOG".to_owned(), "info".to_owned());
        spec.environment.insert("PORT".to_owned(), "8080".to_owned());
        spec.restart = RestartPolicy {
            strategy: RestartStrategy::OnFailure,
            delay_secs: 5,
            max_retries: 3,
            reset_after_secs: 300,
        };
        spec.depends_on = DependencySpec {
            after: vec!["database".to_owned()],
            before: vec![],
            requires: vec!["database".to_owned()],
            wants: vec![],
            conflicts: vec![],
        };
        spec.resources = Some(ResourceLimits {
            memory_max: Some("512M".to_owned()),
            memory_high: None,
            cpu_weight: None,
            cpu_quota: None,
            tasks_max: Some(256),
            io_weight: None,
            nice: None,
        });
        spec
    }

    fn overlay_spec() -> ServiceSpec {
        let mut spec = ServiceSpec::new("overlay-svc", "/usr/bin/overlay");
        spec.description = "Overlay service".to_owned();
        spec.args = vec!["--debug".to_owned()];
        spec.service_type = ServiceType::Notify;
        spec.group = Some("overlaygroup".to_owned());
        spec.environment.insert("RUST_LOG".to_owned(), "debug".to_owned());
        spec.environment.insert("NEW_VAR".to_owned(), "hello".to_owned());
        spec.restart = RestartPolicy {
            strategy: RestartStrategy::Always,
            delay_secs: 10,
            max_retries: 5,
            reset_after_secs: 600,
        };
        spec.depends_on = DependencySpec {
            after: vec!["cache".to_owned()],
            before: vec![],
            requires: vec![],
            wants: vec!["metrics".to_owned()],
            conflicts: vec![],
        };
        spec.resources = Some(ResourceLimits {
            memory_max: Some("1G".to_owned()),
            memory_high: None,
            cpu_weight: Some(500),
            cpu_quota: None,
            tasks_max: None,
            io_weight: None,
            nice: Some(-5),
        });
        spec.notify = true;
        spec.watchdog_sec = 30;
        spec.timeout_start_sec = 120;
        spec.timeout_stop_sec = 60;
        spec
    }

    #[test]
    fn scalar_override() {
        let base = base_spec();
        let overlay = overlay_spec();
        let merged = ServiceSpec::merge(&base, &overlay);

        assert_eq!(merged.description, "Overlay service");
        assert_eq!(merged.command, "/usr/bin/overlay");
        assert_eq!(merged.service_type, ServiceType::Notify);
        assert!(merged.notify);
        assert_eq!(merged.watchdog_sec, 30);
        assert_eq!(merged.timeout_start_sec, 120);
        assert_eq!(merged.timeout_stop_sec, 60);
    }

    #[test]
    fn option_fallthrough() {
        let base = base_spec();
        let mut overlay = overlay_spec();
        overlay.working_directory = None;
        overlay.user = None;
        let merged = ServiceSpec::merge(&base, &overlay);

        // Base values used when overlay is None
        assert_eq!(merged.working_directory, Some(PathBuf::from("/var/base")));
        assert_eq!(merged.user, Some("baseuser".to_owned()));
    }

    #[test]
    fn option_override() {
        let base = base_spec();
        let mut overlay = overlay_spec();
        overlay.working_directory = Some(PathBuf::from("/var/overlay"));
        let merged = ServiceSpec::merge(&base, &overlay);

        assert_eq!(
            merged.working_directory,
            Some(PathBuf::from("/var/overlay"))
        );
    }

    #[test]
    fn vec_concatenate() {
        let base = base_spec();
        let overlay = overlay_spec();
        let merged = ServiceSpec::merge(&base, &overlay);

        // depends_on.after: ["database"] + ["cache"] = ["database", "cache"]
        assert!(merged.depends_on.after.contains(&"database".to_owned()));
        assert!(merged.depends_on.after.contains(&"cache".to_owned()));
        assert_eq!(merged.depends_on.after.len(), 2);

        // requires: ["database"] + [] = ["database"]
        assert_eq!(merged.depends_on.requires, vec!["database".to_owned()]);

        // wants: [] + ["metrics"] = ["metrics"]
        assert_eq!(merged.depends_on.wants, vec!["metrics".to_owned()]);
    }

    #[test]
    fn vec_deduplicate() {
        let mut base = base_spec();
        base.depends_on.after = vec!["database".to_owned(), "cache".to_owned()];

        let mut overlay = overlay_spec();
        overlay.depends_on.after = vec!["cache".to_owned(), "redis".to_owned()];

        let merged = ServiceSpec::merge(&base, &overlay);

        // Should contain each only once
        assert_eq!(merged.depends_on.after.len(), 3);
        assert!(merged.depends_on.after.contains(&"database".to_owned()));
        assert!(merged.depends_on.after.contains(&"cache".to_owned()));
        assert!(merged.depends_on.after.contains(&"redis".to_owned()));
    }

    #[test]
    fn map_merge() {
        let base = base_spec();
        let overlay = overlay_spec();
        let merged = ServiceSpec::merge(&base, &overlay);

        // RUST_LOG: overlay "debug" wins over base "info"
        assert_eq!(merged.environment.get("RUST_LOG").unwrap(), "debug");
        // PORT: only in base, preserved
        assert_eq!(merged.environment.get("PORT").unwrap(), "8080");
        // NEW_VAR: only in overlay, added
        assert_eq!(merged.environment.get("NEW_VAR").unwrap(), "hello");
        assert_eq!(merged.environment.len(), 3);
    }

    #[test]
    fn full_spec_merge() {
        let base = base_spec();
        let overlay = overlay_spec();
        let merged = ServiceSpec::merge(&base, &overlay);

        // Scalars from overlay
        assert_eq!(merged.name, "overlay-svc");
        assert_eq!(merged.restart.strategy, RestartStrategy::Always);
        assert_eq!(merged.restart.delay_secs, 10);

        // Resources: merged field-by-field
        let res = merged.resources.as_ref().expect("resources should exist");
        assert_eq!(res.memory_max.as_deref(), Some("1G")); // overlay wins
        assert_eq!(res.tasks_max, Some(256)); // base falls through
        assert_eq!(res.cpu_weight, Some(500)); // overlay adds
        assert_eq!(res.nice, Some(-5)); // overlay adds

        // Args: concatenated + deduped
        assert!(merged.args.contains(&"--verbose".to_owned()));
        assert!(merged.args.contains(&"--debug".to_owned()));

        // Option group from overlay
        assert_eq!(merged.group, Some("overlaygroup".to_owned()));
    }

    #[test]
    fn nested_merge() {
        let mut base = base_spec();
        let mut base_systemd = HashMap::new();
        let mut base_service = HashMap::new();
        base_service.insert("LimitNOFILE".to_owned(), "65536".to_owned());
        base_service.insert("ProtectHome".to_owned(), "yes".to_owned());
        base_systemd.insert("Service".to_owned(), base_service);
        base.overrides.systemd = base_systemd;

        let mut overlay = overlay_spec();
        let mut overlay_systemd = HashMap::new();
        let mut overlay_service = HashMap::new();
        overlay_service.insert("LimitNOFILE".to_owned(), "131072".to_owned());
        overlay_service.insert("PrivateTmp".to_owned(), "yes".to_owned());
        overlay_systemd.insert("Service".to_owned(), overlay_service);
        let mut overlay_unit = HashMap::new();
        overlay_unit.insert("Documentation".to_owned(), "man:app(1)".to_owned());
        overlay_systemd.insert("Unit".to_owned(), overlay_unit);
        overlay.overrides.systemd = overlay_systemd;

        let merged = ServiceSpec::merge(&base, &overlay);

        let service = merged
            .overrides
            .systemd
            .get("Service")
            .expect("Service section");
        // Overlay wins on conflict
        assert_eq!(service.get("LimitNOFILE").unwrap(), "131072");
        // Base preserved
        assert_eq!(service.get("ProtectHome").unwrap(), "yes");
        // Overlay added
        assert_eq!(service.get("PrivateTmp").unwrap(), "yes");

        // New section from overlay
        let unit = merged
            .overrides
            .systemd
            .get("Unit")
            .expect("Unit section");
        assert_eq!(unit.get("Documentation").unwrap(), "man:app(1)");
    }

    #[test]
    fn empty_description_falls_through() {
        let base = base_spec();
        let mut overlay = overlay_spec();
        overlay.description = String::new();

        let merged = ServiceSpec::merge(&base, &overlay);
        assert_eq!(merged.description, "Base service");
    }

    #[test]
    fn resource_limits_base_only() {
        let base = base_spec();
        let mut overlay = overlay_spec();
        overlay.resources = None;

        let merged = ServiceSpec::merge(&base, &overlay);
        let res = merged.resources.as_ref().expect("resources from base");
        assert_eq!(res.memory_max.as_deref(), Some("512M"));
        assert_eq!(res.tasks_max, Some(256));
    }

    #[test]
    fn resource_limits_overlay_only() {
        let mut base = base_spec();
        base.resources = None;

        let overlay = overlay_spec();
        let merged = ServiceSpec::merge(&base, &overlay);
        let res = merged.resources.as_ref().expect("resources from overlay");
        assert_eq!(res.memory_max.as_deref(), Some("1G"));
        assert_eq!(res.cpu_weight, Some(500));
    }
}
