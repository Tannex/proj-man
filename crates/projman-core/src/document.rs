use crate::{Error, NodeType, Properties, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSpan {
    pub key: String,
    pub line: usize,
    pub column: usize,
    pub required: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub properties: Properties,
    pub body: String,
    pub fields: Vec<FieldSpan>,
}
pub fn parse(text: &str) -> Result<Document> {
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(|s| s.trim_end_matches(['\r', '\n'])) != Some("--- projman") {
        return Err(Error::validation("Document must begin with '--- projman'")
            .with_details(json!({"line":1,"column":1})));
    }
    let mut offset = text.find('\n').map_or(text.len(), |i| i + 1);
    let mut properties = Properties::new();
    let mut fields = Vec::new();
    for (index, line) in lines.enumerate() {
        let line_number = index + 2;
        offset += line.len();
        let stripped = line.trim_end_matches(['\r', '\n']);
        if stripped == "---" {
            return Ok(Document {
                properties,
                body: text[offset..].into(),
                fields,
            });
        }
        let (key, value) = stripped.split_once(':').ok_or_else(|| {
            Error::validation("Expected 'property: JSON-value'")
                .with_details(json!({"line":line_number,"column":1}))
        })?;
        let key = key.trim();
        if key.is_empty() || properties.contains_key(key) {
            return Err(
                Error::validation(format!("Empty or duplicate property '{key}'"))
                    .with_details(json!({"line":line_number,"column":1})),
            );
        }
        let leading = value.len() - value.trim_start().len();
        let column = stripped.find(':').unwrap() + 2 + leading;
        let value: Value = serde_json::from_str(value.trim()).map_err(|e| {
            Error::validation(format!("{key}: {e}"))
                .with_details(json!({"line":line_number,"column":column}))
        })?;
        properties.insert(key.into(), value);
        fields.push(FieldSpan {
            key: key.into(),
            line: line_number,
            column,
            required: false,
        });
    }
    Err(Error::validation("Missing header terminator '---'"))
}
pub fn format(ty: &NodeType, properties: &Properties, body: &str) -> Result<String> {
    let mut text = String::from("--- projman\n");
    for field in &ty.properties {
        let value = properties.get(&field.key).unwrap_or(&Value::Null);
        text.push_str(&format!(
            "{}: {}\n",
            field.key,
            serde_json::to_string(value)?
        ));
    }
    text.push_str("---\n");
    text.push_str(body);
    Ok(text)
}
pub fn fields(ty: &NodeType, text: &str) -> Result<Vec<FieldSpan>> {
    let mut fields = parse(text)?.fields;
    for field in &mut fields {
        field.required = ty
            .properties
            .iter()
            .any(|p| p.key == field.key && p.required);
    }
    // Navigation is by schema order, even if the user moved header lines.
    fields.sort_by_key(|f| {
        ty.properties
            .iter()
            .position(|p| p.key == f.key)
            .unwrap_or(usize::MAX)
    });
    Ok(fields)
}
