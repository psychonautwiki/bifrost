use crate::error::BifrostError;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::{debug, instrument, trace, warn};

const MAX_RETRIES: u32 = 3;
const INITIAL_BACKOFF_MS: u64 = 100;

#[derive(Clone)]
pub struct PsychonautApi {
    client: Client,
    base_url: String,
}

#[derive(Deserialize, serde::Serialize, Debug, Clone)]
pub struct AskResultItem {
    pub fulltext: String,
    pub fullurl: String,
}

impl PsychonautApi {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
            base_url: base_url.to_string(),
        }
    }

    /// Execute an HTTP GET request with exponential backoff retry logic.
    async fn get_with_retry(&self, params: &[(&str, &str)]) -> Result<Value, BifrostError> {
        let mut last_error = None;
        let action = params
            .iter()
            .find(|(k, _)| *k == "action")
            .map(|(_, v)| *v)
            .unwrap_or("unknown");

        for attempt in 0..MAX_RETRIES {
            let start = Instant::now();

            debug!(
                action = action,
                attempt = attempt + 1,
                url = %self.base_url,
                "Sending request to PsychonautWiki API"
            );

            match self.client.get(&self.base_url).query(params).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let elapsed = start.elapsed();

                    if status.is_success() {
                        debug!(
                            action = action,
                            status = %status,
                            elapsed_ms = elapsed.as_millis() as u64,
                            "Request successful"
                        );

                        let body: Value = resp.json().await.map_err(BifrostError::from)?;

                        // Log response size in debug mode
                        if let Ok(body_str) = serde_json::to_string(&body) {
                            trace!(
                                action = action,
                                response_bytes = body_str.len(),
                                "Response body received"
                            );
                        }

                        return Ok(body);
                    } else if status.is_server_error() {
                        debug!(
                            action = action,
                            status = %status,
                            elapsed_ms = elapsed.as_millis() as u64,
                            "Server error, will retry"
                        );
                        last_error = Some(BifrostError::Upstream(format!("HTTP {}", status)));
                    } else {
                        debug!(
                            action = action,
                            status = %status,
                            elapsed_ms = elapsed.as_millis() as u64,
                            "Client error, not retrying"
                        );
                        return Err(BifrostError::Upstream(format!("HTTP {}", status)));
                    }
                }
                Err(e) => {
                    let elapsed = start.elapsed();
                    debug!(
                        action = action,
                        error = %e,
                        elapsed_ms = elapsed.as_millis() as u64,
                        "Request failed"
                    );
                    last_error = Some(BifrostError::from(e));
                }
            }

            if attempt < MAX_RETRIES - 1 {
                let backoff = INITIAL_BACKOFF_MS * 2u64.pow(attempt);
                warn!(
                    action = action,
                    attempt = attempt + 1,
                    max_retries = MAX_RETRIES,
                    backoff_ms = backoff,
                    "Request failed, retrying"
                );
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }
        }

        Err(last_error
            .unwrap_or_else(|| BifrostError::Upstream("Request failed after retries".into())))
    }

    fn render_pagination(limit: i32, offset: i32) -> String {
        let mut s = String::new();
        if limit > 0 {
            s.push_str(&format!("|limit={}", limit));
        }
        if offset > 0 {
            s.push_str(&format!("|offset={}", offset));
        }
        s
    }

    #[instrument(skip(self), fields(query_type = "ask"))]
    pub async fn ask_query(
        &self,
        query: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<AskResultItem>, BifrostError> {
        let full_query = format!("{}{}", query, Self::render_pagination(limit, offset));
        debug!(query = %full_query, "Executing SMW ask query");

        let params = [
            ("action", "ask"),
            ("format", "json"),
            ("query", &full_query),
        ];

        let json = self.get_with_retry(&params).await?;

        // Parse query.results
        // Results can be an object (map) or array. We need to normalize.
        let results = json.pointer("/query/results");

        match results {
            Some(Value::Object(map)) => {
                let mut items = Vec::new();
                for (_, v) in map {
                    if let Ok(item) = serde_json::from_value::<AskResultItem>(v.clone()) {
                        items.push(item);
                    }
                }
                Ok(items)
            }
            Some(Value::Array(arr)) => {
                let mut items = Vec::new();
                for v in arr {
                    if let Ok(item) = serde_json::from_value::<AskResultItem>(v.clone()) {
                        items.push(item);
                    }
                }
                Ok(items)
            }
            _ => Ok(Vec::new()),
        }
    }

    #[instrument(skip(self), fields(query_type = "class"))]
    pub async fn get_by_class(
        &self,
        class_type: &str,
        class_name: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<AskResultItem>, BifrostError> {
        let query = format!(
            "[[{}::{}]]|[[Category:Psychoactive substance]]",
            class_type, class_name
        );
        self.ask_query(&query, limit, offset).await
    }

    #[instrument(skip(self), fields(query_type = "substance_effects"))]
    pub async fn get_substance_effects(
        &self,
        substance: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<AskResultItem>, BifrostError> {
        let query = format!("[[:{}]]|?Effect", substance);
        // Note: The legacy code handles printouts differently.
        // We need to fetch the printouts.
        // For simplicity in this rewrite, we'll stick to the ask_query structure but we might need to adjust parsing if printouts are structured differently.
        // However, the legacy code maps text/url from results.
        // Let's assume standard ask behavior for now, but printouts usually come in a specific field.
        // Actually, `ask` with `|?Effect` puts the effect in `printouts`.
        // The generic `ask_query` above extracts `fulltext` and `fullurl` of the SUBJECT.
        // To get the effects, we need to parse the printouts.

        // Custom implementation for printouts:
        let full_query = format!("{}{}", query, Self::render_pagination(limit, offset));
        let params = [
            ("action", "ask"),
            ("format", "json"),
            ("query", &full_query),
        ];
        let json = self.get_with_retry(&params).await?;

        let mut effects = Vec::new();
        if let Some(results) = json.pointer("/query/results") {
            if let Some(sub_obj) = results.get(substance) {
                if let Some(printouts) = sub_obj.get("printouts") {
                    if let Some(eff_arr) = printouts.get("Effect") {
                        if let Some(arr) = eff_arr.as_array() {
                            for item in arr {
                                if let Ok(res) =
                                    serde_json::from_value::<AskResultItem>(item.clone())
                                {
                                    effects.push(res);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(effects)
    }

    #[instrument(skip(self), fields(query_type = "effect_substances"))]
    pub async fn get_effect_substances(
        &self,
        effect: &str,
        limit: i32,
        offset: i32,
    ) -> Result<Vec<AskResultItem>, BifrostError> {
        let query = format!("[[Effect::{}]]|[[Category:Psychoactive substance]]", effect);
        self.ask_query(&query, limit, offset).await
    }

    #[instrument(skip(self), fields(query_type = "browse"))]
    pub async fn browse_by_subject(&self, subject: &str) -> Result<Value, BifrostError> {
        debug!(subject = %subject, "Browsing semantic properties");
        let params = [
            ("action", "browsebysubject"),
            ("format", "json"),
            ("subject", subject),
        ];
        self.get_with_retry(&params).await
    }

    #[instrument(skip(self), fields(query_type = "parse_text"))]
    pub async fn parse_text(&self, page: &str) -> Result<String, BifrostError> {
        debug!(page = %page, "Parsing page text");
        let params = [
            ("action", "parse"),
            ("format", "json"),
            ("page", page),
            ("prop", "text"),
            ("section", "0"),
        ];
        let json = self.get_with_retry(&params).await?;

        json.pointer("/parse/text/*")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| BifrostError::Parsing("No text found in parse response".into()))
    }

    #[instrument(skip(self), fields(query_type = "parse_images"))]
    pub async fn parse_images(&self, page: &str) -> Result<Vec<String>, BifrostError> {
        debug!(page = %page, "Parsing page images");
        let params = [
            ("action", "parse"),
            ("format", "json"),
            ("page", page),
            ("prop", "images"),
        ];
        let json = self.get_with_retry(&params).await?;

        let images: Vec<String> = json
            .pointer("/parse/images")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        debug!(image_count = images.len(), "Found images");
        Ok(images)
    }
}
