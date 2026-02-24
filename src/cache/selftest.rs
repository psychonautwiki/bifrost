//! Search self-test module.
//!
//! Validates the search index against a curated test fixture at boot time.
//! If the self-test fails, the API readiness flag stays false and the server
//! refuses to serve GraphQL queries until the issue is resolved.
//!
//! Test fixture: `data/search_tests.json`
//! Generated from: `data/substance_aliases.json`
//!
//! Test categories:
//! 1. **Exact match tests**: For every substance with curated aliases, every
//!    alias query must return exactly that substance and nothing else.
//! 2. **Negative (exclusion) tests**: Specific high-profile queries that must
//!    NOT return commonly confused substances (e.g., "2cb" -> 2C-B only,
//!    never 2C-C, 2C-D, etc.).

use crate::cache::snapshot::SubstanceSnapshot;
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use tracing::{error, info};

/// Result of running the self-test suite
#[derive(Debug)]
pub struct SelfTestResult {
    /// Total number of test assertions run
    pub total_assertions: usize,
    /// Number of passing assertions
    pub passed: usize,
    /// Number of failing assertions
    pub failed: usize,
    /// Number of tests skipped (substance not in snapshot)
    pub skipped: usize,
    /// Detailed failure reports
    pub failures: Vec<SelfTestFailure>,
    /// Duration of the test run
    pub duration_ms: u64,
}

impl SelfTestResult {
    pub fn is_pass(&self) -> bool {
        self.failed == 0
    }
}

impl fmt::Display for SelfTestResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_pass() {
            write!(
                f,
                "SELF-TEST PASSED: {}/{} assertions passed ({} skipped) in {}ms",
                self.passed, self.total_assertions, self.skipped, self.duration_ms
            )
        } else {
            write!(
                f,
                "SELF-TEST FAILED: {}/{} assertions failed ({} passed, {} skipped) in {}ms",
                self.failed, self.total_assertions, self.passed, self.skipped, self.duration_ms
            )
        }
    }
}

/// A single test failure
#[derive(Debug)]
pub struct SelfTestFailure {
    pub test_type: &'static str,
    pub query: String,
    pub expected: String,
    pub actual: String,
    pub detail: String,
}

impl fmt::Display for SelfTestFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] query=\"{}\" expected=\"{}\" actual=\"{}\" detail=\"{}\"",
            self.test_type, self.query, self.expected, self.actual, self.detail
        )
    }
}

// === Test fixture deserialization ===

#[derive(Debug, Deserialize)]
struct SearchTestFixture {
    exact_match_tests: Vec<ExactMatchGroup>,
    negative_tests: Vec<NegativeTest>,
}

#[derive(Debug, Deserialize)]
struct ExactMatchGroup {
    /// Canonical substance name
    substance: String,
    /// All queries that should resolve to this substance
    queries: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct NegativeTest {
    /// The search query
    query: String,
    /// The expected substance in the result
    expected: String,
    /// Substances that must NOT appear in the result
    excluded: Vec<String>,
}

/// Load the test fixture from disk
fn load_test_fixture(path: &Path) -> anyhow::Result<SearchTestFixture> {
    let content = std::fs::read_to_string(path)?;
    let fixture: SearchTestFixture = serde_json::from_str(&content)?;
    Ok(fixture)
}

/// Run the full self-test suite against a snapshot.
///
/// This validates:
/// 1. Every curated alias resolves to exactly its parent substance (no extra results)
/// 2. High-profile negative tests pass (no false positives for commonly confused substances)
///
/// Substances not present in the snapshot are skipped (they may not have been fetched yet).
/// The test only validates substances that are actually in the index.
pub fn run_self_test(snapshot: &SubstanceSnapshot) -> SelfTestResult {
    let start = std::time::Instant::now();

    let fixture_path = Path::new("data/search_tests.json");
    let fixture = match load_test_fixture(fixture_path) {
        Ok(f) => f,
        Err(e) => {
            error!(
                error = %e,
                path = %fixture_path.display(),
                "Failed to load self-test fixture"
            );
            return SelfTestResult {
                total_assertions: 0,
                passed: 0,
                failed: 1,
                skipped: 0,
                failures: vec![SelfTestFailure {
                    test_type: "fixture_load",
                    query: String::new(),
                    expected: "fixture file readable".into(),
                    actual: format!("error: {}", e),
                    detail: format!("Could not load {}", fixture_path.display()),
                }],
                duration_ms: start.elapsed().as_millis() as u64,
            };
        }
    };

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;
    let mut failures = Vec::new();

    // Build a set of substance names in the snapshot for fast lookup
    let snapshot_names: HashSet<String> = snapshot
        .substances
        .iter()
        .filter_map(|s| s.name.as_ref().map(|n| n.to_lowercase()))
        .collect();

    // === Exact match tests ===
    for group in &fixture.exact_match_tests {
        // Skip if substance is not in the snapshot
        if !snapshot_names.contains(&group.substance.to_lowercase()) {
            skipped += group.queries.len();
            continue;
        }

        for query in &group.queries {
            total += 1;

            let results = snapshot.search(query);
            let result_names: Vec<&str> =
                results.iter().filter_map(|s| s.name.as_deref()).collect();

            // Must return exactly one result matching the expected substance
            if results.len() == 1
                && result_names[0].to_lowercase() == group.substance.to_lowercase()
            {
                passed += 1;
            } else if results.is_empty() {
                failed += 1;
                failures.push(SelfTestFailure {
                    test_type: "exact_match",
                    query: query.clone(),
                    expected: format!("[{}]", group.substance),
                    actual: "[]".into(),
                    detail: format!(
                        "Query \"{}\" returned no results, expected exactly [{}]",
                        query, group.substance
                    ),
                });
            } else {
                failed += 1;
                let actual_str = result_names.join(", ");
                failures.push(SelfTestFailure {
                    test_type: "exact_match",
                    query: query.clone(),
                    expected: format!("[{}]", group.substance),
                    actual: format!("[{}]", actual_str),
                    detail: format!(
                        "Query \"{}\" returned {} results [{}], expected exactly [{}]",
                        query,
                        results.len(),
                        actual_str,
                        group.substance
                    ),
                });
            }
        }
    }

    // === Negative (exclusion) tests ===
    for neg in &fixture.negative_tests {
        // Skip if the expected substance is not in the snapshot
        if !snapshot_names.contains(&neg.expected.to_lowercase()) {
            skipped += 1;
            continue;
        }

        total += 1;

        let results = snapshot.search(&neg.query);
        let result_names_lower: HashSet<String> = results
            .iter()
            .filter_map(|s| s.name.as_ref().map(|n| n.to_lowercase()))
            .collect();

        // Check that expected substance IS in results
        let has_expected = result_names_lower.contains(&neg.expected.to_lowercase());

        // Check that none of the excluded substances are in results
        let found_excluded: Vec<&String> = neg
            .excluded
            .iter()
            .filter(|ex| result_names_lower.contains(&ex.to_lowercase()))
            .collect();

        if has_expected && found_excluded.is_empty() {
            passed += 1;
        } else {
            failed += 1;
            let result_names: Vec<&str> =
                results.iter().filter_map(|s| s.name.as_deref()).collect();

            let detail = if !has_expected {
                format!(
                    "Expected {} in results but got [{}]",
                    neg.expected,
                    result_names.join(", ")
                )
            } else {
                format!(
                    "Found excluded substances {:?} in results [{}]",
                    found_excluded,
                    result_names.join(", ")
                )
            };

            failures.push(SelfTestFailure {
                test_type: "negative_exclusion",
                query: neg.query.clone(),
                expected: format!(
                    "must include {} and exclude {:?}",
                    neg.expected, neg.excluded
                ),
                actual: format!("[{}]", result_names.join(", ")),
                detail,
            });
        }
    }

    let duration_ms = start.elapsed().as_millis() as u64;

    let result = SelfTestResult {
        total_assertions: total,
        passed,
        failed,
        skipped,
        failures,
        duration_ms,
    };

    if result.is_pass() {
        info!(
            total = result.total_assertions,
            passed = result.passed,
            skipped = result.skipped,
            duration_ms = result.duration_ms,
            "Search self-test PASSED"
        );
    } else {
        error!(
            total = result.total_assertions,
            passed = result.passed,
            failed = result.failed,
            skipped = result.skipped,
            duration_ms = result.duration_ms,
            "Search self-test FAILED"
        );
        // Log first N failures for visibility
        let max_log = 20;
        for (i, failure) in result.failures.iter().enumerate() {
            if i >= max_log {
                error!(
                    remaining = result.failures.len() - max_log,
                    "... and more failures (truncated)"
                );
                break;
            }
            error!(failure = %failure, "Self-test failure");
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::snapshot::{SubstanceAliases, SubstanceSnapshot};
    use crate::graphql::model::Substance;
    use std::collections::HashMap;

    fn make_substance(name: &str) -> Substance {
        Substance {
            name: Some(name.to_string()),
            ..Default::default()
        }
    }

    /// Test that the self-test passes with a correctly configured snapshot.
    /// Uses a representative set of substances covering all negative test targets.
    #[test]
    fn test_selftest_passes_basic() {
        let substances = vec![
            // 2C-x family
            make_substance("2C-B"),
            make_substance("2C-C"),
            make_substance("2C-D"),
            make_substance("2C-E"),
            make_substance("2C-I"),
            make_substance("2C-P"),
            make_substance("2C-T-2"),
            make_substance("2C-T-7"),
            make_substance("2C-T-21"),
            make_substance("2C-H"),
            make_substance("2C-B-FLY"),
            make_substance("2C-x"),
            // LSD family
            make_substance("LSD"),
            make_substance("LSA"),
            make_substance("LSM-775"),
            make_substance("LSZ"),
            make_substance("1P-LSD"),
            make_substance("AL-LAD"),
            make_substance("ALD-52"),
            make_substance("ETH-LAD"),
            // MDMA family
            make_substance("MDMA"),
            make_substance("MDA"),
            make_substance("MDEA"),
            make_substance("Methylone"),
            make_substance("MDPV"),
            // DMT family
            make_substance("DMT"),
            make_substance("5-MeO-DMT"),
            make_substance("4-AcO-DMT"),
            make_substance("DPT"),
            make_substance("DET"),
            make_substance("DiPT"),
            // Ketamine family
            make_substance("Ketamine"),
            make_substance("Methoxetamine"),
            make_substance("Deschloroketamine"),
            make_substance("2-Fluorodeschloroketamine"),
            // Amphetamine family
            make_substance("Amphetamine"),
            make_substance("Methamphetamine"),
            make_substance("Dextroamphetamine"),
            make_substance("Lisdexamfetamine"),
            make_substance("Methylphenidate"),
            // Opioids
            make_substance("Heroin"),
            make_substance("Morphine"),
            make_substance("Codeine"),
            make_substance("Oxycodone"),
            make_substance("Hydrocodone"),
            make_substance("Fentanyl"),
            // GHB/GBL
            make_substance("GHB"),
            make_substance("GBL"),
            make_substance("1,4-Butanediol"),
            // Psilocybin
            make_substance("Psilocybin mushrooms"),
            make_substance("Psilocin"),
            make_substance("Amanita muscaria"),
            // Benzos
            make_substance("Alprazolam"),
            make_substance("Diazepam"),
            make_substance("Clonazepam"),
            make_substance("Lorazepam"),
            // NBOMe
            make_substance("25I-NBOMe"),
            make_substance("25B-NBOMe"),
            make_substance("25C-NBOMe"),
            make_substance("25D-NBOMe"),
            // Misc
            make_substance("Cannabis"),
            make_substance("Mescaline"),
            make_substance("Escaline"),
            make_substance("Proscaline"),
            make_substance("PCP"),
            make_substance("PCE"),
            make_substance("3-MeO-PCP"),
            make_substance("3-MeO-PCE"),
            make_substance("Dextromethorphan"),
            make_substance("Ibogaine"),
            make_substance("Kratom"),
            make_substance("Nitrous"),
            make_substance("Salvinorin A"),
        ];

        // Load the real alias file
        let alias_path = Path::new("data/substance_aliases.json");
        let aliases = if alias_path.exists() {
            SubstanceAliases::load_from_file(alias_path)
                .unwrap_or_else(|_| SubstanceAliases::empty())
        } else {
            // If no alias file, build minimal aliases for the test
            let mut map = HashMap::new();
            map.insert("2C-B".into(), vec!["2cb".into(), "2-cb".into()]);
            map.insert(
                "LSD".into(),
                vec!["Acid".into(), "LSD-25".into(), "Lucy".into()],
            );
            map.insert(
                "MDMA".into(),
                vec!["Molly".into(), "Ecstasy".into(), "XTC".into()],
            );
            SubstanceAliases { aliases: map }
        };

        let snapshot = SubstanceSnapshot::build_with_aliases(substances, aliases);
        let result = run_self_test(&snapshot);

        // With only a subset of substances, many tests will be skipped
        // but the ones that DO run should pass
        if !result.is_pass() {
            for f in &result.failures {
                eprintln!("FAILURE: {}", f);
            }
        }
        assert!(
            result.is_pass(),
            "Self-test failed with {} failures out of {} assertions",
            result.failed,
            result.total_assertions
        );
        assert!(result.total_assertions > 0, "Should have run some tests");
        assert!(
            result.skipped > 0,
            "Should have skipped some tests (not all substances present)"
        );
    }

    /// Test that the self-test catches a broken alias index.
    /// If "2cb" maps to the wrong substance, the test should fail.
    #[test]
    fn test_selftest_catches_wrong_alias() {
        let substances = vec![make_substance("2C-B"), make_substance("2C-C")];

        // Intentionally wrong: map "2cb" to 2C-C instead of 2C-B
        let mut map = HashMap::new();
        map.insert("2C-C".into(), vec!["2cb".into(), "2-cb".into()]);
        let aliases = SubstanceAliases { aliases: map };

        let snapshot = SubstanceSnapshot::build_with_aliases(substances, aliases);
        let result = run_self_test(&snapshot);

        // The test for "2cb" -> 2C-B should now fail (it resolves to 2C-C)
        // And/or the negative test for "2cb" should fail
        assert!(
            result.failed > 0,
            "Self-test should have caught the wrong alias mapping"
        );
    }
}
