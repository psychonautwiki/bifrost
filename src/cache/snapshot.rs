//! Snapshot data structures for in-memory substance cache.
//!
//! This module provides the core data structures for holding a complete snapshot
//! of all substances with efficient indexing for all query patterns.

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
    /// Number of trigrams indexed
    pub trigram_count: usize,
}

/// Trigram index for fuzzy text search
#[derive(Debug, Clone)]
pub struct TrigramIndex {
    /// Trigram -> list of (substance_index, score_boost)
    trigrams: HashMap<String, Vec<(usize, f32)>>,
}

impl TrigramIndex {
    /// Create a new empty trigram index
    pub fn new() -> Self {
        Self {
            trigrams: HashMap::new(),
        }
    }

    /// Extract trigrams from a string with padding for edge matching
    fn extract_trigrams(s: &str) -> Vec<String> {
        let normalized = s.to_lowercase();
        // Pad with spaces for edge trigrams
        let padded = format!("  {}  ", normalized);
        let chars: Vec<char> = padded.chars().collect();

        if chars.len() < 3 {
            return vec![];
        }

        chars.windows(3).map(|w| w.iter().collect()).collect()
    }

    /// Insert a text entry into the index
    pub fn insert(&mut self, text: &str, idx: usize) {
        self.insert_with_boost(text, idx, 1.0);
    }

    /// Insert a text entry with a custom score boost
    pub fn insert_with_boost(&mut self, text: &str, idx: usize, boost: f32) {
        for trigram in Self::extract_trigrams(text) {
            self.trigrams.entry(trigram).or_default().push((idx, boost));
        }
    }

    /// Search for entries matching the query
    /// Returns (index, score) pairs sorted by score descending
    pub fn search(&self, query: &str, threshold: f32) -> Vec<(usize, f32)> {
        let query_trigrams = Self::extract_trigrams(query);
        if query_trigrams.is_empty() {
            return vec![];
        }

        let query_len = query_trigrams.len() as f32;
        let mut scores: HashMap<usize, f32> = HashMap::new();

        for trigram in &query_trigrams {
            if let Some(matches) = self.trigrams.get(trigram) {
                for (idx, boost) in matches {
                    *scores.entry(*idx).or_default() += boost;
                }
            }
        }

        // Normalize by query length and filter by threshold
        let mut results: Vec<(usize, f32)> = scores
            .into_iter()
            .map(|(idx, score)| (idx, score / query_len))
            .filter(|(_, score)| *score >= threshold)
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Get the total number of trigrams indexed
    pub fn len(&self) -> usize {
        self.trigrams.len()
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.trigrams.is_empty()
    }
}

impl Default for TrigramIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Complete snapshot of all substances with indexes for efficient queries
#[derive(Debug, Clone)]
pub struct SubstanceSnapshot {
    /// Primary storage - all substances
    pub substances: Vec<Substance>,

    /// Substance name -> index (case-insensitive)
    pub by_name: HashMap<String, usize>,

    /// Chemical class -> sorted indices
    pub by_chemical_class: HashMap<String, Vec<usize>>,

    /// Psychoactive class -> sorted indices
    pub by_psychoactive_class: HashMap<String, Vec<usize>>,

    /// Effect name -> sorted indices (inverted index from substance.effects)
    pub by_effect: HashMap<String, Vec<usize>>,

    /// Trigram index for fuzzy name search
    pub trigram_index: TrigramIndex,

    /// Snapshot metadata
    pub meta: SnapshotMeta,
}

impl SubstanceSnapshot {
    /// Build a new snapshot from a list of substances
    pub fn build(substances: Vec<Substance>) -> Self {
        let start = Instant::now();
        let substance_count = substances.len();

        let mut snapshot = Self {
            substances,
            by_name: HashMap::new(),
            by_chemical_class: HashMap::new(),
            by_psychoactive_class: HashMap::new(),
            by_effect: HashMap::new(),
            trigram_index: TrigramIndex::new(),
            meta: SnapshotMeta {
                created_at: start,
                substance_count,
                build_duration_ms: 0,
                effect_count: 0,
                trigram_count: 0,
            },
        };

        snapshot.rebuild_indexes();

        let build_duration = start.elapsed();
        snapshot.meta.build_duration_ms = build_duration.as_millis() as u64;
        snapshot.meta.effect_count = snapshot.by_effect.len();
        snapshot.meta.trigram_count = snapshot.trigram_index.len();

        info!(
            substances = substance_count,
            effects = snapshot.meta.effect_count,
            trigrams = snapshot.meta.trigram_count,
            duration_ms = snapshot.meta.build_duration_ms,
            "Snapshot built"
        );

        snapshot
    }

    /// Rebuild all indexes from the substances list
    pub fn rebuild_indexes(&mut self) {
        self.by_name.clear();
        self.by_chemical_class.clear();
        self.by_psychoactive_class.clear();
        self.by_effect.clear();
        self.trigram_index = TrigramIndex::new();

        for idx in 0..self.substances.len() {
            // Clone the substance to avoid borrow issues
            let substance = self.substances[idx].clone();
            self.index_substance(idx, &substance);
        }
    }

    /// Index a single substance
    fn index_substance(&mut self, idx: usize, substance: &Substance) {
        // Name index (lowercase for case-insensitive lookup)
        if let Some(name) = &substance.name {
            let name_lower = name.to_lowercase();
            self.by_name.insert(name_lower.clone(), idx);

            // Primary name gets higher boost
            self.trigram_index.insert_with_boost(name, idx, 2.0);

            // Also index common names with normal boost
            if let Some(common_names) = &substance.common_names {
                for cn in common_names {
                    self.trigram_index.insert(cn, idx);
                }
            }

            // Index systematic name with lower boost
            if let Some(systematic) = &substance.systematic_name {
                self.trigram_index.insert_with_boost(systematic, idx, 0.5);
            }
        }

        // Chemical class index
        if let Some(class) = &substance.class {
            if let Some(chemicals) = &class.chemical {
                for c in chemicals {
                    self.by_chemical_class
                        .entry(c.to_lowercase())
                        .or_default()
                        .push(idx);
                }
            }

            // Psychoactive class index
            if let Some(psychoactives) = &class.psychoactive {
                for p in psychoactives {
                    self.by_psychoactive_class
                        .entry(p.to_lowercase())
                        .or_default()
                        .push(idx);
                }
            }
        }

        // Effect index (inverted) - from pre-populated effects field
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

    /// Get a substance by name (case-insensitive)
    pub fn get_by_name(&self, name: &str) -> Option<&Substance> {
        self.by_name
            .get(&name.to_lowercase())
            .map(|&idx| &self.substances[idx])
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

    /// Search substances by fuzzy text match
    pub fn search(&self, query: &str, threshold: f32) -> Vec<&Substance> {
        self.trigram_index
            .search(query, threshold)
            .into_iter()
            .map(|(idx, _score)| &self.substances[idx])
            .collect()
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

    #[test]
    fn test_trigram_extraction() {
        let trigrams = TrigramIndex::extract_trigrams("LSD");
        assert!(!trigrams.is_empty());
        // Should include edge trigrams with padding
        assert!(trigrams.contains(&"  l".to_string()));
        assert!(trigrams.contains(&" ls".to_string()));
        assert!(trigrams.contains(&"lsd".to_string()));
    }

    #[test]
    fn test_trigram_search() {
        let mut index = TrigramIndex::new();
        index.insert("LSD", 0);
        index.insert("Lysergic acid diethylamide", 0);
        index.insert("MDMA", 1);
        index.insert("Cannabis", 2);

        // Exact match should score high
        let results = index.search("LSD", 0.3);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, 0);

        // Partial match
        let results = index.search("Lyser", 0.3);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, 0);
    }

    #[test]
    fn test_snapshot_build() {
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
