pub mod api;
pub mod parser;

use crate::cache::StaleWhileRevalidateCache;
use crate::config::Config;
use crate::error::BifrostError;
use crate::graphql::model::{Effect, Substance, SubstanceImage};
use crate::services::psychonaut::api::{AskResultItem, PsychonautApi};
use crate::services::psychonaut::parser::WikitextParser;
use futures::{stream, StreamExt};
use md5::Digest;
use std::sync::Arc;

/// Maximum concurrent requests to the upstream API
const MAX_CONCURRENT_REQUESTS: usize = 100;

pub struct PsychonautService {
    api: PsychonautApi,
    parser: WikitextParser,
    cache: StaleWhileRevalidateCache<String, serde_json::Value>,
    config: Config,
}

impl PsychonautService {
    pub fn new(config: &Config) -> Self {
        Self {
            api: PsychonautApi::new(&config.psychonaut.api_url),
            parser: WikitextParser::new(),
            cache: StaleWhileRevalidateCache::new(config.psychonaut.cache_ttl_ms),
            config: config.clone(),
        }
    }

    /// Cached ask query - caches the raw API results
    async fn cached_ask_query(
        &self,
        query: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<AskResultItem>, BifrostError> {
        let cache_key = format!("ask:{}:{}:{}", query, limit, offset);
        let api_ref = self.api.clone();
        let query_clone = query.to_string();

        let result = self
            .cache
            .get(cache_key, move || {
                let api = api_ref.clone();
                let q = query_clone.clone();
                async move {
                    let items = api.ask_query(&q, limit, offset).await?;
                    Ok(serde_json::to_value(&items)?)
                }
            })
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    /// Cached class query
    async fn cached_get_by_class(
        &self,
        class_type: &str,
        class_name: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<AskResultItem>, BifrostError> {
        let cache_key = format!("class:{}:{}:{}:{}", class_type, class_name, limit, offset);
        let api_ref = self.api.clone();
        let class_type_clone = class_type.to_string();
        let class_name_clone = class_name.to_string();

        let result = self
            .cache
            .get(cache_key, move || {
                let api = api_ref.clone();
                let ct = class_type_clone.clone();
                let cn = class_name_clone.clone();
                async move {
                    let items = api.get_by_class(&ct, &cn, limit, offset).await?;
                    Ok(serde_json::to_value(&items)?)
                }
            })
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    /// Cached effect substances query
    async fn cached_get_effect_substances(
        &self,
        effect: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<AskResultItem>, BifrostError> {
        let cache_key = format!("effect_substances:{}:{}:{}", effect, limit, offset);
        let api_ref = self.api.clone();
        let effect_clone = effect.to_string();

        let result = self
            .cache
            .get(cache_key, move || {
                let api = api_ref.clone();
                let e = effect_clone.clone();
                async move {
                    let items = api.get_effect_substances(&e, limit, offset).await?;
                    Ok(serde_json::to_value(&items)?)
                }
            })
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    /// Cached substance effects query
    async fn cached_get_substance_effects(
        &self,
        substance: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<AskResultItem>, BifrostError> {
        let cache_key = format!("substance_effects:{}:{}:{}", substance, limit, offset);
        let api_ref = self.api.clone();
        let substance_clone = substance.to_string();

        let result = self
            .cache
            .get(cache_key, move || {
                let api = api_ref.clone();
                let s = substance_clone.clone();
                async move {
                    let items = api.get_substance_effects(&s, limit, offset).await?;
                    Ok(serde_json::to_value(&items)?)
                }
            })
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    pub async fn get_substances(
        &self,
        query: Option<String>,
        effect: Option<String>,
        chemical_class: Option<String>,
        psychoactive_class: Option<String>,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<Substance>, BifrostError> {
        let params = vec![&query, &effect, &chemical_class, &psychoactive_class];
        if params.iter().filter(|p| p.is_some()).count() >= 2 {
            return Err(BifrostError::Parsing(
                "Parameters are mutually exclusive".into(),
            ));
        }

        let results = if let Some(c) = chemical_class {
            self.cached_get_by_class("Chemical class", &c, limit, offset)
                .await?
        } else if let Some(p) = psychoactive_class {
            self.cached_get_by_class("Psychoactive class", &p, limit, offset)
                .await?
        } else if let Some(e) = effect {
            self.cached_get_effect_substances(&e, limit, offset).await?
        } else {
            let q = query.unwrap_or_default();
            let article_query = if q.is_empty() {
                "Category:Psychoactive substance".to_string()
            } else {
                // Limit query to Psychoactive substance category to avoid non-substance matches
                format!(":{}]]|[[Category:Psychoactive substance", q)
            };

            let mut res = self
                .cached_ask_query(&format!("[[{}]]", article_query), limit, offset)
                .await?;

            if res.is_empty() && !q.is_empty() {
                res = self
                    .cached_ask_query(
                        &format!("[[common_name::{}]]|[[Category:Psychoactive substance]]", q),
                        limit,
                        offset,
                    )
                    .await?;
            }
            if res.is_empty() && !q.is_empty() {
                res = self
                    .cached_ask_query(
                        &format!(
                            "[[systematic_name::{}]]|[[Category:Psychoactive substance]]",
                            q
                        ),
                        limit,
                        offset,
                    )
                    .await?;
            }
            res
        };

        // Filter out incorrectly tagged pages (e.g., "Experience:..." or other namespaced pages)
        let results: Vec<_> = results
            .into_iter()
            .filter(|item| !item.fulltext.contains(':'))
            .collect();

        // Fetch substance details in parallel (up to MAX_CONCURRENT_REQUESTS at a time)
        let cache = Arc::new(self.cache.clone());
        let api = Arc::new(self.api.clone());
        let parser = Arc::new(self.parser.clone());

        let enriched: Vec<Substance> = stream::iter(results)
            .map(|item| {
                let cache = cache.clone();
                let api = api.clone();
                let parser = parser.clone();

                async move {
                    let substance_name = item.fulltext.clone();
                    let url = item.fullurl.clone();
                    let cache_key = format!("substance:{}", substance_name);

                    let result = cache
                        .get(cache_key, {
                            let api = api.clone();
                            let parser = parser.clone();
                            let name = substance_name.clone();
                            let url = url.clone();
                            move || {
                                let api = api.clone();
                                let parser = parser.clone();
                                let name = name.clone();
                                let url = url.clone();
                                async move {
                                    let raw = api.browse_by_subject(&name).await?;
                                    let mut parsed = parser.parse_smw(raw)?;

                                    if let Some(obj) = parsed.as_object_mut() {
                                        obj.insert(
                                            "name".to_string(),
                                            serde_json::Value::String(name),
                                        );
                                        obj.insert(
                                            "url".to_string(),
                                            serde_json::Value::String(url),
                                        );
                                    }

                                    let substance: Substance = serde_json::from_value(parsed)
                                        .map_err(|e| {
                                            BifrostError::Parsing(format!(
                                                "Failed to parse substance: {}",
                                                e
                                            ))
                                        })?;

                                    Ok(serde_json::to_value(substance)?)
                                }
                            }
                        })
                        .await;

                    match result {
                        Ok(val) => serde_json::from_value::<Substance>(val).ok(),
                        Err(e) => {
                            tracing::warn!("Skipping substance '{}': {}", substance_name, e);
                            None
                        }
                    }
                }
            })
            .buffer_unordered(MAX_CONCURRENT_REQUESTS)
            .filter_map(|x| async { x })
            .collect()
            .await;

        Ok(enriched)
    }

    pub async fn get_substance_effects(
        &self,
        substance: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<Effect>, BifrostError> {
        let results = self
            .cached_get_substance_effects(substance, limit, offset)
            .await?;
        Ok(results
            .into_iter()
            .map(|r| Effect {
                name: Some(r.fulltext),
                url: Some(r.fullurl),
            })
            .collect())
    }

    pub async fn get_effect_substances(
        &self,
        effect: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<Substance>, BifrostError> {
        let results = self
            .cached_get_effect_substances(effect, limit, offset)
            .await?;
        Ok(results
            .into_iter()
            .map(|r| Substance {
                name: Some(r.fulltext),
                url: Some(r.fullurl),
                ..Default::default()
            })
            .collect())
    }

    pub async fn get_substance_abstract(
        &self,
        substance: &str,
    ) -> Result<Option<String>, BifrostError> {
        let cache_key = format!("abstract:{}", substance);
        let api_ref = self.api.clone();
        let name_clone = substance.to_string();

        let text = self
            .cache
            .get(cache_key, move || {
                let api = api_ref.clone();
                let name = name_clone.clone();
                async move {
                    let raw = api.parse_text(&name).await?;
                    let re = regex::Regex::new(r"<[^>]*>").unwrap();
                    let no_tags = re.replace_all(&raw, "").to_string();
                    let cleaned = no_tags
                        .trim()
                        .replace("[edit]", "")
                        .lines()
                        .map(|l| l.trim())
                        .filter(|l| !l.is_empty())
                        .take(2)
                        .collect::<Vec<&str>>()
                        .join(" ");

                    Ok(serde_json::Value::String(cleaned))
                }
            })
            .await?;

        Ok(text.as_str().map(|s| s.to_string()))
    }

    pub async fn get_substance_images(
        &self,
        substance: &str,
    ) -> Result<Option<Vec<SubstanceImage>>, BifrostError> {
        let cache_key = format!("images:{}", substance);
        let api_ref = self.api.clone();
        let name_clone = substance.to_string();
        let config_ref = self.config.clone();

        let images_json = self
            .cache
            .get(cache_key, move || {
                let api = api_ref.clone();
                let name = name_clone.clone();
                async move {
                    let images = api.parse_images(&name).await?;
                    Ok(serde_json::to_value(images)?)
                }
            })
            .await?;

        let images: Vec<String> = serde_json::from_value(images_json)?;
        if images.is_empty() {
            return Ok(None);
        }

        let thumb_size = config_ref.psychonaut.thumb_size;
        let cdn_url = config_ref.psychonaut.cdn_url;

        let mapped = images
            .into_iter()
            .map(|filename| {
                let mut hasher = md5::Md5::new();
                hasher.update(filename.as_bytes());
                let hash = hasher.finalize();
                let hash_hex = hex::encode(hash);
                let a = &hash_hex[0..1];
                let ab = &hash_hex[0..2];

                SubstanceImage {
                    thumb: Some(format!(
                        "{}w/thumb.php?f={}&width={}",
                        cdn_url, filename, thumb_size
                    )),
                    image: Some(format!("{}w/images/{}/{}/{}", cdn_url, a, ab, filename)),
                }
            })
            .collect();

        Ok(Some(mapped))
    }
}
