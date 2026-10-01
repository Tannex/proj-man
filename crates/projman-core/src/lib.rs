//! Editor-independent application model and validated graph operations.
mod error;
pub mod schema;
pub use error::{Error, Result};
pub use schema::{NodeType, Properties, Property, RelationType, Schema, ValueType};
pub mod graph;
pub use graph::{Action, Edge, Mutation, Node, Operation, Receipt, State};
pub mod document;
pub mod explorer;
pub mod proposal;
