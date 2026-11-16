//! Every operation `ApiDoc::openapi()` documents, read the way a walk reads
//! them.
//!
//! Moved here from `tests/no_token_walk.rs` for `tests/audit_walk.rs` (#960):
//! two walks over the same document should read the same operations, and a
//! second copy would be a second place for the reading to drift.

use axum::http::Method;
use serde_json::Value;
use utoipa::OpenApi;
use uuid::Uuid;

use kelir_backend::router::ApiDoc;

/// The methods a path item can carry, by the key the document gives them.
pub const METHODS: [(&str, Method); 5] = [
    ("get", Method::GET),
    ("post", Method::POST),
    ("put", Method::PUT),
    ("patch", Method::PATCH),
    ("delete", Method::DELETE),
];

/// One operation of the document.
pub struct Operation {
    pub method: Method,
    pub template: String,
    /// The path with every `{capture}` resolved.
    pub path: String,
    pub declares_bearer: bool,
}

impl Operation {
    /// `METHOD template`, the name each walk lists an operation by.
    pub fn name(&self) -> String {
        format!("{} {}", self.method, self.template)
    }

    /// Whether the operation changes something: every method but `GET`.
    pub fn is_mutating(&self) -> bool {
        self.method != Method::GET
    }
}

/// A value for a path capture: the annotation's `example` if it gives one, and
/// otherwise a fresh uuid, which is what almost all of them are.
pub fn capture_value(parameters: Option<&Vec<Value>>, name: &str) -> String {
    parameters
        .into_iter()
        .flatten()
        .filter(|parameter| parameter["in"] == "path" && parameter["name"] == name)
        .find_map(|parameter| {
            parameter["example"]
                .as_str()
                .or_else(|| parameter["schema"]["example"].as_str())
        })
        .map_or_else(|| Uuid::now_v7().to_string(), ToOwned::to_owned)
}

/// Every operation in the document, whatever it declares.
pub fn every_operation() -> Vec<Operation> {
    let document = serde_json::to_value(ApiDoc::openapi()).expect("the document serialises");
    let paths = document["paths"]
        .as_object()
        .expect("the document has paths");

    let mut operations = Vec::new();
    for (template, item) in paths {
        for (key, method) in METHODS {
            let operation = &item[key];
            if operation.is_null() {
                continue;
            }

            let mut path = template.clone();
            while let Some(start) = path.find('{') {
                let end = start + path[start..].find('}').expect("a closed capture");
                let value =
                    capture_value(operation["parameters"].as_array(), &path[start + 1..end]);
                path.replace_range(start..=end, &value);
            }

            operations.push(Operation {
                method,
                template: template.clone(),
                path,
                declares_bearer: operation["security"].as_array().is_some_and(|schemes| {
                    schemes.iter().any(|scheme| scheme.get("bearer").is_some())
                }),
            });
        }
    }

    operations
}
