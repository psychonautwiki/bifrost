use async_graphql::SimpleObject;
use serde::{Deserialize, Serialize};

/// Deserialize a field that can be either a string or an array of strings.
/// If it's an array, join with ", ".
mod string_or_array {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrArray {
            String(String),
            Array(Vec<String>),
        }

        let opt = Option::<StringOrArray>::deserialize(deserializer)?;
        Ok(opt.map(|v| match v {
            StringOrArray::String(s) => s,
            StringOrArray::Array(arr) => arr.join(", "),
        }))
    }

    pub fn serialize<S>(value: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(s) => serializer.serialize_some(s),
            None => serializer.serialize_none(),
        }
    }
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Default, Clone)]
#[graphql(complex)]
pub struct Substance {
    pub name: Option<String>,
    pub url: Option<String>,
    pub featured: Option<bool>,

    pub class: Option<SubstanceClass>,
    pub tolerance: Option<SubstanceTolerance>,
    pub roa: Option<SubstanceRoaTypes>,
    pub roas: Option<Vec<SubstanceRoa>>,

    #[serde(rename = "addictionPotential", with = "string_or_array", default)]
    pub addiction_potential: Option<String>,
    pub toxicity: Option<Vec<String>>,
    #[serde(rename = "crossTolerances")]
    pub cross_tolerances: Option<Vec<String>>,
    #[serde(rename = "commonNames")]
    pub common_names: Option<Vec<String>>,
    #[serde(rename = "systematicName", with = "string_or_array", default)]
    pub systematic_name: Option<String>,

    // Interaction references (names only - resolved at query time via snapshot)
    #[serde(rename = "uncertainInteractions")]
    #[graphql(skip)]
    pub uncertain_interactions_raw: Option<Vec<String>>,

    #[serde(rename = "unsafeInteractions")]
    #[graphql(skip)]
    pub unsafe_interactions_raw: Option<Vec<String>>,

    #[serde(rename = "dangerousInteractions")]
    #[graphql(skip)]
    pub dangerous_interactions_raw: Option<Vec<String>>,

    // Pre-fetched cached data (populated during revalidation, served from snapshot)
    // These are skipped in GraphQL and resolved via ComplexObject
    #[serde(rename = "effectsCache")]
    #[graphql(skip)]
    pub effects_cache: Option<Vec<Effect>>,

    #[serde(rename = "summaryCache")]
    #[graphql(skip)]
    pub summary_cache: Option<String>,

    #[serde(rename = "imagesCache")]
    #[graphql(skip)]
    pub images_cache: Option<Vec<SubstanceImage>>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct SubstanceClass {
    pub chemical: Option<Vec<String>>,
    pub psychoactive: Option<Vec<String>>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct SubstanceTolerance {
    #[serde(with = "string_or_array", default)]
    pub full: Option<String>,
    #[serde(with = "string_or_array", default)]
    pub half: Option<String>,
    #[serde(with = "string_or_array", default)]
    pub zero: Option<String>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct SubstanceRoaRange {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct SubstanceRoaDurationRange {
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(with = "string_or_array", default)]
    pub units: Option<String>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct SubstanceRoaDose {
    #[serde(with = "string_or_array", default)]
    pub units: Option<String>,
    pub threshold: Option<f64>,
    pub heavy: Option<f64>,
    pub common: Option<SubstanceRoaRange>,
    pub light: Option<SubstanceRoaRange>,
    pub strong: Option<SubstanceRoaRange>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct SubstanceRoaDuration {
    pub afterglow: Option<SubstanceRoaDurationRange>,
    pub comeup: Option<SubstanceRoaDurationRange>,
    pub duration: Option<SubstanceRoaDurationRange>,
    pub offset: Option<SubstanceRoaDurationRange>,
    pub onset: Option<SubstanceRoaDurationRange>,
    pub peak: Option<SubstanceRoaDurationRange>,
    pub total: Option<SubstanceRoaDurationRange>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct SubstanceRoa {
    #[serde(with = "string_or_array", default)]
    pub name: Option<String>,
    pub dose: Option<SubstanceRoaDose>,
    pub duration: Option<SubstanceRoaDuration>,
    pub bioavailability: Option<SubstanceRoaRange>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct SubstanceRoaTypes {
    pub oral: Option<SubstanceRoa>,
    pub sublingual: Option<SubstanceRoa>,
    pub buccal: Option<SubstanceRoa>,
    pub insufflated: Option<SubstanceRoa>,
    pub rectal: Option<SubstanceRoa>,
    pub transdermal: Option<SubstanceRoa>,
    pub subcutaneous: Option<SubstanceRoa>,
    pub intramuscular: Option<SubstanceRoa>,
    pub intravenous: Option<SubstanceRoa>,
    pub smoked: Option<SubstanceRoa>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct SubstanceImage {
    pub thumb: Option<String>,
    pub image: Option<String>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
#[graphql(complex)]
pub struct Effect {
    pub name: Option<String>,
    pub url: Option<String>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct Experience {
    pub substances: Option<Vec<Substance>>,
    pub effects: Option<Vec<Effect>>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct ErowidMeta {
    #[serde(rename = "erowidId")]
    pub erowid_id: Option<String>,
    pub gender: Option<String>,
    pub published: Option<String>,
    pub year: Option<i32>,
    pub age: Option<i32>,
    pub views: Option<i32>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct ErowidSubstanceInfo {
    pub amount: Option<String>,
    pub method: Option<String>,
    pub substance: Option<String>,
    pub form: Option<String>,
}

#[derive(SimpleObject, Serialize, Deserialize, Debug, Clone)]
pub struct ErowidExperience {
    pub title: Option<String>,
    pub author: Option<String>,
    pub substance: Option<String>,
    pub meta: Option<ErowidMeta>,
    #[serde(rename = "substanceInfo")]
    pub substance_info: Option<Vec<ErowidSubstanceInfo>>,
    #[serde(rename = "erowidNotes")]
    pub erowid_notes: Option<Vec<String>>,
    #[serde(rename = "pullQuotes")]
    pub pull_quotes: Option<Vec<String>>,
    pub body: Option<String>,
}
