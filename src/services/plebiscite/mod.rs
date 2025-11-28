use crate::config::Config;
use crate::error::BifrostError;
use crate::graphql::model::ErowidExperience;
use futures::stream::TryStreamExt;
use mongodb::{bson::doc, options::ClientOptions, Client, Collection};
use tracing::info;

pub struct PlebisciteService {
    collection: Collection<ErowidExperience>,
}

impl PlebisciteService {
    pub async fn new(config: &Config) -> Result<Self, BifrostError> {
        let mongo_url = config.plebiscite.mongo_url.as_ref()
            .ok_or_else(|| BifrostError::Internal("Mongo URL not configured".to_string()))?;

        let mut client_options = ClientOptions::parse(mongo_url).await?;
        client_options.app_name = Some("Bifrost".to_string());
        
        let client = Client::with_options(client_options)?;
        let db = client.database(&config.plebiscite.mongo_db);
        let collection = db.collection::<ErowidExperience>(&config.plebiscite.mongo_collection);

        info!("Connected to MongoDB: {}/{}", config.plebiscite.mongo_db, config.plebiscite.mongo_collection);

        Ok(Self { collection })
    }

    pub async fn find(
        &self,
        substance: Option<String>,
        offset: Option<i32>,
        limit: Option<i32>,
    ) -> Result<Vec<ErowidExperience>, BifrostError> {
        let mut filter = doc! {};
        if let Some(s) = substance {
            filter.insert("substanceInfo.substance", s);
        }

        let find_options = mongodb::options::FindOptions::builder()
            .sort(doc! { "meta.published": -1 })
            .skip(offset.map(|x| x as u64))
            .limit(limit.map(|x| x as i64).or(Some(50)))
            .build();

        let mut cursor = self.collection.find(filter, find_options).await?;
        let mut results = Vec::new();

        while let Some(doc) = cursor.try_next().await? {
            results.push(doc);
        }

        Ok(results)
    }
}
