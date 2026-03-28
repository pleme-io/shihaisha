//! Structural diff between two `ServiceSpec` values.
//!
//! Computes a set of [`Change`] values describing what fields were added,
//! removed, or modified between an old and new spec.  The implementation
//! serialises both specs to `serde_json::Value` and recursively walks the
//! JSON trees, producing dot-delimited field paths (e.g. `restart.strategy`,
//! `environment.PORT`).
//!
//! This is the foundation for a plan/apply workflow: compute the diff, show
//! it to the user, then apply only if confirmed.

use crate::types::service_spec::ServiceSpec;

/// A single structural change between two specs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A field or key was added (exists in new but not old).
    Added {
        /// Dot-delimited path to the field.
        path: String,
        /// JSON representation of the new value.
        value: String,
    },
    /// A field or key was removed (exists in old but not new).
    Removed {
        /// Dot-delimited path to the field.
        path: String,
        /// JSON representation of the old value.
        value: String,
    },
    /// A field's value changed.
    Modified {
        /// Dot-delimited path to the field.
        path: String,
        /// JSON representation of the old value.
        old: String,
        /// JSON representation of the new value.
        new: String,
    },
}

/// Compute the structural diff between two `ServiceSpec` values.
///
/// Returns an empty `Vec` when the specs are identical.  The comparison
/// is performed on the JSON representation, so field ordering within
/// objects does not matter.
#[must_use]
pub fn diff(old: &ServiceSpec, new: &ServiceSpec) -> Vec<Change> {
    let old_val = serde_json::to_value(old).expect("ServiceSpec serialises to JSON");
    let new_val = serde_json::to_value(new).expect("ServiceSpec serialises to JSON");
    let mut changes = Vec::new();
    diff_values("", &old_val, &new_val, &mut changes);
    // Sort for deterministic output
    changes.sort_by(|a, b| {
        let path_a = match a {
            Change::Added { path, .. }
            | Change::Removed { path, .. }
            | Change::Modified { path, .. } => path,
        };
        let path_b = match b {
            Change::Added { path, .. }
            | Change::Removed { path, .. }
            | Change::Modified { path, .. } => path,
        };
        path_a.cmp(path_b)
    });
    changes
}

/// Recursively diff two JSON values, accumulating changes.
fn diff_values(
    path: &str,
    old: &serde_json::Value,
    new: &serde_json::Value,
    changes: &mut Vec<Change>,
) {
    use serde_json::Value;

    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            // Check keys in old
            for (k, v) in a {
                let child_path = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                match b.get(k) {
                    Some(bv) => diff_values(&child_path, v, bv, changes),
                    None => changes.push(Change::Removed {
                        path: child_path,
                        value: v.to_string(),
                    }),
                }
            }
            // Check keys only in new
            for (k, v) in b {
                if !a.contains_key(k) {
                    let child_path = if path.is_empty() {
                        k.clone()
                    } else {
                        format!("{path}.{k}")
                    };
                    changes.push(Change::Added {
                        path: child_path,
                        value: v.to_string(),
                    });
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a != b {
                changes.push(Change::Modified {
                    path: path.to_owned(),
                    old: old.to_string(),
                    new: new.to_string(),
                });
            }
        }
        _ => {
            if old != new {
                changes.push(Change::Modified {
                    path: path.to_owned(),
                    old: old.to_string(),
                    new: new.to_string(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::service_spec::RestartStrategy;

    fn test_spec(name: &str) -> ServiceSpec {
        let mut spec = ServiceSpec::new(name, "/usr/bin/test");
        spec.description = "Test service".to_owned();
        spec
    }

    #[test]
    fn no_changes() {
        let spec = test_spec("test");
        let changes = diff(&spec, &spec);
        assert!(changes.is_empty(), "identical specs should produce no diff");
    }

    #[test]
    fn name_changed() {
        let old = test_spec("old-name");
        let new = test_spec("new-name");
        let changes = diff(&old, &new);

        let name_change = changes
            .iter()
            .find(|c| matches!(c, Change::Modified { path, .. } if path == "name"))
            .expect("should detect name change");

        match name_change {
            Change::Modified { old, new, .. } => {
                assert!(old.contains("old-name"));
                assert!(new.contains("new-name"));
            }
            _ => panic!("expected Modified"),
        }
    }

    #[test]
    fn env_added() {
        let old = test_spec("test");
        let mut new = test_spec("test");
        new.environment
            .insert("NEW_VAR".to_owned(), "value".to_owned());

        let changes = diff(&old, &new);

        let added = changes
            .iter()
            .find(|c| {
                matches!(c, Change::Added { path, .. } if path == "environment.NEW_VAR")
            })
            .expect("should detect added env var");

        match added {
            Change::Added { value, .. } => {
                assert!(value.contains("value"));
            }
            _ => panic!("expected Added"),
        }
    }

    #[test]
    fn env_removed() {
        let mut old = test_spec("test");
        old.environment
            .insert("OLD_VAR".to_owned(), "gone".to_owned());
        let new = test_spec("test");

        let changes = diff(&old, &new);

        let removed = changes
            .iter()
            .find(|c| {
                matches!(c, Change::Removed { path, .. } if path == "environment.OLD_VAR")
            })
            .expect("should detect removed env var");

        match removed {
            Change::Removed { value, .. } => {
                assert!(value.contains("gone"));
            }
            _ => panic!("expected Removed"),
        }
    }

    #[test]
    fn nested_change() {
        let old = test_spec("test");
        let mut new = test_spec("test");
        new.restart.strategy = RestartStrategy::Always;

        let changes = diff(&old, &new);

        let strategy_change = changes
            .iter()
            .find(|c| {
                matches!(c, Change::Modified { path, .. } if path == "restart.strategy")
            })
            .expect("should detect restart.strategy change");

        match strategy_change {
            Change::Modified { old, new, .. } => {
                assert!(old.contains("on-failure"));
                assert!(new.contains("always"));
            }
            _ => panic!("expected Modified"),
        }
    }

    #[test]
    fn multiple_changes() {
        let old = test_spec("test");
        let mut new = test_spec("test");
        new.description = "Updated description".to_owned();
        new.command = "/usr/bin/updated".to_owned();
        new.notify = true;
        new.timeout_start_sec = 120;

        let changes = diff(&old, &new);

        assert!(
            changes.len() >= 4,
            "expected at least 4 changes, got {}",
            changes.len()
        );

        // Verify specific paths are present
        let paths: Vec<&str> = changes
            .iter()
            .map(|c| match c {
                Change::Added { path, .. }
                | Change::Removed { path, .. }
                | Change::Modified { path, .. } => path.as_str(),
            })
            .collect();

        assert!(paths.contains(&"description"));
        assert!(paths.contains(&"command"));
        assert!(paths.contains(&"notify"));
        assert!(paths.contains(&"timeout_start_sec"));
    }

    #[test]
    fn deterministic_ordering() {
        let old = test_spec("test");
        let mut new = test_spec("test");
        new.description = "Changed".to_owned();
        new.command = "/changed".to_owned();

        let changes1 = diff(&old, &new);
        let changes2 = diff(&old, &new);

        assert_eq!(changes1, changes2, "diff output should be deterministic");
    }

    #[test]
    fn args_array_change() {
        let old = test_spec("test");
        let mut new = test_spec("test");
        new.args = vec!["--flag".to_owned()];

        let changes = diff(&old, &new);

        let args_change = changes
            .iter()
            .find(|c| matches!(c, Change::Modified { path, .. } if path == "args"))
            .expect("should detect args change");

        match args_change {
            Change::Modified { old, new, .. } => {
                assert!(old.contains("[]"));
                assert!(new.contains("--flag"));
            }
            _ => panic!("expected Modified"),
        }
    }
}
