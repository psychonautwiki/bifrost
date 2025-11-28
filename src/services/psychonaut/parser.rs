use crate::error::BifrostError;
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::HashMap;
use once_cell::sync::Lazy;

static RANGE_DUR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(.*?)_(.*?)_(.*?)_time$").unwrap());
static RANGE_DOSE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(.*?)_(.*?)_(.*?)_dose$").unwrap());
static DEF_DOSE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(.*?)_(.*?)_dose$").unwrap());
static DEF_BIO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(.*?)_(.*?)_bioavailability$").unwrap());
static DOSE_UNITS: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(.*?)_dose_units$").unwrap());
static ROA_TIME_UNITS: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(.*?)_(.*?)_time_units$").unwrap());
static META_TOLERANCE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)Time_to_(.*?)_tolerance$").unwrap());
static WT_LINK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\[\[(.*?)\]\]").unwrap());
static WT_NAMED_LINK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\[\[.*?\|(.*?)\]\]").unwrap());
static WT_SUB_SUP: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)<su[bp]>(.*?)</su[bp]>").unwrap());

#[derive(Clone)]
pub struct WikitextParser;

impl WikitextParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse_smw(&self, smw_data: Value) -> Result<Value, BifrostError> {
        let entities = smw_data.pointer("/query/data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| BifrostError::Parsing("Invalid SMW data structure".into()))?;

        let mut proc_map = Value::Object(Map::new());

        for entity in entities {
            let prop_name = entity.get("property")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .replace("_", " ");

            let prop_name_lower = prop_name.to_lowercase().replace(" ", "_");

            let data_items = entity.get("dataitem").and_then(|v| v.as_array());
            if data_items.is_none() { continue; }

            let val = self.extract_value(data_items.unwrap());
            if val.is_null() { continue; }

            self.dispatch_property(&mut proc_map, &prop_name_lower, val);
        }

        if let Some(obj) = proc_map.as_object_mut() {
            if let Some(roa) = obj.get_mut("roa").and_then(|v| v.as_object_mut()) {
                let mut roa_list = Vec::new();
                for (name, data) in roa.iter() {
                    let mut obj = data.clone();
                    if let Some(o) = obj.as_object_mut() {
                        o.insert("name".to_string(), Value::String(name.clone()));
                    }
                    roa_list.push(obj);
                }
                obj.insert("roas".to_string(), Value::Array(roa_list));
            }
        }

        Ok(proc_map)
    }

    fn extract_value(&self, items: &Vec<Value>) -> Value {
        let processed: Vec<Value> = items.iter().map(|item| {
            let type_id = item.get("type").and_then(|v| v.as_u64()).unwrap_or(0);
            let val_item = item.get("item");

            match type_id {
                1 => {
                    if let Some(s) = val_item.and_then(|v| v.as_str()) {
                        s.parse::<f64>().ok().map(Value::from).unwrap_or(Value::Null)
                    } else if let Some(n) = val_item.and_then(|v| v.as_f64()) {
                        Value::from(n)
                    } else {
                        Value::Null
                    }
                },
                9 => {
                    if let Some(s) = val_item.and_then(|v| v.as_str()) {
                        let stripped = s.split('#').next().unwrap_or(s);
                        Value::String(stripped.to_string())
                    } else {
                        Value::Null
                    }
                },
                _ => val_item.cloned().unwrap_or(Value::Null),
            }
        }).collect();

        if processed.is_empty() {
            Value::Null
        } else if processed.len() == 1 {
            processed[0].clone()
        } else {
            Value::Array(processed)
        }
    }

    fn dispatch_property(&self, map: &mut Value, prop_name: &str, val: Value) {
        if let Some(caps) = RANGE_DUR.captures(prop_name) {
            self.set_nested(map, &format!("roa.{}.duration.{}.{}", &caps[1], &caps[3], &caps[2]), val);
            return;
        }
        if let Some(caps) = RANGE_DOSE.captures(prop_name) {
            self.set_nested(map, &format!("roa.{}.dose.{}.{}", &caps[1], &caps[3], &caps[2]), val);
            return;
        }
        if let Some(caps) = DEF_DOSE.captures(prop_name) {
            self.set_nested(map, &format!("roa.{}.dose.{}", &caps[1], &caps[2]), val);
            return;
        }
        if let Some(caps) = DEF_BIO.captures(prop_name) {
            self.set_nested(map, &format!("roa.{}.bioavailability.{}", &caps[1], &caps[2]), val);
            return;
        }
        if let Some(caps) = DOSE_UNITS.captures(prop_name) {
            self.set_nested(map, &format!("roa.{}.dose.units", &caps[1]), val);
            return;
        }
        if let Some(caps) = ROA_TIME_UNITS.captures(prop_name) {
            self.set_nested(map, &format!("roa.{}.duration.{}.units", &caps[1], &caps[2]), val);
            return;
        }
        if let Some(caps) = META_TOLERANCE.captures(prop_name) {
            self.set_nested(map, &format!("tolerance.{}", &caps[1]), val);
            return;
        }

        // Fields that should always be arrays
        let array_fields = HashMap::from([
            ("uncertaininteraction", "uncertainInteractions"),
            ("unsafeinteraction", "unsafeInteractions"),
            ("dangerousinteraction", "dangerousInteractions"),
            ("effect", "effects"),
            ("common_name", "commonNames"),
        ]);

        if let Some(target) = array_fields.get(prop_name) {
            self.set_nested(map, target, self.ensure_array(self.sanitize_text_val(val)));
            return;
        }

        // Scalar fields
        let scalar_fields = HashMap::from([
            ("addiction_potential", "addictionPotential"),
            ("systematic_name", "systematicName"),
        ]);

        if let Some(target) = scalar_fields.get(prop_name) {
            self.set_nested(map, target, self.sanitize_text_val(val));
            return;
        }

        match prop_name {
            "cross-tolerance" => {
                if let Value::String(s) = &val {
                    let matches: Vec<Value> = WT_LINK.captures_iter(s)
                        .map(|c| Value::String(c[1].to_string()))
                        .collect();
                    self.set_nested(map, "crossTolerances", Value::Array(matches));
                } else if let Value::Array(arr) = &val {
                     let mut all_matches = Vec::new();
                     for item in arr {
                         if let Value::String(s) = item {
                             for cap in WT_LINK.captures_iter(s) {
                                 all_matches.push(Value::String(cap[1].to_string()));
                             }
                         }
                     }
                     self.set_nested(map, "crossTolerances", Value::Array(all_matches));
                }
            },
            "featured" => {
                let is_featured = if let Value::String(s) = &val { s == "t" } else { false };
                self.set_nested(map, "featured", Value::Bool(is_featured));
            },
            "toxicity" => {
                let arr = if val.is_array() { val } else { Value::Array(vec![val]) };
                self.set_nested(map, "toxicity", self.sanitize_text_val(arr));
            },
            "psychoactive_class" => {
                self.set_nested(map, "class.psychoactive", self.clean_class(val));
            },
            "chemical_class" => {
                self.set_nested(map, "class.chemical", self.clean_class(val));
            },
            _ => {}
        }
    }

    fn set_nested(&self, map: &mut Value, path: &str, val: Value) {
        let parts: Vec<&str> = path.split('.').collect();
        self.insert_recursive(map, &parts, val);
    }

    fn insert_recursive(&self, current: &mut Value, parts: &[&str], val: Value) {
        if parts.is_empty() { return; }
        let key = parts[0];

        if parts.len() == 1 {
            if let Some(obj) = current.as_object_mut() {
                obj.insert(key.to_string(), val);
            }
            return;
        }

        if let Some(obj) = current.as_object_mut() {
            if !obj.contains_key(key) || !obj[key].is_object() {
                obj.insert(key.to_string(), Value::Object(Map::new()));
            }
            let next = obj.get_mut(key).unwrap();
            self.insert_recursive(next, &parts[1..], val);
        }
    }

    /// Ensure value is always an array
    fn ensure_array(&self, val: Value) -> Value {
        match val {
            Value::Array(_) => val,
            Value::Null => Value::Array(vec![]),
            _ => Value::Array(vec![val]),
        }
    }

    fn clean_class(&self, val: Value) -> Value {
        let arr = match val {
            Value::Array(a) => a,
            Value::Null => return Value::Array(vec![]),
            v => vec![v],
        };
        let cleaned: Vec<Value> = arr
            .into_iter()
            .filter_map(|v| {
                if let Value::String(s) = v {
                    Some(Value::String(s.trim_end_matches('#').replace("_", " ")))
                } else if !v.is_null() {
                    Some(v)
                } else {
                    None
                }
            })
            .collect();
        Value::Array(cleaned)
    }

    fn sanitize_text_val(&self, val: Value) -> Value {
        match val {
            Value::String(s) => Value::String(self.sanitize_text(&s)),
            Value::Array(arr) => Value::Array(arr.into_iter().map(|v| self.sanitize_text_val(v)).collect()),
            _ => val,
        }
    }

    fn sanitize_text(&self, text: &str) -> String {
        let mut out = text.to_string();
        out = WT_NAMED_LINK.replace_all(&out, "$1").to_string();
        out = WT_LINK.replace_all(&out, "$1").to_string();
        out = WT_SUB_SUP.replace_all(&out, "$1").to_string();
        out
    }
}
