use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub type Properties = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    String,
    Integer,
    Number,
    Boolean,
    Date,
    Enum,
    List,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Property {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(rename = "type")]
    pub value_type: ValueType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub editor: String,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub items: Option<ValueType>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub min_length: Option<usize>,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub pattern: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeType {
    #[serde(default)]
    pub id: String,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub display_property: String,
    #[serde(default)]
    pub properties: Vec<Property>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationType {
    #[serde(default)]
    pub id: String,
    pub key: String,
    pub name: String,
    pub inverse_name: String,
    pub sources: Vec<String>,
    pub targets: Vec<String>,
    #[serde(default)]
    pub properties: Vec<Property>,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub max_incoming: Option<usize>,
    #[serde(default)]
    pub max_outgoing: Option<usize>,
    #[serde(default)]
    pub allow_duplicates: bool,
    #[serde(default)]
    pub allow_self: bool,
    #[serde(default)]
    pub acyclic: bool,
    #[serde(default)]
    pub ordered: bool,
}
impl RelationType {
    pub fn family(&self) -> &str {
        if self.family.is_empty() {
            &self.key
        } else {
            &self.family
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    #[serde(default)]
    pub node_types: Vec<NodeType>,
    #[serde(default)]
    pub relationship_types: Vec<RelationType>,
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key.as_bytes()[0].is_ascii_lowercase()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn require(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::validation(message))
    }
}
impl Schema {
    pub fn node_type(&self, key: &str) -> Result<&NodeType> {
        self.node_types
            .iter()
            .find(|t| t.key == key)
            .ok_or_else(|| Error::missing("node type", key))
    }
    pub fn relation_type(&self, key: &str) -> Result<&RelationType> {
        self.relationship_types
            .iter()
            .find(|t| t.key == key)
            .ok_or_else(|| Error::missing("relationship type", key))
    }
    pub fn validate(&self) -> Result<()> {
        require(
            !self.node_types.is_empty(),
            "A schema needs at least one node type",
        )?;
        let mut keys = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for t in &self.node_types {
            require(valid_key(&t.key), format!("Invalid type key '{}'", t.key))?;
            require(
                keys.insert(&t.key),
                format!("Duplicate type key '{}'", t.key),
            )?;
            require(!t.name.trim().is_empty(), "Type display name is required")?;
            if !t.id.is_empty() {
                require(
                    Uuid::parse_str(&t.id).is_ok() && ids.insert(&t.id),
                    "Type IDs must be distinct UUIDs",
                )?;
            }
            validate_definitions(&t.properties)?;
            require(
                t.properties
                    .iter()
                    .any(|p| p.key == t.display_property && p.value_type == ValueType::String),
                format!("{}.display_property must name a string property", t.key),
            )?;
        }
        let mut relation_keys = BTreeSet::new();
        type FamilyPolicy = (Option<usize>, Option<usize>, bool, bool);
        let mut families: BTreeMap<&str, FamilyPolicy> = BTreeMap::new();
        for r in &self.relationship_types {
            require(
                valid_key(&r.key) && relation_keys.insert(&r.key),
                "Relationship keys must be unique stable keys",
            )?;
            require(
                !r.name.trim().is_empty() && !r.inverse_name.trim().is_empty(),
                "Relationship display names are required",
            )?;
            require(
                !r.sources.is_empty() && !r.targets.is_empty(),
                "Relationship endpoint type sets cannot be empty",
            )?;
            require(
                r.sources.iter().chain(&r.targets).all(|k| keys.contains(k)),
                format!("Unknown endpoint type in {}", r.key),
            )?;
            require(
                r.family.is_empty() || valid_key(&r.family),
                "Invalid relationship family",
            )?;
            if !r.id.is_empty() {
                require(
                    Uuid::parse_str(&r.id).is_ok() && ids.insert(&r.id),
                    "Type IDs must be distinct UUIDs",
                )?;
            }
            require(
                r.max_incoming != Some(0) && r.max_outgoing != Some(0),
                "Cardinality limits must be positive",
            )?;
            if r.ordered {
                require(
                    r.acyclic && r.max_incoming == Some(1),
                    "An ordered family needs acyclicity and max_incoming=1",
                )?;
            }
            let policy = (r.max_incoming, r.max_outgoing, r.acyclic, r.ordered);
            if let Some(prior) = families.insert(r.family(), policy) {
                require(
                    prior == policy,
                    "All relationship types in a family must share cardinality, ordering, and cycle rules",
                )?;
            }
            validate_definitions(&r.properties)?;
        }
        Ok(())
    }
    pub fn assign_ids(&mut self, previous: &Schema) -> Result<()> {
        let mut used: BTreeMap<String, String> = BTreeMap::new();
        for t in &previous.node_types {
            used.insert(t.id.clone(), format!("node:{}", t.key));
        }
        for t in &previous.relationship_types {
            used.insert(t.id.clone(), format!("relation:{}", t.key));
        }
        for t in &mut self.node_types {
            let old = previous
                .node_types
                .iter()
                .find(|old| old.key == t.key)
                .map(|old| &old.id);
            assign_id(&mut t.id, old, &used, &format!("node:{}", t.key))?;
        }
        for t in &mut self.relationship_types {
            let old = previous
                .relationship_types
                .iter()
                .find(|old| old.key == t.key)
                .map(|old| &old.id);
            assign_id(&mut t.id, old, &used, &format!("relation:{}", t.key))?;
        }
        self.validate()
    }
}
fn assign_id(
    id: &mut String,
    old: Option<&String>,
    used: &BTreeMap<String, String>,
    key: &str,
) -> Result<()> {
    if let Some(old) = old {
        require(
            id.is_empty() || id == old,
            "Changing the identity of an existing type is not allowed",
        )?;
        *id = old.clone();
    } else if id.is_empty() {
        *id = Uuid::new_v4().to_string();
    }
    if let Some(prior) = used.get(id) {
        require(
            prior == key,
            "A type ID cannot be reassigned to another key",
        )?;
    }
    Ok(())
}
fn validate_definitions(properties: &[Property]) -> Result<()> {
    let mut keys = BTreeSet::new();
    for p in properties {
        require(
            valid_key(&p.key) && keys.insert(&p.key),
            format!("Invalid or duplicate property key '{}'", p.key),
        )?;
        require(
            p.min.zip(p.max).is_none_or(|(a, b)| a <= b),
            "Property minimum exceeds maximum",
        )?;
        require(
            p.min_length.zip(p.max_length).is_none_or(|(a, b)| a <= b),
            "Property minimum length exceeds maximum",
        )?;
        if let Some(pattern) = &p.pattern {
            regex::Regex::new(pattern)
                .map_err(|e| Error::validation(format!("{}: invalid pattern: {e}", p.key)))?;
        }
        if p.value_type == ValueType::Enum {
            require(
                !p.choices.is_empty()
                    && p.choices.iter().collect::<BTreeSet<_>>().len() == p.choices.len(),
                "Enum choices must be nonempty and distinct",
            )?;
        }
        if p.value_type == ValueType::List {
            require(
                p.items.as_ref().is_some_and(|i| *i != ValueType::List),
                "Lists need a scalar item type",
            )?;
            if p.items == Some(ValueType::Enum) {
                require(!p.choices.is_empty(), "Enum lists need choices")?;
            }
        }
        if let Some(v) = &p.default {
            require(!v.is_null(), "A default cannot be null")?;
            validate_value(p, v)?;
        }
    }
    Ok(())
}
fn scalar_ok(kind: &ValueType, value: &Value, choices: &[String]) -> bool {
    match kind {
        ValueType::String => value.is_string(),
        ValueType::Integer => value
            .as_i64()
            .is_some_and(|i| i.abs_diff(0) <= 9_007_199_254_740_991),
        ValueType::Number => value.as_f64().is_some_and(|x| x.is_finite()),
        ValueType::Boolean => value.is_boolean(),
        ValueType::Date => value.as_str().is_some_and(|s| {
            s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
        }),
        ValueType::Enum => value
            .as_str()
            .is_some_and(|s| choices.iter().any(|v| v == s)),
        ValueType::List => false,
    }
}
fn validate_value(p: &Property, value: &Value) -> Result<()> {
    let kind_ok = if p.value_type == ValueType::List {
        value.as_array().is_some_and(|values| {
            p.items
                .as_ref()
                .is_some_and(|item| values.iter().all(|v| scalar_ok(item, v, &p.choices)))
        })
    } else {
        scalar_ok(&p.value_type, value, &p.choices)
    };
    require(kind_ok, format!("{}: expected {:?}", p.key, p.value_type))?;
    if let Some(number) = value.as_f64() {
        require(
            p.min.is_none_or(|m| number >= m) && p.max.is_none_or(|m| number <= m),
            format!("{}: value is outside the allowed range", p.key),
        )?;
    }
    let length = value
        .as_str()
        .map(|s| s.chars().count())
        .or_else(|| value.as_array().map(Vec::len));
    if let Some(length) = length {
        require(
            p.min_length.is_none_or(|m| length >= m) && p.max_length.is_none_or(|m| length <= m),
            format!("{}: invalid length", p.key),
        )?;
    }
    if let (Some(pattern), Some(text)) = (&p.pattern, value.as_str()) {
        let regex = regex::Regex::new(pattern).map_err(|e| Error::validation(e.to_string()))?;
        require(
            regex.is_match(text),
            format!("{}: value does not match its pattern", p.key),
        )?;
    }
    Ok(())
}
pub fn validate_properties(defs: &[Property], values: &Properties) -> Result<Vec<String>> {
    for key in values.keys() {
        require(
            defs.iter().any(|p| &p.key == key),
            format!("Unknown property '{key}'"),
        )?;
    }
    let mut missing = Vec::new();
    for p in defs {
        match values.get(&p.key) {
            None | Some(Value::Null) => {
                if p.required {
                    missing.push(p.key.clone());
                }
            }
            Some(v) => {
                validate_value(p, v)?;
                if p.required
                    && (v.as_str().is_some_and(|s| s.trim().is_empty())
                        || v.as_array().is_some_and(Vec::is_empty))
                {
                    missing.push(p.key.clone());
                }
            }
        }
    }
    Ok(missing)
}
pub fn with_defaults(defs: &[Property], mut values: Properties) -> Properties {
    for p in defs {
        if !values.contains_key(&p.key)
            && let Some(value) = &p.default
        {
            values.insert(p.key.clone(), value.clone());
        }
    }
    values
}
