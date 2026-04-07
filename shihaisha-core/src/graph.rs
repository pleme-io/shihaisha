//! Dependency graph resolution using Kahn's algorithm.
//!
//! Given a set of `ServiceSpec` values with dependency relationships
//! (`after`, `before`, `requires`, `wants`), this module computes a valid
//! topological startup order or reports a cycle.
//!
//! ## Edge semantics
//!
//! - `after = [B]` — B must start before us: edge B -> us
//! - `before = [C]` — we must start before C: edge us -> C
//! - `requires = [D]` — D must be running: edge D -> us (like `after`)
//! - `wants = [E]` — E should run: edge E -> us (soft, still ordered)
//! - `conflicts` — not an ordering directive (only runtime exclusion)
//!
//! ## Algorithm
//!
//! Standard Kahn's algorithm with deterministic tie-breaking (sorted queue)
//! for reproducible output across runs.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use crate::types::service_spec::ServiceSpec;
use crate::{Error, Result};

/// Resolve startup ordering for a set of services using Kahn's algorithm.
///
/// Returns service names in topological order (dependencies first).
///
/// # Examples
///
/// ```
/// use shihaisha_core::{ServiceSpec, DependencySpec, resolve_order};
///
/// let db = ServiceSpec::new("db", "/usr/bin/db");
/// let mut app = ServiceSpec::new("app", "/usr/bin/app");
/// app.depends_on = DependencySpec {
///     after: vec!["db".to_owned()],
///     ..DependencySpec::default()
/// };
///
/// let order = resolve_order(&[db, app]).unwrap();
/// assert_eq!(order, vec!["db", "app"]);
/// ```
///
/// # Errors
///
/// Returns `Error::DependencyError` if a circular dependency is detected.
#[must_use]
pub fn resolve_order(specs: &[ServiceSpec]) -> Result<Vec<String>> {
    // Build adjacency list and in-degree counts.
    let mut graph: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut in_degree: HashMap<String, usize> = HashMap::new();

    // Initialise all nodes.
    for spec in specs {
        graph.entry(spec.name.clone()).or_default();
        in_degree.entry(spec.name.clone()).or_insert(0);
    }

    // Build edges.
    for spec in specs {
        // "after" = [B] -> B must start before us -> edge B -> spec.name
        for dep in &spec.depends_on.after {
            if let Some(neighbors) = graph.get_mut(dep) {
                neighbors.insert(spec.name.clone());
                *in_degree.entry(spec.name.clone()).or_insert(0) += 1;
            }
        }

        // "requires" = [D] -> D must be running -> edge D -> spec.name
        for dep in &spec.depends_on.requires {
            if let Some(neighbors) = graph.get_mut(dep) {
                neighbors.insert(spec.name.clone());
                *in_degree.entry(spec.name.clone()).or_insert(0) += 1;
            }
        }

        // "wants" = [E] -> E should run -> edge E -> spec.name (soft ordering)
        for dep in &spec.depends_on.wants {
            if let Some(neighbors) = graph.get_mut(dep) {
                neighbors.insert(spec.name.clone());
                *in_degree.entry(spec.name.clone()).or_insert(0) += 1;
            }
        }

        // "before" = [C] -> we must start before C -> edge spec.name -> C
        for dep in &spec.depends_on.before {
            if let Some(_entry) = graph.get(dep) {
                graph
                    .entry(spec.name.clone())
                    .or_default()
                    .insert(dep.clone());
                *in_degree.entry(dep.clone()).or_insert(0) += 1;
            }
        }
    }

    // Kahn's algorithm with sorted initial queue for determinism.
    let mut queue: VecDeque<String> = {
        let mut initial: Vec<String> = in_degree
            .iter()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(name, _)| name.clone())
            .collect();
        initial.sort();
        VecDeque::from(initial)
    };

    let mut result = Vec::new();

    while let Some(node) = queue.pop_front() {
        result.push(node.clone());

        if let Some(neighbors) = graph.get(&node) {
            // Collect and sort neighbors for deterministic ordering.
            let mut sorted_neighbors: Vec<&String> = neighbors.iter().collect();
            sorted_neighbors.sort();

            for neighbor in sorted_neighbors {
                if let Some(deg) = in_degree.get_mut(neighbor) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }
    }

    if result.len() < in_degree.len() {
        // Cycle detected -- report which nodes are involved.
        let mut remaining: Vec<String> = in_degree
            .iter()
            .filter(|(name, _)| !result.contains(name))
            .map(|(name, _)| name.clone())
            .collect();
        remaining.sort();
        Err(Error::DependencyError(format!(
            "circular dependency involving: {}",
            remaining.join(", ")
        )))
    } else {
        Ok(result)
    }
}

/// Validate that all dependency references point to services in the set.
///
/// # Errors
///
/// Returns `Error::DependencyError` if a reference to an unknown service
/// name is found.
#[must_use]
pub fn validate_references(specs: &[ServiceSpec]) -> Result<()> {
    let names: HashSet<String> = specs.iter().map(|s| s.name.clone()).collect();

    for spec in specs {
        for dep in spec
            .depends_on
            .after
            .iter()
            .chain(&spec.depends_on.before)
            .chain(&spec.depends_on.requires)
            .chain(&spec.depends_on.wants)
            .chain(&spec.depends_on.conflicts)
            .chain(&spec.depends_on.stop_before)
            .chain(&spec.depends_on.stop_after)
        {
            if !names.contains(dep) {
                return Err(Error::DependencyError(format!(
                    "service '{}' references unknown dependency '{dep}'",
                    spec.name
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::service_spec::DependencySpec;

    fn spec_with_deps(name: &str, deps: DependencySpec) -> ServiceSpec {
        let mut spec = ServiceSpec::new(name, "/usr/bin/test");
        spec.depends_on = deps;
        spec
    }

    fn no_deps(name: &str) -> ServiceSpec {
        ServiceSpec::new(name, "/usr/bin/test")
    }

    #[test]
    fn empty_set() {
        let result = resolve_order(&[]).expect("empty set");
        assert!(result.is_empty());
    }

    #[test]
    fn single_service() {
        let specs = [no_deps("alpha")];
        let order = resolve_order(&specs).expect("single");
        assert_eq!(order, vec!["alpha"]);
    }

    #[test]
    fn linear_chain() {
        // A -> B -> C  (A starts first, then B, then C)
        let specs = [
            no_deps("A"),
            spec_with_deps(
                "B",
                DependencySpec {
                    after: vec!["A".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            spec_with_deps(
                "C",
                DependencySpec {
                    after: vec!["B".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        let order = resolve_order(&specs).expect("linear");
        assert_eq!(order, vec!["A", "B", "C"]);
    }

    #[test]
    fn diamond() {
        // A -> B, A -> C, B -> D, C -> D
        let specs = [
            no_deps("A"),
            spec_with_deps(
                "B",
                DependencySpec {
                    after: vec!["A".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            spec_with_deps(
                "C",
                DependencySpec {
                    after: vec!["A".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            spec_with_deps(
                "D",
                DependencySpec {
                    after: vec!["B".to_owned(), "C".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        let order = resolve_order(&specs).expect("diamond");

        // A must come first, D must come last, B and C in between
        assert_eq!(order[0], "A");
        assert_eq!(order[3], "D");
        assert!(
            order[1..3].contains(&"B".to_owned())
                && order[1..3].contains(&"C".to_owned())
        );
    }

    #[test]
    fn cycle_detected() {
        // A -> B -> A (cycle)
        let specs = [
            spec_with_deps(
                "A",
                DependencySpec {
                    after: vec!["B".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            spec_with_deps(
                "B",
                DependencySpec {
                    after: vec!["A".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        let result = resolve_order(&specs);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("circular dependency"),
            "error should mention circular dependency: {err}"
        );
        assert!(
            err.to_string().contains('A') && err.to_string().contains('B'),
            "error should name involved services: {err}"
        );
    }

    #[test]
    fn before_and_after() {
        // X has before=[Z], Y has after=[X]
        // So: X starts before Z, Y starts after X
        // Expected: X, Y, Z (X first since it has no deps and others depend on it)
        let specs = [
            spec_with_deps(
                "X",
                DependencySpec {
                    before: vec!["Z".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            spec_with_deps(
                "Y",
                DependencySpec {
                    after: vec!["X".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            no_deps("Z"),
        ];
        let order = resolve_order(&specs).expect("before_and_after");

        // X must come before Z
        let x_pos = order.iter().position(|n| n == "X").unwrap();
        let z_pos = order.iter().position(|n| n == "Z").unwrap();
        assert!(
            x_pos < z_pos,
            "X should come before Z, got order: {order:?}"
        );

        // Y must come after X
        let y_pos = order.iter().position(|n| n == "Y").unwrap();
        assert!(
            y_pos > x_pos,
            "Y should come after X, got order: {order:?}"
        );
    }

    #[test]
    fn requires_creates_ordering() {
        // svc requires db -> db must start before svc
        let specs = [
            no_deps("db"),
            spec_with_deps(
                "svc",
                DependencySpec {
                    requires: vec!["db".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        let order = resolve_order(&specs).expect("requires");
        assert_eq!(order, vec!["db", "svc"]);
    }

    #[test]
    fn validate_missing_reference() {
        let specs = [spec_with_deps(
            "svc",
            DependencySpec {
                after: vec!["nonexistent".to_owned()],
                ..DependencySpec::default()
            },
        )];
        let result = validate_references(&specs);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("nonexistent"),
            "error should name the unknown dependency: {err}"
        );
        assert!(
            err.to_string().contains("svc"),
            "error should name the referencing service: {err}"
        );
    }

    #[test]
    fn validate_all_references_exist() {
        let specs = [
            no_deps("db"),
            spec_with_deps(
                "svc",
                DependencySpec {
                    after: vec!["db".to_owned()],
                    requires: vec!["db".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        validate_references(&specs).expect("all references should be valid");
    }

    #[test]
    fn deterministic_output() {
        let specs = [
            no_deps("delta"),
            no_deps("alpha"),
            no_deps("charlie"),
            no_deps("bravo"),
        ];

        let order1 = resolve_order(&specs).expect("run 1");
        let order2 = resolve_order(&specs).expect("run 2");

        assert_eq!(
            order1, order2,
            "same input should always produce same output"
        );

        // With no dependencies, alphabetical order is expected
        assert_eq!(order1, vec!["alpha", "bravo", "charlie", "delta"]);
    }

    #[test]
    fn parallel_independent_sorted() {
        // Two independent groups: {A->B} and {C->D}
        let specs = [
            no_deps("A"),
            spec_with_deps(
                "B",
                DependencySpec {
                    after: vec!["A".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            no_deps("C"),
            spec_with_deps(
                "D",
                DependencySpec {
                    after: vec!["C".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        let order = resolve_order(&specs).expect("parallel");

        // A before B, C before D (ordering constraint)
        let a_pos = order.iter().position(|n| n == "A").unwrap();
        let b_pos = order.iter().position(|n| n == "B").unwrap();
        let c_pos = order.iter().position(|n| n == "C").unwrap();
        let d_pos = order.iter().position(|n| n == "D").unwrap();
        assert!(a_pos < b_pos, "A before B");
        assert!(c_pos < d_pos, "C before D");

        // Result should be deterministic (A,C first since both have in-degree 0, sorted)
        assert_eq!(order.len(), 4);
    }

    #[test]
    fn wants_creates_ordering() {
        // svc wants cache -> cache should start before svc
        let specs = [
            no_deps("cache"),
            spec_with_deps(
                "svc",
                DependencySpec {
                    wants: vec!["cache".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        let order = resolve_order(&specs).expect("wants");
        assert_eq!(order, vec!["cache", "svc"]);
    }

    #[test]
    fn three_node_cycle() {
        // A -> B -> C -> A
        let specs = [
            spec_with_deps(
                "A",
                DependencySpec {
                    after: vec!["C".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            spec_with_deps(
                "B",
                DependencySpec {
                    after: vec!["A".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            spec_with_deps(
                "C",
                DependencySpec {
                    after: vec!["B".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        let result = resolve_order(&specs);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("circular dependency"));
    }

    #[test]
    fn validate_then_resolve_succeeds() {
        // Workflow: validate_references passes, then resolve_order returns correct order
        let specs = [
            no_deps("db"),
            spec_with_deps(
                "cache",
                DependencySpec {
                    after: vec!["db".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            spec_with_deps(
                "app",
                DependencySpec {
                    after: vec!["cache".to_owned()],
                    requires: vec!["db".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];

        // Step 1: validate references
        validate_references(&specs).expect("all references should be valid");

        // Step 2: resolve ordering
        let order = resolve_order(&specs).expect("should resolve without cycles");

        // db must come before cache, cache must come before app
        let db_pos = order.iter().position(|n| n == "db").unwrap();
        let cache_pos = order.iter().position(|n| n == "cache").unwrap();
        let app_pos = order.iter().position(|n| n == "app").unwrap();
        assert!(db_pos < cache_pos, "db before cache");
        assert!(cache_pos < app_pos, "cache before app");
    }

    #[test]
    fn resolve_order_with_unknown_deps_graceful() {
        // If a spec references a dep that is not in the set, resolve_order
        // should not crash. It simply ignores edges to unknown nodes since
        // graph.get_mut(dep) returns None for unknown names.
        let specs = [
            no_deps("standalone"),
            spec_with_deps(
                "orphan",
                DependencySpec {
                    after: vec!["unknown-service".to_owned()],
                    wants: vec!["another-missing".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];

        // resolve_order should not panic or error -- it silently drops
        // edges to services not in the set.
        let order = resolve_order(&specs).expect("should not crash on unknown deps");
        assert_eq!(order.len(), 2);
        assert!(order.contains(&"standalone".to_owned()));
        assert!(order.contains(&"orphan".to_owned()));
    }

    #[test]
    fn validate_missing_before_reference() {
        let specs = [spec_with_deps(
            "svc",
            DependencySpec {
                before: vec!["nonexistent".to_owned()],
                ..DependencySpec::default()
            },
        )];
        let result = validate_references(&specs);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("nonexistent"));
    }

    #[test]
    fn validate_missing_requires_reference() {
        let specs = [spec_with_deps(
            "svc",
            DependencySpec {
                requires: vec!["missing-db".to_owned()],
                ..DependencySpec::default()
            },
        )];
        let result = validate_references(&specs);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing-db"));
    }

    #[test]
    fn validate_missing_wants_reference() {
        let specs = [spec_with_deps(
            "svc",
            DependencySpec {
                wants: vec!["missing-cache".to_owned()],
                ..DependencySpec::default()
            },
        )];
        let result = validate_references(&specs);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing-cache"));
    }

    #[test]
    fn validate_missing_conflicts_reference() {
        let specs = [spec_with_deps(
            "svc",
            DependencySpec {
                conflicts: vec!["old-svc".to_owned()],
                ..DependencySpec::default()
            },
        )];
        let result = validate_references(&specs);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("old-svc"));
    }

    #[test]
    fn validate_missing_stop_before_reference() {
        let specs = [spec_with_deps(
            "svc",
            DependencySpec {
                stop_before: vec!["missing-stop".to_owned()],
                ..DependencySpec::default()
            },
        )];
        let result = validate_references(&specs);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing-stop"));
    }

    #[test]
    fn validate_missing_stop_after_reference() {
        let specs = [spec_with_deps(
            "svc",
            DependencySpec {
                stop_after: vec!["missing-dep".to_owned()],
                ..DependencySpec::default()
            },
        )];
        let result = validate_references(&specs);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing-dep"));
    }

    #[test]
    fn resolve_order_before_pointing_at_unknown_is_silent() {
        let specs = [
            spec_with_deps(
                "svc",
                DependencySpec {
                    before: vec!["nonexistent".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        let order = resolve_order(&specs).expect("should not error");
        assert_eq!(order, vec!["svc"]);
    }

    #[test]
    fn resolve_order_requires_unknown_is_silent() {
        let specs = [
            spec_with_deps(
                "svc",
                DependencySpec {
                    requires: vec!["unknown".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        let order = resolve_order(&specs).expect("should not error");
        assert_eq!(order, vec!["svc"]);
    }

    #[test]
    fn validate_empty_set_succeeds() {
        let specs: Vec<ServiceSpec> = vec![];
        validate_references(&specs).expect("empty set is valid");
    }

    #[test]
    fn validate_all_dependency_types_present() {
        let specs = [
            no_deps("db"),
            no_deps("cache"),
            no_deps("old-svc"),
            no_deps("stop-target"),
            spec_with_deps(
                "svc",
                DependencySpec {
                    after: vec!["db".to_owned()],
                    before: vec!["cache".to_owned()],
                    requires: vec!["db".to_owned()],
                    wants: vec!["cache".to_owned()],
                    conflicts: vec!["old-svc".to_owned()],
                    stop_before: vec!["cache".to_owned()],
                    stop_after: vec!["stop-target".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        validate_references(&specs).expect("all references exist");
    }

    #[test]
    fn cycle_error_is_dependency_error() {
        let specs = [
            spec_with_deps(
                "A",
                DependencySpec {
                    after: vec!["B".to_owned()],
                    ..DependencySpec::default()
                },
            ),
            spec_with_deps(
                "B",
                DependencySpec {
                    after: vec!["A".to_owned()],
                    ..DependencySpec::default()
                },
            ),
        ];
        let err = resolve_order(&specs).unwrap_err();
        assert!(!err.is_retryable(), "cycle errors should not be retryable");
    }
}
