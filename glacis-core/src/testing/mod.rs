//! Property-Based Testing Infrastructure
//!
//! Provides property-based testing for coverage invariants:
//! - Coverage scores: 0 ≤ score ≤ 1
//! - Transitivity bounds
//! - Policy consistency checks

use std::collections::HashMap;

/// Coverage score that enforces 0 ≤ score ≤ 1 invariant
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverageScore(f64);

impl CoverageScore {
    /// Create a new coverage score, clamping to valid range
    pub fn new(score: f64) -> Self {
        Self(score.clamp(0.0, 1.0))
    }

    /// Create a coverage score, returning None if out of range
    pub fn try_new(score: f64) -> Option<Self> {
        if (0.0..=1.0).contains(&score) {
            Some(Self(score))
        } else {
            None
        }
    }

    /// Get the raw score value
    pub fn value(&self) -> f64 {
        self.0
    }

    /// Check if fully covered
    pub fn is_full(&self) -> bool {
        (self.0 - 1.0).abs() < f64::EPSILON
    }

    /// Check if not covered at all
    pub fn is_none(&self) -> bool {
        self.0.abs() < f64::EPSILON
    }

    /// Combine two coverage scores (minimum)
    pub fn combine_min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }

    /// Combine two coverage scores (maximum)
    pub fn combine_max(self, other: Self) -> Self {
        Self(self.0.max(other.0))
    }

    /// Combine two coverage scores (average)
    pub fn combine_avg(self, other: Self) -> Self {
        Self((self.0 + other.0) / 2.0)
    }
}

impl From<CoverageScore> for f64 {
    fn from(score: CoverageScore) -> f64 {
        score.0
    }
}

/// A control requirement with coverage
#[derive(Debug, Clone)]
pub struct Requirement {
    /// Requirement ID
    pub id: String,
    /// Coverage score
    pub coverage: CoverageScore,
    /// Dependencies on other requirements
    pub dependencies: Vec<String>,
}

impl Requirement {
    /// Create a new requirement with the given coverage
    pub fn new(id: impl Into<String>, coverage: f64) -> Self {
        Self {
            id: id.into(),
            coverage: CoverageScore::new(coverage),
            dependencies: Vec::new(),
        }
    }

    /// Add dependencies to this requirement
    pub fn with_dependencies(mut self, deps: Vec<String>) -> Self {
        self.dependencies = deps;
        self
    }
}

/// A policy containing multiple requirements
#[derive(Debug, Clone)]
pub struct Policy {
    /// Policy ID
    pub id: String,
    /// Requirements in this policy
    pub requirements: Vec<Requirement>,
}

impl Policy {
    /// Create a new empty policy
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            requirements: Vec::new(),
        }
    }

    /// Add a requirement to this policy
    pub fn add_requirement(mut self, req: Requirement) -> Self {
        self.requirements.push(req);
        self
    }

    /// Calculate aggregate coverage score
    pub fn aggregate_coverage(&self) -> CoverageScore {
        if self.requirements.is_empty() {
            return CoverageScore::new(0.0);
        }

        let sum: f64 = self.requirements.iter().map(|r| r.coverage.value()).sum();
        CoverageScore::new(sum / self.requirements.len() as f64)
    }

    /// Check transitivity: if A depends on B, A's coverage can't exceed B's
    pub fn check_transitivity(&self) -> Vec<String> {
        let coverage_map: HashMap<_, _> = self
            .requirements
            .iter()
            .map(|r| (r.id.clone(), r.coverage))
            .collect();

        let mut violations = Vec::new();

        for req in &self.requirements {
            for dep_id in &req.dependencies {
                if let Some(&dep_coverage) = coverage_map.get(dep_id) {
                    if req.coverage.value() > dep_coverage.value() + f64::EPSILON {
                        violations.push(format!(
                            "{} (coverage {:.2}) exceeds dependency {} (coverage {:.2})",
                            req.id,
                            req.coverage.value(),
                            dep_id,
                            dep_coverage.value()
                        ));
                    }
                }
            }
        }

        violations
    }
}

// Proptest strategies are only available in tests
#[cfg(test)]
mod strategies {
    use super::*;
    use proptest::prelude::*;

    /// Strategy for generating valid coverage scores
    pub fn coverage_score_strategy() -> impl Strategy<Value = CoverageScore> {
        (0.0f64..=1.0f64).prop_map(CoverageScore::new)
    }

    /// Strategy for generating requirement IDs
    pub fn requirement_id_strategy() -> impl Strategy<Value = String> {
        proptest::string::string_regex("[A-Z]{2,4}-[0-9]{1,4}")
            .unwrap()
            .prop_map(|s| s)
    }

    /// Strategy for generating requirements
    pub fn requirement_strategy() -> impl Strategy<Value = Requirement> {
        (requirement_id_strategy(), coverage_score_strategy()).prop_map(|(id, coverage)| {
            Requirement {
                id,
                coverage,
                dependencies: Vec::new(),
            }
        })
    }

    /// Strategy for generating policies with requirements
    pub fn policy_strategy(max_requirements: usize) -> impl Strategy<Value = Policy> {
        (
            requirement_id_strategy(),
            proptest::collection::vec(requirement_strategy(), 0..=max_requirements),
        )
            .prop_map(|(id, requirements)| Policy { id, requirements })
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use super::strategies::*;
    use proptest::prelude::*;

    proptest! {
        /// Property: Coverage scores are always in [0, 1]
        #[test]
        fn coverage_score_bounded(score in any::<f64>()) {
            let coverage = CoverageScore::new(score);
            prop_assert!(coverage.value() >= 0.0);
            prop_assert!(coverage.value() <= 1.0);
        }

        /// Property: try_new rejects invalid scores
        #[test]
        fn try_new_rejects_invalid(score in any::<f64>().prop_filter("out of range", |s| *s < 0.0 || *s > 1.0)) {
            prop_assert!(CoverageScore::try_new(score).is_none());
        }

        /// Property: try_new accepts valid scores
        #[test]
        fn try_new_accepts_valid(score in 0.0f64..=1.0f64) {
            prop_assert!(CoverageScore::try_new(score).is_some());
        }

        /// Property: combine_min produces valid score
        #[test]
        fn combine_min_valid(
            a in coverage_score_strategy(),
            b in coverage_score_strategy()
        ) {
            let result = a.combine_min(b);
            prop_assert!(result.value() >= 0.0);
            prop_assert!(result.value() <= 1.0);
            prop_assert!(result.value() <= a.value());
            prop_assert!(result.value() <= b.value());
        }

        /// Property: combine_max produces valid score
        #[test]
        fn combine_max_valid(
            a in coverage_score_strategy(),
            b in coverage_score_strategy()
        ) {
            let result = a.combine_max(b);
            prop_assert!(result.value() >= 0.0);
            prop_assert!(result.value() <= 1.0);
            prop_assert!(result.value() >= a.value());
            prop_assert!(result.value() >= b.value());
        }

        /// Property: combine_avg produces valid score
        #[test]
        fn combine_avg_valid(
            a in coverage_score_strategy(),
            b in coverage_score_strategy()
        ) {
            let result = a.combine_avg(b);
            prop_assert!(result.value() >= 0.0);
            prop_assert!(result.value() <= 1.0);
            prop_assert!(result.value() >= a.value().min(b.value()));
            prop_assert!(result.value() <= a.value().max(b.value()));
        }

        /// Property: Policy aggregate coverage is bounded
        #[test]
        fn policy_aggregate_bounded(policy in policy_strategy(10)) {
            let aggregate = policy.aggregate_coverage();
            prop_assert!(aggregate.value() >= 0.0);
            prop_assert!(aggregate.value() <= 1.0);
        }

        /// Property: Empty policy has zero coverage
        #[test]
        fn empty_policy_zero_coverage(id in requirement_id_strategy()) {
            let policy = Policy::new(id);
            prop_assert!((policy.aggregate_coverage().value() - 0.0).abs() < f64::EPSILON);
        }

        /// Property: Single requirement policy has that requirement's coverage
        #[test]
        fn single_req_policy_coverage(
            id in requirement_id_strategy(),
            req in requirement_strategy()
        ) {
            let expected = req.coverage.value();
            let policy = Policy::new(id).add_requirement(req);
            prop_assert!((policy.aggregate_coverage().value() - expected).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_transitivity_check() {
        let mut policy = Policy::new("TEST-POLICY");

        // B has 50% coverage
        let req_b = Requirement::new("REQ-B", 0.5);

        // A depends on B but claims 80% coverage - violation!
        let req_a = Requirement::new("REQ-A", 0.8).with_dependencies(vec!["REQ-B".to_string()]);

        policy = policy.add_requirement(req_a).add_requirement(req_b);

        let violations = policy.check_transitivity();
        assert!(!violations.is_empty());
        assert!(violations[0].contains("REQ-A"));
        assert!(violations[0].contains("REQ-B"));
    }

    #[test]
    fn test_valid_transitivity() {
        let mut policy = Policy::new("TEST-POLICY");

        // B has 80% coverage
        let req_b = Requirement::new("REQ-B", 0.8);

        // A depends on B and has 50% coverage - valid
        let req_a = Requirement::new("REQ-A", 0.5).with_dependencies(vec!["REQ-B".to_string()]);

        policy = policy.add_requirement(req_a).add_requirement(req_b);

        let violations = policy.check_transitivity();
        assert!(violations.is_empty());
    }
}
