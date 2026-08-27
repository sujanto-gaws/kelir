//! The document aggregate, its lifecycle, its link and its query.

pub mod document;
pub mod link;
pub mod metadata;
pub mod query;
pub mod status;

pub use document::*;
pub use link::*;
pub use metadata::{MetadataEntry, MetadataSet, MetadataType};
pub use query::*;
pub use status::*;
