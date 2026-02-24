//! GraphQL schema for bifrost.
//!
//! All queries read from the in-memory snapshot - no backend calls are made.
//! The snapshot is kept fresh by the background revalidator.

use crate::cache::snapshot::SnapshotHolder;
use crate::graphql::model::*;
use crate::metrics::SharedMetrics;
use crate::services::plebiscite::PlebisciteService;
use crate::services::reagents::ReagentDataHolder;
use async_graphql::{ComplexObject, Context, EmptyMutation, EmptySubscription, Object, Schema};
use std::sync::Arc;
use std::time::Instant;

pub type BifrostSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

/// Create the GraphQL schema with snapshot-based queries
pub fn create_schema(
    snapshot: SnapshotHolder,
    plebiscite_service: Option<Arc<PlebisciteService>>,
    reagent_data: Option<ReagentDataHolder>,
    metrics: SharedMetrics,
) -> BifrostSchema {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(snapshot)
        .data(plebiscite_service)
        .data(reagent_data)
        .data(metrics)
        .finish()
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Query substances with optional filters.
    /// All filters are mutually exclusive - only one can be specified at a time.
    async fn substances(
        &self,
        ctx: &Context<'_>,
        effect: Option<String>,
        query: Option<String>,
        chemical_class: Option<String>,
        psychoactive_class: Option<String>,
        #[graphql(default = 10)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> async_graphql::Result<Vec<Substance>> {
        let start = Instant::now();
        let snapshot_holder = ctx.data::<SnapshotHolder>()?;
        let metrics = ctx.data::<SharedMetrics>()?;

        // Validate mutual exclusivity
        let params = [&effect, &query, &chemical_class, &psychoactive_class];
        if params.iter().filter(|p| p.is_some()).count() >= 2 {
            return Err(async_graphql::Error::new(
                "Parameters are mutually exclusive: effect, query, chemical_class, psychoactive_class",
            ));
        }

        let snapshot = snapshot_holder.get().await;
        let limit = limit.max(0) as usize;
        let offset = offset.max(0) as usize;

        let results: Vec<Substance> = if let Some(c) = chemical_class {
            snapshot
                .get_by_chemical_class(&c)
                .into_iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect()
        } else if let Some(p) = psychoactive_class {
            snapshot
                .get_by_psychoactive_class(&p)
                .into_iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect()
        } else if let Some(e) = effect {
            snapshot
                .get_by_effect(&e)
                .into_iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect()
        } else if let Some(q) = query {
            if q.is_empty() {
                snapshot
                    .get_all(limit, offset)
                    .into_iter()
                    .cloned()
                    .collect()
            } else {
                // Exact + prefix search against names and aliases
                snapshot
                    .search(&q)
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .cloned()
                    .collect()
            }
        } else {
            // No filter - return all with pagination
            snapshot
                .get_all(limit, offset)
                .into_iter()
                .cloned()
                .collect()
        };

        let duration = start.elapsed().as_secs_f64();
        metrics.record_query("substances", "success", duration, results.len() as u64);

        Ok(results)
    }

    /// Query substances by effect(s). Multiple effects are OR-matched.
    async fn substances_by_effect(
        &self,
        ctx: &Context<'_>,
        effect: Option<Vec<String>>,
        #[graphql(default = 50)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> async_graphql::Result<Vec<Substance>> {
        let start = Instant::now();
        let snapshot_holder = ctx.data::<SnapshotHolder>()?;
        let metrics = ctx.data::<SharedMetrics>()?;

        let effects = match effect {
            Some(e) if !e.is_empty() => e,
            _ => {
                metrics.record_query("substances_by_effect", "success", 0.0, 0);
                return Ok(vec![]);
            }
        };

        let snapshot = snapshot_holder.get().await;
        let limit = limit.max(0) as usize;
        let offset = offset.max(0) as usize;

        let results: Vec<Substance> = snapshot
            .get_by_effects(&effects)
            .into_iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();

        let duration = start.elapsed().as_secs_f64();
        metrics.record_query(
            "substances_by_effect",
            "success",
            duration,
            results.len() as u64,
        );

        Ok(results)
    }

    /// Query effects for a given substance.
    async fn effects_by_substance(
        &self,
        ctx: &Context<'_>,
        substance: String,
        #[graphql(default = 50)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> async_graphql::Result<Vec<Effect>> {
        let start = Instant::now();
        let snapshot_holder = ctx.data::<SnapshotHolder>()?;
        let metrics = ctx.data::<SharedMetrics>()?;

        let snapshot = snapshot_holder.get().await;
        let limit = limit.max(0) as usize;
        let offset = offset.max(0) as usize;

        let results: Vec<Effect> = snapshot
            .get_effects_for_substance(&substance)
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect();

        let duration = start.elapsed().as_secs_f64();
        metrics.record_query(
            "effects_by_substance",
            "success",
            duration,
            results.len() as u64,
        );

        Ok(results)
    }

    /// Query Erowid experience reports (requires PLEBISCITE feature)
    async fn erowid(
        &self,
        ctx: &Context<'_>,
        substance: Option<String>,
        #[graphql(default = 50)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> async_graphql::Result<Option<Vec<ErowidExperience>>> {
        let start = Instant::now();
        let metrics = ctx.data::<SharedMetrics>()?;
        let service_opt = ctx.data::<Option<Arc<PlebisciteService>>>()?;

        if let Some(service) = service_opt {
            let results = service.find(substance, Some(offset), Some(limit)).await?;
            let duration = start.elapsed().as_secs_f64();
            metrics.record_query("erowid", "success", duration, results.len() as u64);
            Ok(Some(results))
        } else {
            metrics.record_query("erowid", "disabled", 0.0, 0);
            Ok(None)
        }
    }

    /// Legacy experiences query (stub for API compatibility)
    async fn experiences(
        &self,
        ctx: &Context<'_>,
        substances_by_effect: Option<String>,
        effects_by_substance: Option<String>,
        substance: Option<String>,
    ) -> async_graphql::Result<Vec<Experience>> {
        let start = Instant::now();
        let snapshot_holder = ctx.data::<SnapshotHolder>()?;
        let metrics = ctx.data::<SharedMetrics>()?;

        let snapshot = snapshot_holder.get().await;

        let mut result = Experience {
            substances: None,
            effects: None,
        };

        // Get substances by effect
        if let Some(effect) = substances_by_effect {
            let substances: Vec<Substance> = snapshot
                .get_by_effect(&effect)
                .into_iter()
                .take(50)
                .cloned()
                .collect();
            result.substances = Some(substances);
        }

        // Get effects by substance
        if let Some(sub) = effects_by_substance {
            let effects = snapshot.get_effects_for_substance(&sub);
            result.effects = Some(effects);
        }

        // Get substance directly
        if let Some(sub) = substance {
            if let Some(s) = snapshot.get_by_name(&sub) {
                result.substances = Some(vec![s.clone()]);
            }
        }

        let duration = start.elapsed().as_secs_f64();
        metrics.record_query("experiences", "success", duration, 1);

        Ok(vec![result])
    }

    /// Query reagent test results for substances.
    ///
    /// Accepts either a single substance name or an array of names.
    /// Names are fuzzy-matched: "4homet", "4-homet", "4-HO-MET" all match.
    /// Returns None for a query if:
    /// - No match found
    /// - Multiple substances could match (ambiguous)
    async fn reagent_results(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Single substance name to query")] substance: Option<String>,
        #[graphql(desc = "Multiple substance names to query")] substances: Option<Vec<String>>,
    ) -> async_graphql::Result<Vec<ReagentQueryResult>> {
        let start = Instant::now();
        let metrics = ctx.data::<SharedMetrics>()?;
        let reagent_data_opt = ctx.data::<Option<ReagentDataHolder>>()?;

        let reagent_data = match reagent_data_opt {
            Some(data) => data,
            None => {
                metrics.record_query("reagent_results", "disabled", 0.0, 0);
                return Ok(vec![]);
            }
        };

        // Collect all queries
        let queries: Vec<String> = match (substance, substances) {
            (Some(s), None) => vec![s],
            (None, Some(ss)) => ss,
            (Some(s), Some(mut ss)) => {
                ss.insert(0, s);
                ss
            }
            (None, None) => vec![],
        };

        if queries.is_empty() {
            metrics.record_query("reagent_results", "success", 0.0, 0);
            return Ok(vec![]);
        }

        let mut results = Vec::new();

        for query in queries {
            if let Some(substance_reagents) = reagent_data.lookup(&query) {
                results.push(ReagentQueryResult {
                    query: query.clone(),
                    matched_name: substance_reagents.substance_name.clone(),
                    results: substance_reagents.results,
                });
            }
        }

        let duration = start.elapsed().as_secs_f64();
        metrics.record_query("reagent_results", "success", duration, results.len() as u64);

        Ok(results)
    }

    /// List all available reagent tests
    async fn reagents(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Reagent>> {
        let reagent_data_opt = ctx.data::<Option<ReagentDataHolder>>()?;

        match reagent_data_opt {
            Some(data) => Ok(data.get_all_reagents()),
            None => Ok(vec![]),
        }
    }

    /// List all reagent colors
    async fn reagent_colors(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<ReagentColor>> {
        let reagent_data_opt = ctx.data::<Option<ReagentDataHolder>>()?;

        match reagent_data_opt {
            Some(data) => Ok(data.get_all_colors()),
            None => Ok(vec![]),
        }
    }
}

#[ComplexObject]
impl Substance {
    /// Get effects for this substance (from cached data)
    async fn effects(&self, _ctx: &Context<'_>) -> async_graphql::Result<Vec<Effect>> {
        // Return pre-cached effects
        Ok(self.effects_cache.clone().unwrap_or_default())
    }

    /// Get summary for this substance (from cached data)
    async fn summary(&self, _ctx: &Context<'_>) -> async_graphql::Result<Option<String>> {
        // Return pre-cached summary
        Ok(self.summary_cache.clone())
    }

    /// Get images for this substance (from cached data)
    async fn images(
        &self,
        _ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<Vec<SubstanceImage>>> {
        // Return pre-cached images
        Ok(self.images_cache.clone())
    }

    /// Get uncertain interactions (resolved from snapshot)
    async fn uncertain_interactions(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<Vec<Substance>>> {
        self.resolve_interactions(ctx, &self.uncertain_interactions_raw)
            .await
    }

    /// Get unsafe interactions (resolved from snapshot)
    async fn unsafe_interactions(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<Vec<Substance>>> {
        self.resolve_interactions(ctx, &self.unsafe_interactions_raw)
            .await
    }

    /// Get dangerous interactions (resolved from snapshot)
    async fn dangerous_interactions(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<Vec<Substance>>> {
        self.resolve_interactions(ctx, &self.dangerous_interactions_raw)
            .await
    }

    /// Get reagent test results for this substance
    ///
    /// Uses fuzzy matching to find the substance in the reagent database.
    /// Returns None if the substance is not found or if the match is ambiguous.
    async fn reagents(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<SubstanceReagents>> {
        let name = match &self.name {
            Some(n) => n,
            None => return Ok(None),
        };

        let reagent_data_opt = ctx.data::<Option<ReagentDataHolder>>()?;

        match reagent_data_opt {
            Some(data) => Ok(data.lookup(name)),
            None => Ok(None),
        }
    }
}

impl Substance {
    /// Resolve interaction names to substances using the snapshot
    async fn resolve_interactions(
        &self,
        ctx: &Context<'_>,
        raw: &Option<Vec<String>>,
    ) -> async_graphql::Result<Option<Vec<Substance>>> {
        let names = match raw {
            Some(n) if !n.is_empty() => n,
            _ => return Ok(None),
        };

        let snapshot_holder = ctx.data::<SnapshotHolder>()?;
        let snapshot = snapshot_holder.get().await;

        let resolved = snapshot.resolve_interactions(names);
        Ok(Some(resolved))
    }
}

#[ComplexObject]
impl Effect {
    /// Get substances that produce this effect (from snapshot index)
    async fn substances(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Substance>> {
        let name = match &self.name {
            Some(n) => n,
            None => return Ok(vec![]),
        };

        let snapshot_holder = ctx.data::<SnapshotHolder>()?;
        let snapshot = snapshot_holder.get().await;

        let results: Vec<Substance> = snapshot
            .get_by_effect(name)
            .into_iter()
            .take(50)
            .cloned()
            .collect();

        Ok(results)
    }
}

#[ComplexObject]
impl SubstanceReagents {
    /// Get the linked PsychonautWiki substance for this reagent entry.
    ///
    /// Uses exact name/alias matching to find the substance in the
    /// PsychonautWiki database. Returns None if no match is found.
    async fn pw_substance(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<Substance>> {
        let snapshot_holder = ctx.data::<SnapshotHolder>()?;
        let snapshot = snapshot_holder.get().await;

        // Try exact name or alias match
        if let Some(s) = snapshot.get_by_name_or_alias(&self.substance_name) {
            return Ok(Some(s.clone()));
        }

        Ok(None)
    }
}

#[ComplexObject]
impl ReagentQueryResult {
    /// Get the linked PsychonautWiki substance for this query result.
    ///
    /// Uses the original query string for exact name/alias matching against the
    /// PsychonautWiki substance database. Falls back to the matched reagent
    /// database name if the original query doesn't match.
    async fn pw_substance(
        &self,
        ctx: &Context<'_>,
    ) -> async_graphql::Result<Option<Substance>> {
        let snapshot_holder = ctx.data::<SnapshotHolder>()?;
        let snapshot = snapshot_holder.get().await;

        // Try the original query first (what the user typed)
        if let Some(s) = snapshot.get_by_name_or_alias(&self.query) {
            return Ok(Some(s.clone()));
        }

        // Fallback: try the matched reagent database name
        if let Some(s) = snapshot.get_by_name_or_alias(&self.matched_name) {
            return Ok(Some(s.clone()));
        }

        Ok(None)
    }
}
