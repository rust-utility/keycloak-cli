//! The `report` document: users, events and sessions with ASCII charts.
//!
//! A report is a snapshot, not a table: a title, a few sections, and for each
//! section a summary line plus horizontal Unicode bars. Both renderers (markdown
//! and a self-contained HTML page) are written here, so the same data produces a
//! document that reads well in a terminal or in a browser and needs no assets.
//!
//! The data comes from the endpoints the CLI already exposes (F26): users and
//! their enabled/disabled split, event counts by type, and active sessions per
//! client. A section whose endpoint fails says so instead of failing the report —
//! Keycloak only serves events when the events feature is enabled, and a report
//! without them is still a report.

use keycloak::KeycloakAdmin;
use keycloak::KeycloakTokenSupplier;
use serde::Serialize;

use crate::config::Config;
use crate::error::Error;

/// How wide the bars of a chart may get.
const BAR_WIDTH: usize = 36;

/// How many bars a chart shows before folding the rest into `other`.
#[allow(dead_code)] // a build with no tag features has no sections to chart
const MAX_BARS: usize = 10;

/// How many events a report reads.
#[allow(dead_code)] // the events section needs `tag-realms-admin`
const EVENT_SAMPLE: i32 = 1000;

/// A section of the report.
#[derive(Debug, PartialEq, Eq)]
pub struct Section {
    /// Heading, e.g. `Users`.
    pub title: String,
    /// One line of prose under the heading.
    pub summary: String,
    /// The chart: labelled values, already sorted.
    pub bars: Vec<Bar>,
}

/// One bar of a chart.
#[derive(Debug, PartialEq, Eq)]
pub struct Bar {
    pub label: String,
    pub value: u64,
}

/// The report document.
#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    /// Base URL the data came from.
    pub server: String,
    /// Realm the data came from.
    pub realm: String,
    /// Unix seconds at which the report was collected.
    pub generated_at: u64,
    pub sections: Vec<Section>,
}

impl Report {
    /// An empty report for `config`, ready to have sections added.
    pub fn new(config: &Config) -> Self {
        Self {
            server: config.server.clone(),
            realm: config.realm.clone(),
            generated_at: crate::session::now(),
            sections: Vec::new(),
        }
    }

    /// The document as markdown.
    pub fn markdown(&self) -> String {
        let mut text = format!("# keycloak-cli report — realm `{}`\n\n", self.realm);
        text.push_str(&format!("- server: `{}`\n", self.server));
        text.push_str(&format!(
            "- generated: {} (unix seconds)\n",
            self.generated_at
        ));
        text.push('\n');
        for section in &self.sections {
            text.push_str(&format!("## {}\n\n", section.title));
            text.push_str(&section.summary);
            text.push_str("\n\n");
            if !section.bars.is_empty() {
                text.push_str("```\n");
                for line in chart_lines(&section.bars) {
                    text.push_str(&line);
                    text.push('\n');
                }
                text.push_str("```\n\n");
            }
        }
        text
    }

    /// The document as a self-contained HTML page (inline styles, no assets).
    pub fn html(&self) -> String {
        let mut text =
            String::from("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
        text.push_str(&format!(
            "<title>keycloak-cli report — realm {}</title>\n",
            escape_html(&self.realm)
        ));
        text.push_str(
            "<style>
body { font-family: system-ui, sans-serif; margin: 2rem auto; max-width: 56rem; padding: 0 1rem; color: #1f2328; }
h1 { font-size: 1.4rem; margin-bottom: 0.25rem; }
h2 { font-size: 1.1rem; margin-top: 2rem; margin-bottom: 0.25rem; }
.meta { color: #57606a; font-size: 0.9rem; margin-top: 0; }
pre { background: #f6f8fa; padding: 0.75rem 1rem; overflow-x: auto; border-radius: 6px; }
</style>
</head>
<body>
",
        );
        text.push_str("<h1>keycloak-cli report</h1>\n");
        text.push_str(&format!(
            "<p class=\"meta\">realm <strong>{}</strong> — server <code>{}</code> — generated {} (unix seconds)</p>\n",
            escape_html(&self.realm),
            escape_html(&self.server),
            self.generated_at
        ));
        for section in &self.sections {
            text.push_str(&format!(
                "<section>\n<h2>{}</h2>\n",
                escape_html(&section.title)
            ));
            text.push_str(&format!("<p>{}</p>\n", escape_html(&section.summary)));
            if !section.bars.is_empty() {
                text.push_str("<pre>");
                text.push_str(&escape_html(&chart_lines(&section.bars).join("\n")));
                text.push_str("</pre>\n");
            }
            text.push_str("</section>\n");
        }
        text.push_str("</body>\n</html>\n");
        text
    }
}

/// Renders the bars: `label  ████████ 8`, scaled so the longest bar is
/// [`BAR_WIDTH`] wide.
fn chart_lines(bars: &[Bar]) -> Vec<String> {
    let label_width = bars
        .iter()
        .map(|bar| bar.label.chars().count())
        .max()
        .unwrap_or(0)
        .min(24);
    let largest = bars.iter().map(|bar| bar.value).max().unwrap_or(0);

    bars.iter()
        .map(|bar| {
            let label = truncate(&bar.label, label_width);
            let width = if largest == 0 {
                0
            } else {
                // Always show something for a non-zero value, in proportion.
                ((bar.value as f64 / largest as f64) * BAR_WIDTH as f64).round() as usize
            }
            .max(usize::from(bar.value > 0));
            format!(
                "{label:<label_width$}  {:<width$} {}",
                "█".repeat(width),
                bar.value
            )
        })
        .collect()
}

/// Cuts a label to `width` characters, marking the cut.
fn truncate(label: &str, width: usize) -> String {
    if label.chars().count() <= width {
        return label.to_owned();
    }
    let mut cut: String = label.chars().take(width.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// Turns labelled counts into bars: sorted by value, capped at [`MAX_BARS`].
#[allow(dead_code)] // used by the sections and the tests, which feature-gate separately
fn chart(mut counts: Vec<(String, u64)>) -> Vec<Bar> {
    counts.retain(|(label, value)| !label.is_empty() && *value > 0);
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    if counts.len() > MAX_BARS {
        let rest: u64 = counts[MAX_BARS..].iter().map(|(_, value)| value).sum();
        counts.truncate(MAX_BARS);
        counts.push((format!("other ({} kinds)", counts.len()), rest));
    }
    counts
        .into_iter()
        .map(|(label, value)| Bar { label, value })
        .collect()
}

/// Collects the report with `admin`.
#[allow(unused_variables)] // a build with no tag features collects nothing
pub async fn collect<TS: KeycloakTokenSupplier>(
    admin: &KeycloakAdmin<TS>,
    config: &Config,
) -> Result<Report, Error> {
    #[allow(unused_mut, unused_variables)]
    let mut report = Report::new(config);

    #[cfg(feature = "tag-users")]
    report.sections.push(users(admin, &config.realm).await);

    #[cfg(feature = "tag-realms-admin")]
    {
        report.sections.push(events(admin, &config.realm).await);
        report.sections.push(sessions(admin, &config.realm).await);
    }

    Ok(report)
}

/// The users section: how many exist and how they split by state.
#[cfg(feature = "tag-users")]
async fn users<TS: KeycloakTokenSupplier>(admin: &KeycloakAdmin<TS>, realm: &str) -> Section {
    let count = |enabled: Option<bool>| async move {
        admin
            .realm_users_count_get(
                realm, None, None, None, None, enabled, None, None, None, None, None, None, None,
                None,
            )
            .await
    };
    match count(None).await {
        Err(error) => unavailable("Users", error),
        Ok(total) => {
            let enabled = count(Some(true)).await.ok();
            let disabled = count(Some(false)).await.ok();
            let mut bars = Vec::new();
            if let Some(enabled) = enabled {
                bars.push(("enabled".to_owned(), enabled.max(0) as u64));
            }
            if let Some(disabled) = disabled {
                bars.push(("disabled".to_owned(), disabled.max(0) as u64));
            }
            let summary = match (enabled, disabled) {
                (Some(enabled), Some(disabled)) => {
                    format!("{total} users in total: {enabled} enabled, {disabled} disabled.")
                }
                _ => format!("{total} users in total."),
            };
            Section {
                title: "Users".to_owned(),
                summary,
                bars: chart(bars),
            }
        }
    }
}

/// The events section: how many events of each type the sample holds.
#[cfg(feature = "tag-realms-admin")]
async fn events<TS: KeycloakTokenSupplier>(admin: &KeycloakAdmin<TS>, realm: &str) -> Section {
    let events = admin
        .realm_events_get(
            realm,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(EVENT_SAMPLE),
            None,
            None,
        )
        .await;
    match events {
        Err(error) => unavailable("Events by type", error),
        Ok(events) => {
            let mut counts: std::collections::BTreeMap<String, u64> =
                std::collections::BTreeMap::new();
            for event in &events {
                let kind = event
                    .type_
                    .as_ref()
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_owned());
                *counts.entry(kind).or_default() += 1;
            }
            let summary = if events.is_empty() {
                "No events in the sample (Keycloak only stores events when the events \
                 feature is enabled)."
                    .to_owned()
            } else {
                format!(
                    "{} events in the last sample of at most {EVENT_SAMPLE}.",
                    events.len()
                )
            };
            Section {
                title: "Events by type".to_owned(),
                summary,
                bars: chart(counts.into_iter().collect()),
            }
        }
    }
}

/// The sessions section: active sessions per client.
#[cfg(feature = "tag-realms-admin")]
async fn sessions<TS: KeycloakTokenSupplier>(admin: &KeycloakAdmin<TS>, realm: &str) -> Section {
    let stats = admin.realm_client_session_stats_get(realm).await;
    match stats {
        Err(error) => unavailable("Sessions per client", error),
        Ok(stats) => {
            let mut counts = Vec::new();
            for stat in &stats {
                let label = stat
                    .get("clientId")
                    .or_else(|| stat.get("client-id"))
                    .or_else(|| stat.get("client_id"))
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_owned());
                let active = stat
                    .get("active")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                counts.push((label, active));
            }
            let total: u64 = counts.iter().map(|(_, active)| *active).sum();
            Section {
                title: "Sessions per client".to_owned(),
                summary: format!("{total} active sessions in {} clients.", counts.len()),
                bars: chart(counts),
            }
        }
    }
}

/// A section whose endpoint failed.
#[allow(dead_code)]
fn unavailable(title: &str, error: impl std::fmt::Display) -> Section {
    Section {
        title: title.to_owned(),
        summary: format!("Unavailable: {error}"),
        bars: Vec::new(),
    }
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

/// A report as JSON, for `--output json`.
#[derive(Serialize)]
struct JsonReport<'a> {
    server: &'a str,
    realm: &'a str,
    generated_at: u64,
    sections: Vec<JsonSection<'a>>,
}

#[derive(Serialize)]
struct JsonSection<'a> {
    title: &'a str,
    summary: &'a str,
    bars: Vec<JsonBar<'a>>,
}

#[derive(Serialize)]
struct JsonBar<'a> {
    label: &'a str,
    value: u64,
}

impl Report {
    /// The document as JSON.
    pub fn json(&self) -> Result<String, Error> {
        let report = JsonReport {
            server: &self.server,
            realm: &self.realm,
            generated_at: self.generated_at,
            sections: self
                .sections
                .iter()
                .map(|section| JsonSection {
                    title: &section.title,
                    summary: &section.summary,
                    bars: section
                        .bars
                        .iter()
                        .map(|bar| JsonBar {
                            label: &bar.label,
                            value: bar.value,
                        })
                        .collect(),
                })
                .collect(),
        };
        let mut text = serde_json::to_string_pretty(&report)?;
        text.push('\n');
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Report {
        Report {
            server: "http://localhost:8080".to_owned(),
            realm: "master".to_owned(),
            generated_at: 1_760_000_000,
            sections: vec![
                Section {
                    title: "Users".to_owned(),
                    summary: "10 users in total: 8 enabled, 2 disabled.".to_owned(),
                    bars: chart(vec![("enabled".to_owned(), 8), ("disabled".to_owned(), 2)]),
                },
                Section {
                    title: "Events by type".to_owned(),
                    summary: "Unavailable: no events".to_owned(),
                    bars: Vec::new(),
                },
            ],
        }
    }

    #[test]
    fn bars_scale_to_the_largest_value() {
        let bars = chart(vec![
            ("a".to_owned(), 10),
            ("b".to_owned(), 5),
            ("c".to_owned(), 1),
        ]);
        assert_eq!(bars[0].value, 10);
        let lines = chart_lines(&bars);
        assert_eq!(lines[0], format!("a  {} 10", "█".repeat(BAR_WIDTH)));
        assert!(
            lines[1].contains('█') && lines[1].ends_with(" 5"),
            "{lines:?}"
        );
        // A single hit still shows a block, it is not invisible.
        assert!(
            lines[2].contains('█') && lines[2].ends_with(" 1"),
            "{lines:?}"
        );
    }

    #[test]
    fn empty_values_and_labels_are_dropped() {
        let bars = chart(vec![
            ("zero".to_owned(), 0),
            (String::new(), 5),
            ("kept".to_owned(), 3),
        ]);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].label, "kept");
        assert!(chart_lines(&[]).is_empty());
    }

    #[test]
    fn many_kinds_fold_into_other() {
        let counts: Vec<(String, u64)> = (0..MAX_BARS + 5)
            .map(|index| (format!("type-{index:02}"), 1))
            .collect();
        let bars = chart(counts);
        assert_eq!(bars.len(), MAX_BARS + 1, "{bars:?}");
        assert_eq!(bars[MAX_BARS].label, format!("other ({MAX_BARS} kinds)"));
        assert_eq!(bars[MAX_BARS].value, 5);
    }

    #[test]
    fn long_labels_are_truncated() {
        let label = "a-very-long-label-that-would-push-the-chart-off-the-screen";
        let bars = chart(vec![(label.to_owned(), 1)]);
        let line = &chart_lines(&bars)[0];
        let shown = line.split("  ").next().expect("a label");
        assert_eq!(shown.chars().count(), 24, "{line}");
        assert!(
            shown.starts_with("a-very-long") && shown.ends_with('…'),
            "{line}"
        );
    }

    #[test]
    fn markdown_has_the_sections_and_charts() {
        let text = report().markdown();
        assert!(
            text.starts_with("# keycloak-cli report — realm `master`"),
            "{text}"
        );
        assert!(text.contains("- server: `http://localhost:8080`"), "{text}");
        assert!(text.contains("## Users"), "{text}");
        assert!(
            text.contains("10 users in total: 8 enabled, 2 disabled."),
            "{text}"
        );
        assert!(text.contains("```\nenabled"), "{text}");
        assert!(text.contains("## Events by type"), "{text}");
        assert!(text.contains("Unavailable: no events"), "{text}");
    }

    #[test]
    fn html_is_self_contained_and_escaped() {
        let mut report = report();
        report.sections[0].summary = "<script>alert(1)</script> & co".to_owned();
        let text = report.html();
        assert!(text.starts_with("<!doctype html>"), "{text}");
        assert!(text.contains("<style>"), "{text}");
        assert!(text.contains("</html>"), "{text}");
        assert!(!text.contains("<script>"), "{text}");
        assert!(text.contains("&lt;script&gt;"), "{text}");
        assert!(text.contains("&amp; co"), "{text}");
        // No external assets.
        for forbidden in ["http://", "https://", "<link", "<script src"] {
            if forbidden == "http://" {
                // The server URL is data, not an asset; check for asset tags instead.
                assert!(!text.contains("<link"), "{text}");
                continue;
            }
            assert!(!text.contains(forbidden), "{text}");
        }
        assert!(text.contains("<pre>"), "{text}");
    }

    #[test]
    fn json_carries_the_two_shapes() {
        let text = report().json().expect("json renders");
        assert!(text.contains("\"realm\": \"master\""), "{text}");
        assert!(text.contains("\"title\": \"Users\""), "{text}");
        assert!(text.contains("\"label\": \"enabled\""), "{text}");
        assert!(text.contains("\"value\": 8"), "{text}");
    }
}
