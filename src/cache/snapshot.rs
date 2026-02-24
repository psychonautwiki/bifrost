//! Snapshot data structures for in-memory substance cache.
//!
//! This module provides the core data structures for holding a complete snapshot
//! of all substances with efficient indexing for all query patterns.
//!
//! Substance search uses exact and prefix matching against canonical names,
//! common names, and curated aliases (loaded from data/substance_aliases.json).
//! No fuzzy/trigram matching is used for the top-level substance query.

use crate::graphql::model::{Effect, Substance, SubstanceImage};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Metadata about the snapshot
#[derive(Debug, Clone)]
pub struct SnapshotMeta {
    /// When this snapshot was created
    pub created_at: Instant,
    /// Number of substances in the snapshot
    pub substance_count: usize,
    /// How long it took to build the indexes
    pub build_duration_ms: u64,
    /// Number of effects indexed
    pub effect_count: usize,
    /// Number of aliases indexed
    pub alias_count: usize,
}

/// Alias data loaded from substance_aliases.json
/// Maps canonical substance name -> list of alternative names
#[derive(Debug, Clone, Default)]
pub struct SubstanceAliases {
    /// Canonical substance name -> list of aliases
    pub aliases: HashMap<String, Vec<String>>,
}

impl SubstanceAliases {
    /// Load aliases from a JSON file
    pub fn load_from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let parsed: serde_json::Value = serde_json::from_str(&content)?;

        let mut aliases = HashMap::new();

        if let Some(alias_map) = parsed.get("aliases").and_then(|v| v.as_object()) {
            for (substance_name, alias_list) in alias_map {
                if let Some(arr) = alias_list.as_array() {
                    let names: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    if !names.is_empty() {
                        aliases.insert(substance_name.clone(), names);
                    }
                }
            }
        }

        info!(
            substances_with_aliases = aliases.len(),
            total_aliases = aliases.values().map(|v| v.len()).sum::<usize>(),
            "Loaded substance aliases"
        );

        Ok(Self { aliases })
    }

    /// Create empty aliases (no file available)
    pub fn empty() -> Self {
        Self {
            aliases: HashMap::new(),
        }
    }

    /// Merge wiki redirect data into this alias set.
    /// Redirects are added as aliases only if they don't already exist.
    /// Filters out case-only duplicates, subpages, talk pages, and botany suffixes.
    pub fn merge_redirects(&mut self, redirects: &HashMap<String, Vec<String>>) {
        let mut added = 0usize;
        let mut skipped_curated = 0usize;

        // Build a set of all aliases already claimed by curated data
        // (across ALL substances). If a wiki redirect tries to map an alias
        // to a different substance than the curated data says, skip it.
        let mut curated_alias_to_target: HashMap<String, String> = HashMap::new();
        for (substance, alias_list) in &self.aliases {
            for alias in alias_list {
                curated_alias_to_target.insert(alias.to_lowercase(), substance.clone());
            }
        }

        for (target, sources) in redirects {
            let target_lower = target.to_lowercase();
            let existing = self.aliases.entry(target.clone()).or_default();
            let existing_lower: std::collections::HashSet<String> =
                existing.iter().map(|a| a.to_lowercase()).collect();

            for source in sources {
                // Skip problematic entries
                if source.starts_with("Talk:")
                    || source.starts_with("File:")
                    || source.starts_with("Project talk:")
                    || source.contains('/')
                    || source.ends_with("(Botany)")
                    || source.ends_with("(botany)")
                    || source.ends_with("(Mycology)")
                    || source.ends_with("(mycology)")
                {
                    continue;
                }

                let source_lower = source.to_lowercase();

                // Skip case-only duplicates of the target
                if source_lower == target_lower {
                    continue;
                }

                // Skip if already present in this target's alias list
                if existing_lower.contains(&source_lower) {
                    continue;
                }

                // Skip if this alias is already curated for a DIFFERENT substance.
                // Curated data always takes priority over wiki redirects.
                if let Some(curated_target) = curated_alias_to_target.get(&source_lower) {
                    if curated_target.to_lowercase() != target_lower {
                        skipped_curated += 1;
                        continue;
                    }
                }

                existing.push(source.clone());
                added += 1;
            }
        }

        info!(
            new_aliases = added,
            skipped_curated_conflicts = skipped_curated,
            total_substances = self.aliases.len(),
            total_aliases = self.aliases.values().map(|v| v.len()).sum::<usize>(),
            "Merged wiki redirects into aliases"
        );
    }

    /// Save the merged alias data to a cache file for faster subsequent loads
    pub fn save_redirect_cache(
        redirects: &HashMap<String, Vec<String>>,
        path: &std::path::Path,
    ) -> anyhow::Result<()> {
        let output = serde_json::json!({
            "_meta": {
                "description": "Cached PsychonautWiki redirect mappings",
                "cached_at": chrono::Utc::now().to_rfc3339(),
                "total_targets": redirects.len(),
                "total_redirects": redirects.values().map(|v| v.len()).sum::<usize>(),
            },
            "redirects": redirects,
        });
        let content = serde_json::to_string_pretty(&output)?;
        std::fs::write(path, content)?;
        info!(path = %path.display(), "Saved redirect cache");
        Ok(())
    }

    /// Load cached redirect data
    pub fn load_redirect_cache(
        path: &std::path::Path,
    ) -> anyhow::Result<HashMap<String, Vec<String>>> {
        let content = std::fs::read_to_string(path)?;
        let parsed: serde_json::Value = serde_json::from_str(&content)?;

        let mut redirects = HashMap::new();
        if let Some(redirect_map) = parsed.get("redirects").and_then(|v| v.as_object()) {
            for (target, sources) in redirect_map {
                if let Some(arr) = sources.as_array() {
                    let names: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    if !names.is_empty() {
                        redirects.insert(target.clone(), names);
                    }
                }
            }
        }

        info!(
            targets = redirects.len(),
            total_redirects = redirects.values().map(|v| v.len()).sum::<usize>(),
            path = %path.display(),
            "Loaded redirect cache"
        );

        Ok(redirects)
    }
}

/// Complete snapshot of all substances with indexes for efficient queries
#[derive(Debug, Clone)]
pub struct SubstanceSnapshot {
    /// Primary storage - all substances
    pub substances: Vec<Substance>,

    /// Substance name -> index (case-insensitive)
    pub by_name: HashMap<String, usize>,

    /// Alias -> index (case-insensitive). Maps alternative names/abbreviations
    /// to substance indices. Populated from substance_aliases.json + common_names.
    pub by_alias: HashMap<String, usize>,

    /// Chemical class -> sorted indices
    pub by_chemical_class: HashMap<String, Vec<usize>>,

    /// Psychoactive class -> sorted indices
    pub by_psychoactive_class: HashMap<String, Vec<usize>>,

    /// Effect name -> sorted indices (inverted index from substance.effects)
    pub by_effect: HashMap<String, Vec<usize>>,

    /// Curated alias data (kept for rebuilding indexes)
    pub alias_data: SubstanceAliases,

    /// Snapshot metadata
    pub meta: SnapshotMeta,
}

impl SubstanceSnapshot {
    /// Build a new snapshot from a list of substances with alias data
    pub fn build(substances: Vec<Substance>) -> Self {
        Self::build_with_aliases(substances, SubstanceAliases::empty())
    }

    /// Build a new snapshot from a list of substances with curated aliases
    pub fn build_with_aliases(substances: Vec<Substance>, alias_data: SubstanceAliases) -> Self {
        let start = Instant::now();
        let substance_count = substances.len();

        let mut snapshot = Self {
            substances,
            by_name: HashMap::new(),
            by_alias: HashMap::new(),
            by_chemical_class: HashMap::new(),
            by_psychoactive_class: HashMap::new(),
            by_effect: HashMap::new(),
            alias_data,
            meta: SnapshotMeta {
                created_at: start,
                substance_count,
                build_duration_ms: 0,
                effect_count: 0,
                alias_count: 0,
            },
        };

        snapshot.rebuild_indexes();

        let build_duration = start.elapsed();
        snapshot.meta.build_duration_ms = build_duration.as_millis() as u64;
        snapshot.meta.effect_count = snapshot.by_effect.len();
        snapshot.meta.alias_count = snapshot.by_alias.len();

        info!(
            substances = substance_count,
            effects = snapshot.meta.effect_count,
            aliases = snapshot.meta.alias_count,
            duration_ms = snapshot.meta.build_duration_ms,
            "Snapshot built"
        );

        snapshot
    }

    /// Rebuild all indexes from the substances list.
    ///
    /// Index priority (highest first):
    /// 1. Canonical substance names → `by_name` (always wins)
    /// 2. Curated aliases from `alias_data` → `by_alias` (manual review, highest alias priority)
    /// 3. `common_names` from wiki data → `by_alias` (only if slot not already taken)
    /// 4. `systematic_name` from wiki data → `by_alias` (lowest priority)
    ///
    /// This ensures that manually curated aliases always take precedence over
    /// automatically extracted wiki data (common_names, systematic_name).
    pub fn rebuild_indexes(&mut self) {
        self.by_name.clear();
        self.by_alias.clear();
        self.by_chemical_class.clear();
        self.by_psychoactive_class.clear();
        self.by_effect.clear();

        // Phase 1: Index all canonical names into by_name
        for (idx, substance) in self.substances.iter().enumerate() {
            if let Some(name) = &substance.name {
                self.by_name.insert(name.to_lowercase(), idx);
            }
        }

        // Phase 2: Index curated aliases (highest priority for by_alias)
        let alias_data = self.alias_data.clone();
        for (substance_name, aliases) in &alias_data.aliases {
            if let Some(&idx) = self.by_name.get(&substance_name.to_lowercase()) {
                for alias in aliases {
                    let alias_lower = alias.to_lowercase();
                    // Don't overwrite canonical names
                    if !self.by_name.contains_key(&alias_lower) {
                        // Curated aliases ALWAYS win - overwrite any existing alias entry
                        self.by_alias.insert(alias_lower, idx);
                    }
                }
            }
        }

        // Phase 3: Index common_names and systematic_name (lower priority, don't overwrite)
        for (idx, substance) in self.substances.iter().enumerate() {
            if substance.name.is_none() {
                continue;
            }

            // common_names → alias (only if slot not taken by curated alias or canonical name)
            if let Some(common_names) = &substance.common_names {
                for cn in common_names {
                    let cn_lower = cn.to_lowercase();
                    if !self.by_alias.contains_key(&cn_lower)
                        && !self.by_name.contains_key(&cn_lower)
                    {
                        self.by_alias.insert(cn_lower, idx);
                    }
                }
            }

            // systematic_name → alias (lowest priority)
            if let Some(systematic) = &substance.systematic_name {
                let sys_lower = systematic.to_lowercase();
                if !self.by_alias.contains_key(&sys_lower)
                    && !self.by_name.contains_key(&sys_lower)
                {
                    self.by_alias.insert(sys_lower, idx);
                }
            }
        }

        // Phase 4: Index class and effect data
        for (idx, substance) in self.substances.iter().enumerate() {
            if let Some(class) = &substance.class {
                if let Some(chemicals) = &class.chemical {
                    for c in chemicals {
                        self.by_chemical_class
                            .entry(c.to_lowercase())
                            .or_default()
                            .push(idx);
                    }
                }

                if let Some(psychoactives) = &class.psychoactive {
                    for p in psychoactives {
                        self.by_psychoactive_class
                            .entry(p.to_lowercase())
                            .or_default()
                            .push(idx);
                    }
                }
            }

            if let Some(effects) = &substance.effects_cache {
                for effect in effects {
                    if let Some(name) = &effect.name {
                        self.by_effect
                            .entry(name.to_lowercase())
                            .or_default()
                            .push(idx);
                    }
                }
            }
        }
    }

    /// Get a substance by name (case-insensitive)
    pub fn get_by_name(&self, name: &str) -> Option<&Substance> {
        self.by_name
            .get(&name.to_lowercase())
            .map(|&idx| &self.substances[idx])
    }

    /// Get a substance by name or alias (case-insensitive)
    /// Tries exact name match first, then alias match
    pub fn get_by_name_or_alias(&self, query: &str) -> Option<&Substance> {
        let query_lower = query.to_lowercase();

        // 1. Exact name match
        if let Some(&idx) = self.by_name.get(&query_lower) {
            return Some(&self.substances[idx]);
        }

        // 2. Alias match
        if let Some(&idx) = self.by_alias.get(&query_lower) {
            return Some(&self.substances[idx]);
        }

        None
    }

    /// Search substances by exact name/alias match, then prefix match.
    ///
    /// Search priority:
    /// 1. Exact match on canonical name (case-insensitive)
    /// 2. Exact match on alias (case-insensitive)
    /// 3. Prefix match on canonical name (case-insensitive)
    /// 4. Prefix match on alias (case-insensitive)
    ///
    /// Returns results deduplicated and ordered by match quality.
    pub fn search(&self, query: &str) -> Vec<&Substance> {
        let query_lower = query.to_lowercase();

        if query_lower.is_empty() {
            return vec![];
        }

        let mut seen = std::collections::HashSet::new();
        let mut results = Vec::new();

        // 1. Exact match on canonical name → return ONLY this substance
        //    If the query matches a canonical substance name, that is the
        //    definitive answer. We do NOT also check aliases because another
        //    substance might have this string as a common_name/alias, and
        //    we don't want to return both.
        if let Some(&idx) = self.by_name.get(&query_lower) {
            return vec![&self.substances[idx]];
        }

        // 2. Exact match on alias → return ONLY the aliased substance
        if let Some(&idx) = self.by_alias.get(&query_lower) {
            return vec![&self.substances[idx]];
        }

        // 3. Prefix match on canonical names
        for (name, &idx) in &self.by_name {
            if name.starts_with(&query_lower) {
                if seen.insert(idx) {
                    results.push(&self.substances[idx]);
                }
            }
        }

        // 4. Prefix match on aliases
        for (alias, &idx) in &self.by_alias {
            if alias.starts_with(&query_lower) {
                if seen.insert(idx) {
                    results.push(&self.substances[idx]);
                }
            }
        }

        // Sort prefix matches alphabetically by name for deterministic results
        if results.len() > 1 {
            results.sort_by(|a, b| {
                let name_a = a.name.as_deref().unwrap_or("");
                let name_b = b.name.as_deref().unwrap_or("");
                name_a.to_lowercase().cmp(&name_b.to_lowercase())
            });
        }

        results
    }

    /// Get substances by chemical class
    pub fn get_by_chemical_class(&self, class: &str) -> Vec<&Substance> {
        self.by_chemical_class
            .get(&class.to_lowercase())
            .map(|indices| indices.iter().map(|&idx| &self.substances[idx]).collect())
            .unwrap_or_default()
    }

    /// Get substances by psychoactive class
    pub fn get_by_psychoactive_class(&self, class: &str) -> Vec<&Substance> {
        self.by_psychoactive_class
            .get(&class.to_lowercase())
            .map(|indices| indices.iter().map(|&idx| &self.substances[idx]).collect())
            .unwrap_or_default()
    }

    /// Get substances by effect name
    pub fn get_by_effect(&self, effect: &str) -> Vec<&Substance> {
        self.by_effect
            .get(&effect.to_lowercase())
            .map(|indices| indices.iter().map(|&idx| &self.substances[idx]).collect())
            .unwrap_or_default()
    }

    /// Get substances matching multiple effects (union/OR)
    pub fn get_by_effects(&self, effects: &[String]) -> Vec<&Substance> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();

        for effect in effects {
            if let Some(indices) = self.by_effect.get(&effect.to_lowercase()) {
                for &idx in indices {
                    if seen.insert(idx) {
                        result.push(&self.substances[idx]);
                    }
                }
            }
        }

        result
    }

    /// Get all substances with pagination
    pub fn get_all(&self, limit: usize, offset: usize) -> Vec<&Substance> {
        self.substances.iter().skip(offset).take(limit).collect()
    }

    /// Update a substance in place and rebuild indexes
    pub fn update_substance(&mut self, name: &str, new_substance: Substance) {
        if let Some(&idx) = self.by_name.get(&name.to_lowercase()) {
            self.substances[idx] = new_substance;
            self.rebuild_indexes();
        }
    }

    /// Add a new substance and rebuild indexes
    pub fn add_substance(&mut self, substance: Substance) {
        self.substances.push(substance);
        self.meta.substance_count = self.substances.len();
        self.rebuild_indexes();
    }

    /// Remove a substance by name and rebuild indexes
    pub fn remove_substance(&mut self, name: &str) -> bool {
        if let Some(&idx) = self.by_name.get(&name.to_lowercase()) {
            self.substances.remove(idx);
            self.meta.substance_count = self.substances.len();
            self.rebuild_indexes();
            true
        } else {
            false
        }
    }

    /// Resolve interaction names to substances (returns stubs for missing)
    pub fn resolve_interactions(&self, names: &[String]) -> Vec<Substance> {
        names
            .iter()
            .map(|name| {
                if let Some(&idx) = self.by_name.get(&name.to_lowercase()) {
                    self.substances[idx].clone()
                } else {
                    // Return stub for missing substances
                    Substance {
                        name: Some(name.clone()),
                        url: Some(format!(
                            "https://psychonautwiki.org/wiki/{}",
                            urlencoding::encode(name)
                        )),
                        ..Default::default()
                    }
                }
            })
            .collect()
    }

    /// Get effects for a substance by name
    pub fn get_effects_for_substance(&self, substance_name: &str) -> Vec<Effect> {
        self.get_by_name(substance_name)
            .and_then(|s| s.effects_cache.clone())
            .unwrap_or_default()
    }

    /// Get summary for a substance by name
    pub fn get_summary_for_substance(&self, substance_name: &str) -> Option<String> {
        self.get_by_name(substance_name)
            .and_then(|s| s.summary_cache.clone())
    }

    /// Get images for a substance by name
    pub fn get_images_for_substance(&self, substance_name: &str) -> Option<Vec<SubstanceImage>> {
        self.get_by_name(substance_name)
            .and_then(|s| s.images_cache.clone())
    }
}

/// Thread-safe holder for the current snapshot with atomic swap
#[derive(Clone)]
pub struct SnapshotHolder {
    current: Arc<RwLock<Arc<SubstanceSnapshot>>>,
}

impl SnapshotHolder {
    /// Create a new snapshot holder with the given initial snapshot
    pub fn new(snapshot: SubstanceSnapshot) -> Self {
        Self {
            current: Arc::new(RwLock::new(Arc::new(snapshot))),
        }
    }

    /// Get the current snapshot (readers acquire Arc clone - never blocked)
    pub async fn get(&self) -> Arc<SubstanceSnapshot> {
        self.current.read().await.clone()
    }

    /// Atomically swap to a new snapshot
    pub async fn swap(&self, new_snapshot: SubstanceSnapshot) {
        let new_arc = Arc::new(new_snapshot);
        let mut guard = self.current.write().await;
        debug!(
            old_count = guard.meta.substance_count,
            new_count = new_arc.meta.substance_count,
            "Swapping snapshot"
        );
        *guard = new_arc;
        // Old snapshot dropped when last reader releases Arc
    }

    /// Get mutable access to the current snapshot (for in-place updates)
    /// This clones the snapshot, modifies it, and swaps
    pub async fn modify<F>(&self, f: F)
    where
        F: FnOnce(&mut SubstanceSnapshot),
    {
        let current = self.get().await;
        let mut new_snapshot = (*current).clone();
        f(&mut new_snapshot);
        self.swap(new_snapshot).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_substance(name: &str) -> Substance {
        Substance {
            name: Some(name.to_string()),
            ..Default::default()
        }
    }

    fn make_test_aliases() -> SubstanceAliases {
        let mut aliases = HashMap::new();
        aliases.insert(
            "LSD".to_string(),
            vec![
                "Acid".to_string(),
                "LSD-25".to_string(),
                "Lucy".to_string(),
                "Lysergic acid diethylamide".to_string(),
            ],
        );
        aliases.insert(
            "MDMA".to_string(),
            vec![
                "Ecstasy".to_string(),
                "Molly".to_string(),
                "XTC".to_string(),
            ],
        );
        SubstanceAliases { aliases }
    }

    #[test]
    fn test_exact_name_search() {
        let substances = vec![
            make_test_substance("LSD"),
            make_test_substance("LSA"),
            make_test_substance("MDMA"),
            make_test_substance("Cannabis"),
        ];

        let snapshot = SubstanceSnapshot::build_with_aliases(substances, make_test_aliases());

        // Exact match: "LSD" should return only LSD
        let results = snapshot.search("LSD");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name.as_deref(), Some("LSD"));

        // Exact match case-insensitive
        let results = snapshot.search("lsd");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name.as_deref(), Some("LSD"));
    }

    #[test]
    fn test_alias_search() {
        let substances = vec![
            make_test_substance("LSD"),
            make_test_substance("MDMA"),
            make_test_substance("Cannabis"),
        ];

        let snapshot = SubstanceSnapshot::build_with_aliases(substances, make_test_aliases());

        // Alias match: "Acid" should return LSD
        let results = snapshot.search("Acid");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name.as_deref(), Some("LSD"));

        // Alias match: "Molly" should return MDMA
        let results = snapshot.search("Molly");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name.as_deref(), Some("MDMA"));

        // Alias match: "ecstasy" (case-insensitive) should return MDMA
        let results = snapshot.search("ecstasy");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name.as_deref(), Some("MDMA"));
    }

    #[test]
    fn test_prefix_search() {
        let substances = vec![
            make_test_substance("LSD"),
            make_test_substance("LSA"),
            make_test_substance("LSM-775"),
            make_test_substance("LSZ"),
            make_test_substance("MDMA"),
        ];

        let snapshot = SubstanceSnapshot::build_with_aliases(substances, make_test_aliases());

        // Prefix match: "LS" should return LSD, LSA, LSM-775, LSZ
        let results = snapshot.search("LS");
        assert_eq!(results.len(), 4);
        let names: Vec<&str> = results
            .iter()
            .filter_map(|s| s.name.as_deref())
            .collect();
        assert!(names.contains(&"LSD"));
        assert!(names.contains(&"LSA"));
        assert!(names.contains(&"LSM-775"));
        assert!(names.contains(&"LSZ"));
    }

    #[test]
    fn test_exact_match_takes_priority_over_prefix() {
        let substances = vec![
            make_test_substance("LSD"),
            make_test_substance("LSA"),
        ];

        let snapshot = SubstanceSnapshot::build_with_aliases(substances, make_test_aliases());

        // "LSD" is an exact match, should return ONLY LSD (not LSA via prefix)
        let results = snapshot.search("LSD");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name.as_deref(), Some("LSD"));
    }

    #[test]
    fn test_no_match() {
        let substances = vec![
            make_test_substance("LSD"),
            make_test_substance("MDMA"),
        ];

        let snapshot = SubstanceSnapshot::build_with_aliases(substances, make_test_aliases());

        // No match
        let results = snapshot.search("Aspirin");
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_snapshot_build_basic() {
        let substances = vec![
            Substance {
                name: Some("LSD".to_string()),
                class: Some(crate::graphql::model::SubstanceClass {
                    chemical: Some(vec!["Lysergamide".to_string()]),
                    psychoactive: Some(vec!["Psychedelic".to_string()]),
                }),
                ..Default::default()
            },
            Substance {
                name: Some("MDMA".to_string()),
                class: Some(crate::graphql::model::SubstanceClass {
                    chemical: Some(vec!["Phenethylamine".to_string()]),
                    psychoactive: Some(vec!["Entactogen".to_string()]),
                }),
                ..Default::default()
            },
        ];

        let snapshot = SubstanceSnapshot::build(substances);
        assert_eq!(snapshot.meta.substance_count, 2);
        assert!(snapshot.get_by_name("lsd").is_some());
        assert!(snapshot.get_by_name("LSD").is_some()); // Case insensitive
        assert!(!snapshot.get_by_chemical_class("lysergamide").is_empty());
    }
}
