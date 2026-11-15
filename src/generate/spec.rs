//! The subset of the Admin REST API description the generator reads.
//!
//! The description is `api/openapi.json` inside the pinned `keycloak` crate
//! tarball, so the generator and the wrapped crate cannot disagree about the
//! operations. Only what the command tree needs is modelled: paths, methods,
//! tags, parameters and whether an operation takes a body. Schemas are kept
//! unparsed ([`Parameter::schema`]) until the emitter needs the value type.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::generate::naming::{Method, ParamName};

/// The OpenAPI description of the Admin REST API.
#[derive(Debug, Deserialize)]
pub struct Spec {
    /// Version of the description itself, e.g. `3.0.1`.
    pub openapi: String,
    /// Every path of the API, keyed by its path template.
    #[serde(default)]
    pub paths: BTreeMap<String, PathItem>,
}

impl Spec {
    /// Reads a description from its JSON bytes.
    pub fn from_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// Every operation of the description, in a deterministic order.
    pub fn operations(&self) -> Vec<ApiOperation<'_>> {
        let mut operations = Vec::new();
        for (path, item) in &self.paths {
            for method in Method::ALL {
                let Some(operation) = item.operation(method) else {
                    continue;
                };
                operations.push(ApiOperation {
                    path,
                    method,
                    operation,
                    parameters: item.merged_parameters(operation),
                });
            }
        }
        operations
    }
}

/// The operations of one path.
#[derive(Debug, Default, Deserialize)]
pub struct PathItem {
    /// Parameters shared by every operation of the path.
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    pub get: Option<Operation>,
    pub post: Option<Operation>,
    pub put: Option<Operation>,
    pub delete: Option<Operation>,
    pub patch: Option<Operation>,
}

impl PathItem {
    /// The operation of `method`, when the path has it.
    pub fn operation(&self, method: Method) -> Option<&Operation> {
        match method {
            Method::Get => self.get.as_ref(),
            Method::Post => self.post.as_ref(),
            Method::Put => self.put.as_ref(),
            Method::Delete => self.delete.as_ref(),
            Method::Patch => self.patch.as_ref(),
        }
    }

    /// The parameters of `operation`: the path-level ones, overridden by the
    /// operation-level ones, in path order and without duplicates.
    fn merged_parameters<'a>(&'a self, operation: &'a Operation) -> Vec<&'a Parameter> {
        let mut merged: Vec<&Parameter> = Vec::new();
        for parameter in self.parameters.iter().chain(&operation.parameters) {
            match merged.iter_mut().find(|existing| {
                existing.name == parameter.name && existing.location == parameter.location
            }) {
                Some(existing) => *existing = parameter,
                None => merged.push(parameter),
            }
        }
        merged
    }
}

/// One HTTP operation of the API.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Operation {
    /// Tags this operation belongs to; the first one names its feature and module.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Parameters declared by this operation (path-level ones come from [`PathItem`]).
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default, rename = "requestBody")]
    pub request_body: Option<RequestBody>,
    #[serde(default)]
    pub deprecated: bool,
}

impl Operation {
    /// The first tag, i.e. the `tag-*` feature the operation is generated under.
    pub fn tag(&self) -> Option<&str> {
        self.tags.first().map(String::as_str)
    }
}

/// The body of an operation.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RequestBody {
    #[serde(default)]
    pub required: bool,
    /// Content types, keyed by media type.
    #[serde(default)]
    pub content: BTreeMap<String, Value>,
}

/// A path or query parameter.
#[derive(Clone, Debug, Deserialize)]
pub struct Parameter {
    /// The name as the description spells it, e.g. `client-uuid`.
    pub name: String,
    #[serde(rename = "in")]
    pub location: Location,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub description: Option<String>,
    /// The value type, left unparsed; `type` is sometimes a string and sometimes
    /// an array in OpenAPI 3.1, and only the emitter needs it.
    #[serde(default)]
    pub schema: Option<Value>,
}

impl Parameter {
    /// The parameter name in the spellings the command tree uses.
    pub fn param_name(&self) -> ParamName {
        ParamName::new(&self.name)
    }
}

/// Where a parameter travels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Location {
    Path,
    Query,
    Header,
    Cookie,
}

/// One operation together with the parameters that apply to it.
#[derive(Debug)]
pub struct ApiOperation<'a> {
    /// The path template, e.g. `/admin/realms/{realm}/users`.
    pub path: &'a str,
    /// The HTTP method.
    pub method: Method,
    /// The operation as described.
    pub operation: &'a Operation,
    /// Path-level and operation-level parameters, merged and deduplicated.
    pub parameters: Vec<&'a Parameter>,
}

impl<'a> ApiOperation<'a> {
    /// The tag the operation is generated under.
    pub fn tag(&self) -> Option<&'a str> {
        self.operation.tag()
    }

    /// The parameters that travel in the path, in path order.
    pub fn path_parameters(&self) -> Vec<&'a Parameter> {
        self.parameters
            .iter()
            .copied()
            .filter(|parameter| parameter.location == Location::Path)
            .collect()
    }

    /// The parameters that become flags.
    pub fn query_parameters(&self) -> Vec<&'a Parameter> {
        self.parameters
            .iter()
            .copied()
            .filter(|parameter| parameter.location == Location::Query)
            .collect()
    }

    /// Whether the operation takes a request body.
    pub fn has_body(&self) -> bool {
        self.operation.request_body.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = r#"{
      "openapi": "3.1.0",
      "paths": {
        "/admin/realms/{realm}/users": {
          "parameters": [
            {"name": "realm", "in": "path", "required": true}
          ],
          "get": {
            "tags": ["Users"],
            "summary": "Get users",
            "parameters": [
              {"name": "briefRepresentation", "in": "query", "schema": {"type": "boolean"}},
              {"name": "realm", "in": "path", "required": true, "description": "realm override"}
            ]
          },
          "post": {
            "tags": ["Users"],
            "requestBody": {"required": true, "content": {"application/json": {}}}
          }
        },
        "/admin/realms": {
          "get": {"tags": ["Realms Admin"]}
        }
      }
    }"#;

    fn spec() -> Spec {
        Spec::from_json(SPEC.as_bytes()).expect("valid description")
    }

    #[test]
    fn reads_operations_in_a_deterministic_order() {
        let spec = spec();
        let described: Vec<(&str, Method)> = spec
            .operations()
            .iter()
            .map(|operation| (operation.path, operation.method))
            .collect();
        assert_eq!(
            described,
            [
                ("/admin/realms", Method::Get),
                ("/admin/realms/{realm}/users", Method::Get),
                ("/admin/realms/{realm}/users", Method::Post),
            ]
        );
    }

    #[test]
    fn merges_path_and_operation_parameters() {
        let spec = spec();
        let operation = &spec.operations()[1];
        assert_eq!(operation.tag(), Some("Users"));
        let names: Vec<&str> = operation
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        // The path-level `realm` is overridden in place, not duplicated.
        assert_eq!(names, ["realm", "briefRepresentation"]);
        assert_eq!(
            operation.parameters[0].description.as_deref(),
            Some("realm override")
        );
        assert_eq!(operation.path_parameters().len(), 1);
        assert_eq!(
            operation.query_parameters()[0].param_name().snake,
            "brief_representation"
        );
        assert!(!operation.has_body());
    }

    #[test]
    fn sees_request_bodies() {
        let spec = spec();
        let post = &spec.operations()[2];
        assert_eq!(post.method, Method::Post);
        assert!(post.has_body());
        assert!(post.operation.request_body.as_ref().unwrap().required);
    }
}
