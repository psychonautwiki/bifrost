use crate::graphql::model::*;
use crate::services::plebiscite::PlebisciteService;
use crate::services::psychonaut::PsychonautService;
use async_graphql::{ComplexObject, Context, EmptyMutation, EmptySubscription, Object, Schema};
use std::sync::Arc;

pub type BifrostSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

pub fn create_schema(
    psychonaut_service: Arc<PsychonautService>,
    plebiscite_service: Option<Arc<PlebisciteService>>,
) -> BifrostSchema {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(psychonaut_service)
        .data(plebiscite_service)
        .finish()
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
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
        let service = ctx.data::<Arc<PsychonautService>>()?;
        let results = service.get_substances(query, effect, chemical_class, psychoactive_class, limit, offset).await?;
        Ok(results)
    }

    async fn substances_by_effect(
        &self,
        ctx: &Context<'_>,
        effect: Option<Vec<String>>,
        #[graphql(default = 50)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> async_graphql::Result<Vec<Substance>> {
        let service = ctx.data::<Arc<PsychonautService>>()?;
        let effect_query = effect.map(|e| e.join("|")).unwrap_or_default();
        if effect_query.is_empty() {
            return Ok(vec![]);
        }
        let results = service.get_effect_substances(&effect_query, limit, offset).await?;
        Ok(results)
    }

    async fn effects_by_substance(
        &self,
        ctx: &Context<'_>,
        substance: String,
        #[graphql(default = 50)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> async_graphql::Result<Vec<Effect>> {
        let service = ctx.data::<Arc<PsychonautService>>()?;
        let results = service.get_substance_effects(&substance, limit, offset).await?;
        Ok(results)
    }

    async fn erowid(
        &self,
        ctx: &Context<'_>,
        substance: Option<String>,
        #[graphql(default = 50)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> async_graphql::Result<Option<Vec<ErowidExperience>>> {
        let service_opt = ctx.data::<Option<Arc<PlebisciteService>>>()?;
        if let Some(service) = service_opt {
            let results = service.find(substance, Some(offset), Some(limit)).await?;
            Ok(Some(results))
        } else {
            Ok(None)
        }
    }

    /// Query experiences by substance or effect relationships.
    /// Note: This is a stub that was present in the legacy API but never fully implemented.
    async fn experiences(
        &self,
        ctx: &Context<'_>,
        substances_by_effect: Option<String>,
        effects_by_substance: Option<String>,
        substance: Option<String>,
    ) -> async_graphql::Result<Vec<Experience>> {
        let service = ctx.data::<Arc<PsychonautService>>()?;

        let mut result = Experience {
            substances: None,
            effects: None,
        };

        // Get substances by effect
        if let Some(effect) = substances_by_effect {
            let substances = service.get_effect_substances(&effect, 50, 0).await?;
            result.substances = Some(substances);
        }

        // Get effects by substance
        if let Some(sub) = effects_by_substance {
            let effects = service.get_substance_effects(&sub, 50, 0).await?;
            result.effects = Some(effects);
        }

        // Get substance directly
        if let Some(sub) = substance {
            let substances = service.get_substances(Some(sub), None, None, None, 1, 0).await?;
            result.substances = Some(substances);
        }

        Ok(vec![result])
    }
}

#[ComplexObject]
impl Substance {
    async fn effects(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Effect>> {
        if let Some(name) = &self.name {
            let service = ctx.data::<Arc<PsychonautService>>()?;
            let results = service.get_substance_effects(name, 50, 0).await?;
            Ok(results)
        } else {
            Ok(vec![])
        }
    }

    async fn summary(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<String>> {
        if let Some(name) = &self.name {
            let service = ctx.data::<Arc<PsychonautService>>()?;
            let summary = service.get_substance_abstract(name).await?;
            Ok(summary)
        } else {
            Ok(None)
        }
    }

    async fn images(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Vec<SubstanceImage>>> {
        if let Some(name) = &self.name {
            let service = ctx.data::<Arc<PsychonautService>>()?;
            let images = service.get_substance_images(name).await?;
            Ok(images)
        } else {
            Ok(None)
        }
    }

    async fn uncertain_interactions(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Vec<Substance>>> {
        self.resolve_interactions(ctx, &self.uncertain_interactions_raw).await
    }

    async fn unsafe_interactions(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Vec<Substance>>> {
        self.resolve_interactions(ctx, &self.unsafe_interactions_raw).await
    }

    async fn dangerous_interactions(&self, ctx: &Context<'_>) -> async_graphql::Result<Option<Vec<Substance>>> {
        self.resolve_interactions(ctx, &self.dangerous_interactions_raw).await
    }
}

impl Substance {
    async fn resolve_interactions(&self, ctx: &Context<'_>, raw: &Option<Vec<String>>) -> async_graphql::Result<Option<Vec<Substance>>> {
        if let Some(names) = raw {
            let service = ctx.data::<Arc<PsychonautService>>()?;
            let mut resolved = Vec::new();
            for name in names {
                let res = service.get_substances(Some(name.clone()), None, None, None, 1, 0).await?;
                if let Some(sub) = res.first() {
                    resolved.push(sub.clone());
                } else {
                    resolved.push(Substance {
                        name: Some(name.clone()),
                        ..Default::default()
                    });
                }
            }
            Ok(Some(resolved))
        } else {
            Ok(None)
        }
    }
}

#[ComplexObject]
impl Effect {
    async fn substances(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Substance>> {
        if let Some(name) = &self.name {
            let service = ctx.data::<Arc<PsychonautService>>()?;
            let results = service.get_effect_substances(name, 50, 0).await?;
            Ok(results)
        } else {
            Ok(vec![])
        }
    }
}
