//! Signatures of the `keycloak` crate's generated REST methods.
//!
//! A command has to call the crate method for its operation, with the types the
//! method actually takes, so the generator reads them from the crate sources that
//! ship in the tarball (`src/rest/generated_rest/*.rs`) instead of re-deriving
//! them from schemas: the crate's own type mapping (including its patch file and
//! `TypeVec`/`TypeMap` aliases) is the only thing that has to agree with the
//! compiler, and reading it removes the chance of the two drifting apart.
//!
//! The method *name* is derived from path and method with the same rule the
//! `keycloak` generator uses ([`method_name`]); [`CrateApi::resolve`] then looks
//! it up, and a name that does not exist is an error, never a silently missing
//! command.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use heck::ToSnakeCase;

use crate::generate::naming::Method;

/// Words the `keycloak` generator suffixes with an underscore in Rust names.
const RESERVED_WORDS: &[&str] = &["type", "self", "static", "use", "if"];

/// One parameter of a crate method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrateParameter {
    /// The name as the crate spells it, e.g. `client_uuid` or `type_`.
    pub name: String,
    /// The Rust type as the crate declares it, e.g. `&str` or `Option<i32>`.
    pub ty: String,
}

/// One REST method of `KeycloakAdmin`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrateMethod {
    /// The method name, e.g. `realm_clients_get`.
    pub name: String,
    /// The generated file it lives in, e.g. `clients.rs` (for diagnostics).
    pub file: String,
    /// The parameters after `&self`, in declaration order.
    pub parameters: Vec<CrateParameter>,
    /// The `T` of `Result<T, KeycloakError>`.
    pub returns: String,
}

impl CrateMethod {
    /// The parameter that carries the request body, when the operation has one.
    pub fn body(&self) -> Option<&CrateParameter> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name == "body")
    }

    /// Whether the method discards the response ([`keycloak::DefaultResponse`]).
    pub fn discards_response(&self) -> bool {
        self.returns == "DefaultResponse"
    }
}

/// Every REST method of the crate, by name.
#[derive(Debug)]
pub struct CrateApi {
    methods: BTreeMap<String, CrateMethod>,
}

impl CrateApi {
    /// Reads every `*.rs` file of a `src/rest/generated_rest` directory.
    pub fn read(dir: &Path) -> Result<Self, SignatureError> {
        let mut files = Vec::new();
        let entries = std::fs::read_dir(dir).map_err(|error| SignatureError::Io {
            path: dir.to_owned(),
            error,
        })?;
        for entry in entries {
            let path = entry
                .map_err(|error| SignatureError::Io {
                    path: dir.to_owned(),
                    error,
                })?
                .path();
            if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
        files.sort();

        let mut methods = BTreeMap::new();
        for path in files {
            let text = std::fs::read_to_string(&path).map_err(|error| SignatureError::Io {
                path: path.clone(),
                error,
            })?;
            for method in parse_methods(&path, &text)? {
                if let Some(previous) = methods.insert(method.name.clone(), method.clone()) {
                    return Err(SignatureError::Duplicate {
                        name: method.name,
                        first: previous.file,
                        second: method.file,
                    });
                }
            }
        }
        if methods.is_empty() {
            return Err(SignatureError::Empty {
                path: dir.to_owned(),
            });
        }
        Ok(Self { methods })
    }

    /// The method of an operation, if the crate has one with the derived name.
    pub fn resolve(&self, path: &str, method: Method) -> Option<&CrateMethod> {
        self.methods.get(&method_name(path, method))
    }

    /// The method with this exact name.
    pub fn get(&self, name: &str) -> Option<&CrateMethod> {
        self.methods.get(name)
    }

    /// How many methods were read.
    pub fn len(&self) -> usize {
        self.methods.len()
    }

    /// Whether no method was read.
    pub fn is_empty(&self) -> bool {
        self.methods.is_empty()
    }
}

/// The name the `keycloak` generator gives the REST method of an operation.
///
/// The rule is a port of `Call::to_rust_method` in the crate's
/// `examples/openapi.rs`: the path without `/admin/realms`, every path parameter
/// replaced by `with_<snake_case name>` (`realm` alone), the CamelCase HTTP verb
/// appended, everything snake-cased. `/admin/realms` → `get`, and
/// `/admin/realms/{realm}/clients/{client-uuid}` → `realm_clients_with_client_uuid_get`.
pub fn method_name(path: &str, method: Method) -> String {
    let mut name = path
        .strip_prefix("/admin/realms")
        .unwrap_or(path)
        .to_owned();
    for raw in parameter_names(path) {
        let with = if raw == "realm" {
            String::new()
        } else {
            "with_".to_owned()
        } + &rust_name(&raw);
        name = name.replace(&format!("{{{raw}}}"), &with);
    }
    let verb = match method {
        Method::Get => "Get",
        Method::Post => "Post",
        Method::Put => "Put",
        Method::Delete => "Delete",
        Method::Patch => "Patch",
    };
    format!("{name}{verb}").to_snake_case()
}

/// The Rust spelling of a parameter name: snake_case, keywords suffixed.
pub fn rust_name(spec_name: &str) -> String {
    let mut name = spec_name.to_snake_case();
    while RESERVED_WORDS.contains(&name.as_str()) {
        name.push('_');
    }
    name
}

/// The `{…}` parameter names of a path, in path order.
fn parameter_names(path: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        names.push(after[..close].to_owned());
        rest = &after[close + 1..];
    }
    names
}

/// Reads every `pub async fn` of a generated REST file.
fn parse_methods(path: &Path, text: &str) -> Result<Vec<CrateMethod>, SignatureError> {
    let file = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut methods = Vec::new();
    for chunk in text.split("\n    pub async fn ").skip(1) {
        methods.push(parse_method(&file, chunk)?);
    }
    Ok(methods)
}

/// Reads one `pub async fn` from the chunk that starts at its name.
fn parse_method(file: &str, chunk: &str) -> Result<CrateMethod, SignatureError> {
    let malformed = |detail: &str| SignatureError::Malformed {
        file: file.to_owned(),
        detail: detail.to_owned(),
    };

    let open = chunk
        .find('(')
        .ok_or_else(|| malformed("no parameter list"))?;
    let name = chunk[..open].trim().to_owned();
    let close = find_close_paren(&chunk[open..])
        .ok_or_else(|| malformed(&format!("{name} has an unterminated parameter list")))?;
    let parameters_text = &chunk[open + 1..open + close];

    let tail = &chunk[open + close + 1..];
    let arrow = tail
        .find("->")
        .ok_or_else(|| malformed(&format!("{name} has no return type")))?;
    let after_arrow = tail[arrow + 2..].trim_start();
    let brace = after_arrow
        .find('{')
        .ok_or_else(|| malformed(&format!("{name} has no body")))?;
    let returns = result_type(after_arrow[..brace].trim())
        .ok_or_else(|| malformed(&format!("{name} does not return Result<T, KeycloakError>")))?;

    let mut parameters = Vec::new();
    for raw in split_top_level(parameters_text) {
        let raw = raw.trim();
        if raw.is_empty() || raw == "&self" {
            continue;
        }
        let (parameter, ty) = raw
            .split_once(':')
            .ok_or_else(|| malformed(&format!("{name} has a parameter without a type: {raw}")))?;
        parameters.push(CrateParameter {
            name: parameter.trim().to_owned(),
            ty: ty.trim().to_owned(),
        });
    }

    Ok(CrateMethod {
        name,
        file: file.to_owned(),
        parameters,
        returns,
    })
}

/// The index of the `)` that closes the `(` `text` starts with.
fn find_close_paren(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits a comma separated list, ignoring commas nested in `<…>`, `(…)` or `[…]`.
fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// The `T` of `Result<T, KeycloakError>`.
fn result_type(returns: &str) -> Option<String> {
    let inner = returns.strip_prefix("Result<")?.strip_suffix('>')?;
    let parts = split_top_level(inner);
    if parts.len() != 2 || parts[1].trim() != "KeycloakError" {
        return None;
    }
    Some(parts[0].trim().to_owned())
}

/// Why a crate method could not be read.
#[derive(Debug)]
pub enum SignatureError {
    /// A file or directory could not be read.
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
    /// A function does not have the shape the generator writes.
    Malformed { file: String, detail: String },
    /// Two files declare a method with the same name.
    Duplicate {
        name: String,
        first: String,
        second: String,
    },
    /// The directory contains no generated REST file.
    Empty { path: PathBuf },
}

impl fmt::Display for SignatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignatureError::Io { path, error } => {
                write!(f, "cannot read {}: {error}", path.display())
            }
            SignatureError::Malformed { file, detail } => write!(f, "{file}: {detail}"),
            SignatureError::Duplicate {
                name,
                first,
                second,
            } => write!(f, "method {name} is declared in both {first} and {second}"),
            SignatureError::Empty { path } => {
                write!(f, "no generated REST file in {}", path.display())
            }
        }
    }
}

impl std::error::Error for SignatureError {}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
impl<TS: KeycloakTokenSupplier> KeycloakAdmin<TS> {
    /// Get clients belonging to the realm.
    ///
    /// `GET /admin/realms/{realm}/clients`
    #[allow(clippy::too_many_arguments)]
    pub async fn realm_clients_get(
        &self,
        realm: &str,
        client_id: Option<String>,
        first: Option<i32>,
        search: Option<bool>,
        fields: Option<Vec<String>>,
    ) -> Result<TypeVec<ClientRepresentation>, KeycloakError> {
        let realm = p(realm);
        Ok(error_check(response).await?.json().await?)
    }

    pub async fn realm_clients_post(
        &self,
        realm: &str,
        body: ClientRepresentation,
    ) -> Result<DefaultResponse, KeycloakError> {
        let realm = p(realm);
        error_check(response).await.map(From::from)
    }
}
"#;

    #[test]
    fn reads_parameters_and_return_types() {
        let methods = parse_methods(Path::new("clients.rs"), SOURCE).expect("source parses");
        assert_eq!(methods.len(), 2);

        let get = &methods[0];
        assert_eq!(get.name, "realm_clients_get");
        assert_eq!(get.file, "clients.rs");
        assert_eq!(get.returns, "TypeVec<ClientRepresentation>");
        assert!(!get.discards_response());
        let names: Vec<&str> = get
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        assert_eq!(names, ["realm", "client_id", "first", "search", "fields"]);
        let types: Vec<&str> = get
            .parameters
            .iter()
            .map(|parameter| parameter.ty.as_str())
            .collect();
        assert_eq!(
            types,
            [
                "&str",
                "Option<String>",
                "Option<i32>",
                "Option<bool>",
                "Option<Vec<String>>"
            ]
        );
        assert!(get.body().is_none());

        let post = &methods[1];
        assert!(post.discards_response());
        let body = post.body().expect("post takes a body");
        assert_eq!(body.ty, "ClientRepresentation");
    }

    #[test]
    fn derives_the_crate_method_name() {
        assert_eq!(method_name("/admin/realms", Method::Get), "get");
        assert_eq!(method_name("/admin/realms", Method::Post), "post");
        assert_eq!(
            method_name("/admin/realms/{realm}", Method::Delete),
            "realm_delete"
        );
        assert_eq!(
            method_name("/admin/realms/{realm}/clients", Method::Get),
            "realm_clients_get"
        );
        assert_eq!(
            method_name(
                "/admin/realms/{realm}/clients/{client-uuid}/client-secret",
                Method::Post
            ),
            "realm_clients_with_client_uuid_client_secret_post"
        );
        // An acronym in the path keeps its word boundaries, as in the crate.
        assert_eq!(
            method_name("/admin/realms/{realm}/testSMTPConnection", Method::Post),
            "realm_test_smtp_connection_post"
        );
        // A Rust keyword parameter is suffixed, both in the name and the lookup.
        assert_eq!(
            rust_name("type"),
            "type_",
            "keywords are suffixed like the crate does"
        );
        assert_eq!(
            method_name(
                "/admin/realms/{realm}/clients/{client-uuid}/authz/resource-server/resource",
                Method::Get
            ),
            "realm_clients_with_client_uuid_authz_resource_server_resource_get"
        );
    }

    #[test]
    fn reads_a_directory_of_methods() {
        let dir = std::env::temp_dir().join(format!(
            "keycloak-cli-signatures-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time moves forward")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("clients.rs"), SOURCE).expect("write file");
        std::fs::write(dir.join("ignored.txt"), "not rust").expect("write file");

        let api = CrateApi::read(&dir).expect("reads the directory");
        assert_eq!(api.len(), 2);
        assert!(!api.is_empty());
        assert!(api
            .resolve("/admin/realms/{realm}/clients", Method::Get)
            .is_some());
        assert!(api
            .resolve("/admin/realms/{realm}/clients", Method::Delete)
            .is_none());
        assert_eq!(
            api.get("realm_clients_post")
                .map(|method| method.name.as_str()),
            Some("realm_clients_post")
        );

        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn rejects_functions_the_generator_does_not_write() {
        let text = "\n    pub async fn broken(&self, realm: &str) -> ClientRepresentation {\n";
        let error = parse_methods(Path::new("broken.rs"), text).expect_err("no Result");
        assert!(
            error.to_string().contains("does not return Result"),
            "{error}"
        );
        assert!(matches!(error, SignatureError::Malformed { .. }));
    }

    #[test]
    fn splits_only_at_top_level_commas() {
        assert_eq!(
            split_top_level("a: TypeMap<String, Value>, b: Option<Vec<String>>"),
            ["a: TypeMap<String, Value>", " b: Option<Vec<String>>"]
        );
        assert_eq!(
            result_type("Result<TypeMap<String, Value>, KeycloakError>").as_deref(),
            Some("TypeMap<String, Value>")
        );
        assert_eq!(result_type("Result<Value>"), None);
        assert_eq!(result_type("Result<Value, OtherError>"), None);
    }
}
