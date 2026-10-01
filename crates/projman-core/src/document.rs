use crate::{Error, NodeType, Properties, Result, ValueType};
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
    parse_header(text, None)
}

/// Editor input can use bare text for schema-defined textual properties.
/// Stored/API values stay typed, and the formatter always emits canonical JSON.
pub fn parse_with_schema(ty: &NodeType, text: &str) -> Result<Document> {
    parse_header(text, Some(ty))
}

fn parse_header(text: &str, ty: Option<&NodeType>) -> Result<Document> {
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
        let definition = ty
            .map(|ty| {
                ty.properties
                    .iter()
                    .find(|property| property.key == key)
                    .ok_or_else(|| {
                        Error::validation(format!("Unknown property '{key}'"))
                            .with_details(json!({"field":key,"line":line_number,"column":1}))
                    })
            })
            .transpose()?;
        let raw = value.trim();
        let textual = definition.is_some_and(|p| {
            matches!(
                p.value_type,
                ValueType::String | ValueType::Enum | ValueType::Date
            )
        });
        let value: Value = if definition.is_some() && raw.is_empty() {
            Value::Null
        } else if textual && raw != "null" && !raw.starts_with('"') {
            Value::String(raw.into())
        } else {
            serde_json::from_str(raw).map_err(|e| {
                let hint = definition.map(|property| match property.value_type {
                    ValueType::String | ValueType::Enum | ValueType::Date => {
                        "Use plain text or a complete double-quoted JSON string"
                    }
                    ValueType::Boolean => "Use true, false, or null",
                    ValueType::Integer => "Use an integer or null",
                    ValueType::Number => "Use a number or null",
                    ValueType::List => "Use a JSON array (quote text items) or null",
                });
                let message = match hint {
                    Some(hint) => format!("{key}: {hint}. {e}"),
                    None => format!("{key}: {e}"),
                };
                Error::validation(message)
                    .with_details(json!({"field":key,"line":line_number,"column":column}))
            })?
        };
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
    let mut fields = parse_with_schema(ty, text)?.fields;
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

/// Locate schema validation errors in the actual editor document, rather than
/// reporting the JSON decoder's relative "line 1 column 1" position.
pub fn validate(ty: &NodeType, doc: &Document) -> Result<Vec<String>> {
    crate::schema::validate_properties(&ty.properties, &doc.properties).map_err(|error| {
        if let Some(field) = doc
            .fields
            .iter()
            .find(|field| error.message.starts_with(&format!("{}:", field.key)))
        {
            error.with_details(json!({"field":field.key,"line":field.line,"column":field.column}))
        } else {
            error
        }
    })
}
