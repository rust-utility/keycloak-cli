//! Rendering of responses.
//!
//! Every command returns a [`serde_json::Value`] (the generated dispatch
//! serialises the crate's typed response), so rendering is one place: JSON for
//! pipes, a table for terminals, and YAML, markdown or HTML when asked for. A
//! listing is a table of its objects, a single object is a two-column table, and
//! `--fields` projects either to the columns that were asked for.
//!
//! The table, markdown and HTML renderers are small and hand-written on purpose:
//! the output is data, not prose, so a table generator plus an escaper is all that
//! is needed (`pulldown-cmark`/`termimad` render markdown *to* a terminal, and
//! `minijinja` is for the report of Q9).

use std::fmt::Write as _;

use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};
use serde_json::{Map, Value};

use crate::error::Error;

/// How a response is printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// A table when stdout is a terminal, JSON otherwise.
    Auto,
    /// Pretty JSON.
    Json,
    /// YAML.
    Yaml,
    /// A table with borders.
    Table,
    /// A markdown table.
    Markdown,
    /// An HTML table fragment.
    Html,
}

impl Format {
    /// Every spelling `--output` accepts.
    pub const NAMES: &'static [&'static str] =
        &["auto", "json", "yaml", "table", "markdown", "html"];

    /// Parses the value of `--output`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "auto" => Some(Format::Auto),
            "json" => Some(Format::Json),
            "yaml" => Some(Format::Yaml),
            "table" => Some(Format::Table),
            "markdown" => Some(Format::Markdown),
            "html" => Some(Format::Html),
            _ => None,
        }
    }

    /// What this format means for a stream: `auto` is a table on a terminal and
    /// JSON when the output is piped (F22).
    pub fn resolve(self, stdout_is_a_terminal: bool) -> Format {
        match self {
            Format::Auto if stdout_is_a_terminal => Format::Table,
            Format::Auto => Format::Json,
            format => format,
        }
    }
}

/// Renders `value` in `format`, with `fields` projecting the columns.
///
/// A null response prints nothing: operations that answer without a body have
/// nothing to show.
pub fn render(
    value: &Value,
    fields: &[String],
    format: Format,
    stdout_is_a_terminal: bool,
) -> Result<String, Error> {
    if value.is_null() {
        return Ok(String::new());
    }
    let projected = project(value, fields);
    match format.resolve(stdout_is_a_terminal) {
        Format::Json | Format::Auto => Ok(json(&projected)),
        Format::Yaml => yaml(&projected),
        Format::Table => Ok(table(&projected)),
        Format::Markdown => Ok(markdown(&projected)),
        Format::Html => Ok(html(&projected)),
    }
}

/// Keeps only `fields` of objects (and of the objects in a listing).
///
/// An empty list keeps everything; unknown fields are simply absent from the
/// output, the same way JSON serialisation drops them.
fn project(value: &Value, fields: &[String]) -> Value {
    if fields.is_empty() {
        return value.clone();
    }
    let keep = |object: &Map<String, Value>| -> Value {
        let mut projected = Map::new();
        for field in fields {
            if let Some(value) = object.get(field) {
                projected.insert(field.clone(), value.clone());
            }
        }
        Value::Object(projected)
    };
    match value {
        Value::Object(object) => keep(object),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| match item {
                    Value::Object(object) => keep(object),
                    other => other.clone(),
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Pretty JSON, newline terminated.
fn json(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_owned());
    text.push('\n');
    text
}

/// YAML.
fn yaml(value: &Value) -> Result<String, Error> {
    serde_yaml_ng::to_string(value).map_err(|error| Error::Format(error.to_string()))
}

/// A table with borders.
fn table(value: &Value) -> String {
    let grid = Grid::of(value);
    let mut table = Table::new();
    table
        .load_style(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Disabled)
        .set_header(grid.header);
    for row in grid.rows {
        table.add_row(row);
    }
    let mut text = table.to_string();
    text.push('\n');
    text
}

/// A markdown table.
fn markdown(value: &Value) -> String {
    let grid = Grid::of(value);
    let mut text = String::new();
    let header = escape_markdown_row(&grid.header);
    text.push_str(&header);
    text.push('\n');
    let mut separator = String::from("|");
    for _ in &grid.header {
        separator.push_str(" --- |");
    }
    text.push_str(&separator);
    text.push('\n');
    for row in &grid.rows {
        text.push_str(&escape_markdown_row(row));
        text.push('\n');
    }
    text
}

/// An HTML table fragment, for embedding in a page (the report of Q9).
fn html(value: &Value) -> String {
    let grid = Grid::of(value);
    let mut text = String::from("<table>\n  <thead>\n    <tr>");
    for cell in &grid.header {
        let _ = write!(text, "<th>{}</th>", escape_html(cell));
    }
    text.push_str("</tr>\n  </thead>\n  <tbody>\n");
    for row in &grid.rows {
        text.push_str("    <tr>");
        for cell in row {
            let _ = write!(text, "<td>{}</td>", escape_html(cell));
        }
        text.push_str("</tr>\n");
    }
    text.push_str("  </tbody>\n</table>\n");
    text
}

/// A response as a grid of strings, whatever shape it has.
struct Grid {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Grid {
    /// Lays a response out: a listing of objects is one row each, an object is a
    /// field/value pair per entry, and a scalar is a single cell.
    fn of(value: &Value) -> Self {
        match value {
            Value::Array(items) if items.iter().all(Value::is_object) && !items.is_empty() => {
                let header = columns(items);
                let rows = items
                    .iter()
                    .map(|item| {
                        header
                            .iter()
                            .map(|column| cell(item.get(column).unwrap_or(&Value::Null)))
                            .collect()
                    })
                    .collect();
                Self { header, rows }
            }
            Value::Object(object) => {
                let header = vec!["field".to_owned(), "value".to_owned()];
                let rows = object
                    .iter()
                    .map(|(field, value)| vec![field.clone(), cell(value)])
                    .collect();
                Self { header, rows }
            }
            Value::Array(items) => Self {
                header: vec!["value".to_owned()],
                rows: items.iter().map(|item| vec![cell(item)]).collect(),
            },
            other => Self {
                header: vec!["value".to_owned()],
                rows: vec![vec![cell(other)]],
            },
        }
    }
}

/// The columns of a listing: the keys of every object, in the order they appear.
fn columns(items: &[Value]) -> Vec<String> {
    let mut columns: Vec<String> = Vec::new();
    for item in items {
        if let Value::Object(object) = item {
            for key in object.keys() {
                if !columns.iter().any(|column| column == key) {
                    columns.push(key.clone());
                }
            }
        }
    }
    columns
}

/// A cell: scalars as they are, nested values as compact JSON, null as empty.
fn cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Bool(_) | Value::Number(_) => value.to_string(),
        nested => serde_json::to_string(nested).unwrap_or_default(),
    }
}

/// A markdown row, with `|` escaped so it cannot break the table.
fn escape_markdown_row(cells: &[String]) -> String {
    let mut row = String::from("|");
    for cell in cells {
        let _ = write!(row, " {} |", cell.replace('|', "\\|"));
    }
    row
}

/// HTML text escaping.
fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn listing() -> Value {
        json!([
            {"id": "c-1", "clientId": "web", "enabled": true, "attributes": {"a": "b"}},
            {"id": "c-2", "clientId": "api", "enabled": false, "missing": null}
        ])
    }

    #[test]
    fn names_round_trip() {
        for name in Format::NAMES {
            let format = Format::from_name(name).expect("a known format");
            assert_eq!(
                format.resolve(false),
                match format {
                    Format::Auto => Format::Json,
                    other => other,
                }
            );
        }
        assert_eq!(Format::from_name("xml"), None);
        assert_eq!(Format::Auto.resolve(true), Format::Table);
        assert_eq!(Format::Auto.resolve(false), Format::Json);
        assert_eq!(Format::Yaml.resolve(true), Format::Yaml);
    }

    #[test]
    fn json_is_pretty_and_newline_terminated() {
        let text = render(&listing(), &[], Format::Json, false).expect("renders");
        assert!(text.starts_with("[\n  {\n"), "{text}");
        assert!(text.ends_with("}\n]\n") || text.ends_with("]\n"), "{text}");
        assert!(text.contains("\"clientId\": \"web\""), "{text}");
    }

    #[test]
    fn yaml_is_rendered() {
        let text = render(&listing(), &[], Format::Yaml, false).expect("renders");
        assert!(text.contains("clientId: web"), "{text}");
        assert!(text.contains("id: c-1"), "{text}");
        assert!(text.contains("missing: null"), "{text}");
    }

    #[test]
    fn a_listing_becomes_a_table_with_every_column() {
        let text = render(&listing(), &[], Format::Table, false).expect("renders");
        for expected in ["id", "clientId", "enabled", "attributes", "missing"] {
            assert!(text.contains(expected), "{expected} missing:\n{text}");
        }
        assert!(text.contains("web"), "{text}");
        // Nested values are compact JSON in their cell.
        assert!(text.contains(r#"{"a":"b"}"#), "{text}");
        assert!(text.contains('┌') || text.contains('+'), "{text}");
    }

    #[test]
    fn fields_project_the_columns() {
        let fields = vec!["clientId".to_owned(), "id".to_owned()];
        let text = render(&listing(), &fields, Format::Table, false).expect("renders");
        assert!(text.contains("clientId"), "{text}");
        assert!(text.contains("id"), "{text}");
        assert!(!text.contains("enabled"), "{text}");
        assert!(!text.contains("attributes"), "{text}");
        // Order follows --fields.
        let header: Vec<usize> = ["clientId", "id"]
            .iter()
            .map(|column| text.find(column).expect("a column"))
            .collect();
        assert!(header[0] < header[1], "{text}");

        // JSON is projected too.
        let text = render(&listing(), &fields, Format::Json, false).expect("renders");
        assert!(text.contains("clientId"), "{text}");
        assert!(!text.contains("enabled"), "{text}");
    }

    #[test]
    fn a_single_object_becomes_field_and_value() {
        let text = render(
            &json!({"realm": "master", "enabled": true}),
            &[],
            Format::Table,
            false,
        )
        .expect("renders");
        assert!(text.contains("field"), "{text}");
        assert!(text.contains("value"), "{text}");
        assert!(text.contains("realm"), "{text}");
        assert!(text.contains("master"), "{text}");
    }

    #[test]
    fn markdown_escapes_pipes_and_keeps_columns() {
        let text = render(
            &json!([{"name": "a|b", "id": 1}]),
            &[],
            Format::Markdown,
            false,
        )
        .expect("renders");
        // Columns follow the object's key order (serde_json sorts them).
        assert_eq!(text.lines().next(), Some("| id | name |"), "{text}");
        assert_eq!(text.lines().nth(1), Some("| --- | --- |"), "{text}");
        assert!(text.contains("a\\|b"), "{text}");
    }

    #[test]
    fn html_escapes_markup() {
        let text = render(
            &json!([{"name": "<script>", "note": "a & b"}]),
            &[],
            Format::Html,
            false,
        )
        .expect("renders");
        assert!(text.contains("<th>name</th>"), "{text}");
        assert!(text.contains("&lt;script&gt;"), "{text}");
        assert!(text.contains("a &amp; b"), "{text}");
        assert!(!text.contains("<script>"), "{text}");
    }

    #[test]
    fn auto_picks_a_table_for_a_terminal_and_json_for_a_pipe() {
        let terminal = render(&listing(), &[], Format::Auto, true).expect("renders");
        assert!(
            terminal.contains('┌') || terminal.contains('+'),
            "{terminal}"
        );
        let piped = render(&listing(), &[], Format::Auto, false).expect("renders");
        assert!(piped.starts_with('['), "{piped}");
    }

    #[test]
    fn a_bodyless_response_prints_nothing() {
        for format in [
            Format::Json,
            Format::Yaml,
            Format::Table,
            Format::Markdown,
            Format::Html,
        ] {
            assert_eq!(
                render(&Value::Null, &[], format, true).expect("renders"),
                ""
            );
        }
    }

    #[test]
    fn scalars_and_plain_lists_still_render() {
        let text = render(&json!(42), &[], Format::Table, false).expect("renders");
        assert!(text.contains("42"), "{text}");
        let text = render(&json!(["a", "b"]), &[], Format::Table, false).expect("renders");
        assert!(text.contains("a") && text.contains("b"), "{text}");
        let text = render(&json!([]), &[], Format::Table, false).expect("renders");
        assert!(!text.is_empty(), "an empty listing still has a border");
    }
}
