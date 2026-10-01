use serde::Serialize;
use serde_json::{Value, json};
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub code: String,
    pub message: String,
    pub details: Value,
}
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Value::Null,
        }
    }
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new("validation", message)
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("conflict", message)
    }
    pub fn missing(kind: &str, id: &str) -> Self {
        Self::new("not_found", format!("{kind} '{id}' does not exist"))
    }
    pub fn with_details(mut self, details: impl Serialize) -> Self {
        self.details = serde_json::to_value(details).unwrap_or(json!(null));
        self
    }
    pub fn exit_code(&self) -> i32 {
        match self.code.as_str() {
            "usage" => 2,
            "validation" => 3,
            "conflict" => 4,
            "unavailable" => 5,
            "not_found" => 6,
            "configuration" => 7,
            "cancelled" => 130,
            _ => 1,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::validation(e.to_string())
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}
