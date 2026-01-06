//! Reagent test results service.
//!
//! Loads and indexes reagent test data from `data/reagents.json`, providing
//! fuzzy matching for substance names and lookups for reagent results.

use crate::graphql::model::{Reagent, ReagentColor, ReagentTestResult, SubstanceReagents};
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info};

// ============================================================================
// Raw JSON Data Structures (for deserialization)
// ============================================================================

#[derive(Deserialize, Debug)]
struct RawReagentData {
    colors: Vec<RawColor>,
    reagents: Vec<RawReagent>,
    results: Vec<Option<Vec<Option<Vec<RawResult>>>>>,
    substances: Vec<RawSubstance>,
}

#[derive(Deserialize, Debug, Clone)]
struct RawColor {
    id: i32,
    name: String,
    hex: String,
    simple: bool,
    #[serde(rename = "simpleColorId")]
    simple_color_id: i32,
}

#[derive(Deserialize, Debug, Clone)]
struct RawReagent {
    id: i32,
    name: String,
    #[serde(rename = "fullName")]
    full_name: String,
    #[serde(rename = "shortName")]
    short_name: String,
    #[serde(rename = "whiteFirstColor")]
    white_first_color: Option<bool>,
}

#[derive(Deserialize, Debug)]
struct RawSubstance {
    id: i32,
    name: String,
    #[serde(rename = "commonName")]
    common_name: String,
    token: String,
    #[serde(default)]
    classes: Vec<String>,
    #[serde(rename = "isPopular")]
    is_popular: bool,
    sid: i32,
}

/// Raw result tuple: [start_color_ids, end_color_ids, is_positive, description]
#[derive(Deserialize, Debug)]
struct RawResult(Vec<i32>, Vec<i32>, bool, String);

// ============================================================================
// Parsed Substance Name
// ============================================================================

/// A parsed substance name with chemical name and alternative names
#[derive(Debug, Clone)]
struct ParsedSubstanceName {
    /// The primary chemical name (e.g., "4-HO-MET")
    chemical_name: String,
    /// Alternative names extracted from parentheses (e.g., ["Metocin", "Methylcybin"])
    alternative_names: Vec<String>,
    /// The original raw name
    raw_name: String,
}

// ============================================================================
// ReagentData - Main Service
// ============================================================================

/// Holds all reagent test data with indexed lookups
#[derive(Debug)]
pub struct ReagentData {
    /// All colors by ID
    colors: HashMap<i32, ReagentColor>,
    /// All reagents by ID
    reagents: HashMap<i32, Reagent>,
    /// Ordered list of reagents (by ID)
    reagent_order: Vec<i32>,
    /// Substance index -> reagent results
    results: HashMap<usize, Vec<ReagentTestResult>>,
    /// Parsed substance names by index
    substances: Vec<ParsedSubstanceName>,
    /// Exact name lookup -> substance index (lowercase)
    name_to_index: HashMap<String, usize>,
    /// Normalized chemical name -> substance index (for fuzzy matching)
    normalized_to_index: HashMap<String, usize>,
    /// Track which alternative names are NOT unique (appear in multiple substances)
    non_unique_alternatives: HashSet<String>,
}

impl ReagentData {
    /// Load reagent data from the JSON file
    pub fn load_from_file(path: &Path) -> Result<Self, ReagentDataError> {
        let contents = std::fs::read_to_string(path).map_err(|e| ReagentDataError::IoError {
            path: path.to_path_buf(),
            source: e,
        })?;

        Self::load_from_str(&contents)
    }

    /// Load reagent data from a JSON string
    pub fn load_from_str(json: &str) -> Result<Self, ReagentDataError> {
        let raw: RawReagentData =
            serde_json::from_str(json).map_err(|e| ReagentDataError::ParseError { source: e })?;

        let mut data = Self {
            colors: HashMap::new(),
            reagents: HashMap::new(),
            reagent_order: Vec::new(),
            results: HashMap::new(),
            substances: Vec::new(),
            name_to_index: HashMap::new(),
            normalized_to_index: HashMap::new(),
            non_unique_alternatives: HashSet::new(),
        };

        // Index colors by ID
        for color in raw.colors {
            data.colors.insert(
                color.id,
                ReagentColor {
                    id: color.id,
                    name: color.name,
                    hex: color.hex,
                    simple: color.simple,
                    simple_color_id: Some(color.simple_color_id),
                },
            );
        }

        // Index reagents by ID and build order
        for reagent in &raw.reagents {
            data.reagent_order.push(reagent.id);
            data.reagents.insert(
                reagent.id,
                Reagent {
                    id: reagent.id,
                    name: reagent.name.clone(),
                    full_name: reagent.full_name.clone(),
                    short_name: reagent.short_name.clone(),
                    white_first_color: reagent.white_first_color,
                },
            );
        }

        // Parse substance names and build initial index
        let mut alt_name_count: HashMap<String, usize> = HashMap::new();

        for substance in &raw.substances {
            let parsed = Self::parse_substance_name(&substance.name);

            // Count alternative name occurrences
            for alt in &parsed.alternative_names {
                let alt_lower = alt.to_lowercase();
                *alt_name_count.entry(alt_lower).or_insert(0) += 1;
            }

            data.substances.push(parsed);
        }

        // Mark non-unique alternative names
        for (name, count) in alt_name_count {
            if count > 1 {
                data.non_unique_alternatives.insert(name);
            }
        }

        // Build name lookup indexes
        for (idx, parsed) in data.substances.iter().enumerate() {
            // Primary chemical name (always unique)
            let chem_lower = parsed.chemical_name.to_lowercase();
            data.name_to_index.insert(chem_lower.clone(), idx);

            // Normalized version (for fuzzy matching)
            let normalized = Self::normalize_chemical_name(&parsed.chemical_name);
            data.normalized_to_index.insert(normalized.clone(), idx);

            // Alternative names (only if unique)
            for alt in &parsed.alternative_names {
                let alt_lower = alt.to_lowercase();
                if !data.non_unique_alternatives.contains(&alt_lower) {
                    data.name_to_index.insert(alt_lower, idx);
                }
            }
        }

        // Process results
        for (substance_idx, substance_results) in raw.results.iter().enumerate() {
            if let Some(reagent_results) = substance_results {
                let mut test_results = Vec::new();

                for (reagent_idx, reagent_result) in reagent_results.iter().enumerate() {
                    if let Some(results_list) = reagent_result {
                        // Get reagent ID from order
                        let reagent_id = data.reagent_order.get(reagent_idx).copied();

                        if let Some(rid) = reagent_id {
                            if let Some(reagent) = data.reagents.get(&rid) {
                                // Process each result for this reagent
                                // (usually just one, but the format allows multiple)
                                for result in results_list {
                                    let start_colors: Vec<ReagentColor> = result
                                        .0
                                        .iter()
                                        .filter_map(|&id| data.colors.get(&id).cloned())
                                        .collect();

                                    let end_colors: Vec<ReagentColor> = result
                                        .1
                                        .iter()
                                        .filter_map(|&id| data.colors.get(&id).cloned())
                                        .collect();

                                    test_results.push(ReagentTestResult {
                                        reagent: reagent.clone(),
                                        start_colors,
                                        end_colors,
                                        is_positive: result.2,
                                        description: result.3.clone(),
                                    });
                                }
                            }
                        }
                    }
                }

                if !test_results.is_empty() {
                    data.results.insert(substance_idx, test_results);
                }
            }
        }

        info!(
            substances = data.substances.len(),
            reagents = data.reagents.len(),
            colors = data.colors.len(),
            non_unique_alts = data.non_unique_alternatives.len(),
            "Reagent data loaded"
        );

        Ok(data)
    }

    /// Parse a substance name to extract chemical name and alternatives
    ///
    /// Examples:
    /// - "4-HO-MET (Metocin, Methylcybin, Colour, ethocin)" ->
    ///   chemical: "4-HO-MET", alts: ["Metocin", "Methylcybin", "Colour", "ethocin"]
    /// - "2C-B (Nexus, Bees)" -> chemical: "2C-B", alts: ["Nexus", "Bees"]
    /// - "LSD" -> chemical: "LSD", alts: []
    fn parse_substance_name(name: &str) -> ParsedSubstanceName {
        // Match pattern: "Chemical Name (alt1, alt2, ...)"
        let re = Regex::new(r"^(.+?)\s*\(([^)]+)\)\s*$").unwrap();

        if let Some(caps) = re.captures(name) {
            let chemical = caps.get(1).map(|m| m.as_str().trim()).unwrap_or(name);
            let alts_str = caps.get(2).map(|m| m.as_str()).unwrap_or("");

            let alternatives: Vec<String> = alts_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            ParsedSubstanceName {
                chemical_name: chemical.to_string(),
                alternative_names: alternatives,
                raw_name: name.to_string(),
            }
        } else {
            ParsedSubstanceName {
                chemical_name: name.to_string(),
                alternative_names: vec![],
                raw_name: name.to_string(),
            }
        }
    }

    /// Normalize a chemical name for fuzzy matching
    ///
    /// Removes hyphens, spaces, and special characters, converting to lowercase.
    /// This allows "4homet", "4-homet", "4-HO-MET" to all match.
    fn normalize_chemical_name(name: &str) -> String {
        name.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    }

    /// Look up reagent results by substance name with fuzzy matching
    ///
    /// Returns None if:
    /// - No match found
    /// - Multiple substances could match (ambiguous)
    pub fn lookup(&self, query: &str) -> Option<SubstanceReagents> {
        let idx = self.find_unique_substance_index(query)?;
        self.get_results_by_index(idx)
    }

    /// Find a unique substance index for a query
    ///
    /// Returns None if ambiguous or not found
    fn find_unique_substance_index(&self, query: &str) -> Option<usize> {
        let query_lower = query.to_lowercase();
        let query_normalized = Self::normalize_chemical_name(query);

        // 1. Exact match on registered names (chemical or unique alternative)
        if let Some(&idx) = self.name_to_index.get(&query_lower) {
            return Some(idx);
        }

        // 2. Normalized fuzzy match on chemical names
        if let Some(&idx) = self.normalized_to_index.get(&query_normalized) {
            // Check if this normalized form is unique
            let matches: Vec<usize> = self
                .substances
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    Self::normalize_chemical_name(&s.chemical_name) == query_normalized
                })
                .map(|(i, _)| i)
                .collect();

            if matches.len() == 1 {
                return Some(idx);
            } else {
                debug!(
                    query = query,
                    matches = matches.len(),
                    "Ambiguous normalized match"
                );
                return None;
            }
        }

        // 3. Try partial/prefix matching (only if unique)
        let mut prefix_matches: Vec<usize> = Vec::new();

        for (idx, parsed) in self.substances.iter().enumerate() {
            let chem_normalized = Self::normalize_chemical_name(&parsed.chemical_name);

            // Check prefix match
            if chem_normalized.starts_with(&query_normalized) {
                prefix_matches.push(idx);
            }
        }

        if prefix_matches.len() == 1 {
            return Some(prefix_matches[0]);
        }

        // 4. Check alternative names with fuzzy matching (only unique ones)
        let mut alt_matches: Vec<usize> = Vec::new();

        for (idx, parsed) in self.substances.iter().enumerate() {
            for alt in &parsed.alternative_names {
                let alt_lower = alt.to_lowercase();

                // Skip non-unique alternatives
                if self.non_unique_alternatives.contains(&alt_lower) {
                    continue;
                }

                let alt_normalized = Self::normalize_chemical_name(alt);

                if alt_normalized == query_normalized
                    || alt_normalized.starts_with(&query_normalized)
                {
                    if !alt_matches.contains(&idx) {
                        alt_matches.push(idx);
                    }
                }
            }
        }

        if alt_matches.len() == 1 {
            return Some(alt_matches[0]);
        }

        debug!(
            query = query,
            prefix_matches = prefix_matches.len(),
            alt_matches = alt_matches.len(),
            "No unique match found"
        );

        None
    }

    /// Get results by substance index
    fn get_results_by_index(&self, idx: usize) -> Option<SubstanceReagents> {
        let parsed = self.substances.get(idx)?;
        let results = self.results.get(&idx).cloned().unwrap_or_default();

        Some(SubstanceReagents {
            substance_name: parsed.chemical_name.clone(),
            raw_name: Some(parsed.raw_name.clone()),
            results,
        })
    }

    /// Batch lookup for multiple queries
    pub fn lookup_many(&self, queries: &[String]) -> Vec<Option<SubstanceReagents>> {
        queries.iter().map(|q| self.lookup(q)).collect()
    }

    /// Get all reagents
    pub fn get_all_reagents(&self) -> Vec<Reagent> {
        self.reagent_order
            .iter()
            .filter_map(|id| self.reagents.get(id).cloned())
            .collect()
    }

    /// Get all colors
    pub fn get_all_colors(&self) -> Vec<ReagentColor> {
        let mut colors: Vec<_> = self.colors.values().cloned().collect();
        colors.sort_by_key(|c| c.id);
        colors
    }

    /// Get count of substances with reagent data
    pub fn substance_count(&self) -> usize {
        self.substances.len()
    }

    /// Check if a substance name query would be ambiguous
    pub fn is_ambiguous(&self, query: &str) -> bool {
        let query_normalized = Self::normalize_chemical_name(query);

        let matches: Vec<usize> = self
            .substances
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                let chem_norm = Self::normalize_chemical_name(&s.chemical_name);
                chem_norm == query_normalized || chem_norm.starts_with(&query_normalized)
            })
            .map(|(i, _)| i)
            .collect();

        matches.len() > 1
    }
}

// ============================================================================
// Thread-safe holder
// ============================================================================

/// Thread-safe holder for ReagentData
#[derive(Clone)]
pub struct ReagentDataHolder(pub Arc<ReagentData>);

impl ReagentDataHolder {
    pub fn new(data: ReagentData) -> Self {
        Self(Arc::new(data))
    }

    pub fn get(&self) -> &ReagentData {
        &self.0
    }
}

impl std::ops::Deref for ReagentDataHolder {
    type Target = ReagentData;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum ReagentDataError {
    #[error("Failed to read reagent data file {path}: {source}")]
    IoError {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to parse reagent data JSON: {source}")]
    ParseError { source: serde_json::Error },
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_substance_name() {
        // Simple name
        let parsed = ReagentData::parse_substance_name("LSD");
        assert_eq!(parsed.chemical_name, "LSD");
        assert!(parsed.alternative_names.is_empty());

        // Name with alternatives
        let parsed = ReagentData::parse_substance_name("4-HO-MET (Metocin, Methylcybin)");
        assert_eq!(parsed.chemical_name, "4-HO-MET");
        assert_eq!(parsed.alternative_names, vec!["Metocin", "Methylcybin"]);

        // Name with single alternative
        let parsed = ReagentData::parse_substance_name("2C-B (Nexus, Bees)");
        assert_eq!(parsed.chemical_name, "2C-B");
        assert_eq!(parsed.alternative_names, vec!["Nexus", "Bees"]);
    }

    #[test]
    fn test_normalize_chemical_name() {
        assert_eq!(ReagentData::normalize_chemical_name("4-HO-MET"), "4homet");
        assert_eq!(ReagentData::normalize_chemical_name("4homet"), "4homet");
        assert_eq!(ReagentData::normalize_chemical_name("4-homet"), "4homet");
        assert_eq!(ReagentData::normalize_chemical_name("2C-B"), "2cb");
        assert_eq!(ReagentData::normalize_chemical_name("LSD"), "lsd");
    }
}
