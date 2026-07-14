//! DOI resolver fetcher backed by the Crossref works API.

use crate::client::FetchOptions;
use crate::convert::html_to_markdown;
use crate::error::FetchError;
use crate::fetchers::default::{read_full_body, transport_request};
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use serde_json::Value;
use std::time::Duration;
use url::Url;

const API_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_BODY_SIZE: usize = 5 * 1024 * 1024;

pub struct CrossrefFetcher;

impl CrossrefFetcher {
    pub fn new() -> Self {
        Self
    }

    fn doi(url: &Url) -> Option<String> {
        if !matches!(url.host_str()?, "doi.org" | "dx.doi.org") {
            return None;
        }
        let doi = percent_encoding::percent_decode_str(url.path().trim_start_matches('/'))
            .decode_utf8()
            .ok()?
            .trim()
            .to_string();
        valid_doi(&doi).then(|| doi.to_ascii_lowercase())
    }
}

impl Default for CrossrefFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for CrossrefFetcher {
    fn name(&self) -> &'static str {
        "crossref"
    }

    fn matches(&self, url: &Url) -> bool {
        Self::doi(url).is_some()
    }

    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        let request = request.normalized_for_fetch()?;
        let page_url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;
        let doi = Self::doi(&page_url)
            .ok_or_else(|| FetchError::FetcherError("Not a valid DOI resolver URL".into()))?;
        let mut api_url = Url::parse("https://api.crossref.org/works/")
            .map_err(|_| FetchError::InvalidUrlScheme)?;
        api_url
            .path_segments_mut()
            .map_err(|_| FetchError::InvalidUrlScheme)?
            .push(&doi);
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT))
                .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT)),
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let response = transport_request(
            api_url,
            reqwest::Method::GET,
            headers,
            options,
            API_TIMEOUT,
            "api.crossref.org",
            443,
        )
        .await?;
        let status_code = response.status;
        if !(200..300).contains(&status_code) {
            return Ok(FetchResponse {
                url: request.url,
                status_code,
                error: Some(match status_code {
                    404 => format!("DOI not found: {doi}"),
                    403 | 429 => "Crossref API rate limit exceeded or access denied".into(),
                    _ => format!("Crossref API error: HTTP {status_code}"),
                }),
                ..Default::default()
            });
        }
        let body = read_full_body(response, options).await?;
        let value: Value = serde_json::from_slice(&body).map_err(|error| {
            FetchError::FetcherError(format!("Invalid Crossref response: {error}"))
        })?;
        let message = value
            .get("message")
            .ok_or_else(|| FetchError::FetcherError("Crossref response has no work".into()))?;
        let content = render_work(&doi, message);
        let (content, truncated) = truncate(
            content,
            options.max_body_size.unwrap_or(DEFAULT_MAX_BODY_SIZE),
        );
        let size = u64::try_from(content.len()).unwrap_or(u64::MAX);
        Ok(FetchResponse {
            url: request.url,
            status_code,
            content_type: Some("text/markdown".into()),
            format: Some("crossref_work".into()),
            content: Some(content),
            size: Some(size),
            truncated: Some(truncated),
            ..Default::default()
        })
    }
}

fn valid_doi(value: &str) -> bool {
    let Some((prefix, suffix)) = value.split_once('/') else {
        return false;
    };
    prefix.starts_with("10.")
        && prefix[3..].len() >= 4
        && prefix[3..]
            .chars()
            .all(|character| character.is_ascii_digit())
        && !suffix.is_empty()
        && !value.chars().any(char::is_whitespace)
}

fn first<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_array()?.first()?.as_str()
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)?
        .as_str()
        .filter(|value| !value.trim().is_empty())
}

fn render_work(doi: &str, work: &Value) -> String {
    let title = first(work, "title").unwrap_or("Untitled work");
    let mut out = format!("# {title}\n\n- **DOI:** {doi}\n");
    field(&mut out, "Type", text(work, "type"));
    field(&mut out, "Publisher", text(work, "publisher"));
    field(&mut out, "Container", first(work, "container-title"));
    field(&mut out, "Published", publication_date(work));
    field(&mut out, "Volume", text(work, "volume"));
    field(&mut out, "Issue", text(work, "issue"));
    field(&mut out, "Pages", text(work, "page"));
    field(&mut out, "License", license_url(work));
    let authors = authors(work);
    if !authors.is_empty() {
        out.push_str(&format!("- **Authors:** {}\n", authors.join(", ")));
    }
    if let Some(subjects) = work.get("subject").and_then(Value::as_array) {
        let subjects: Vec<&str> = subjects.iter().filter_map(Value::as_str).collect();
        if !subjects.is_empty() {
            out.push_str(&format!("- **Subjects:** {}\n", subjects.join(", ")));
        }
    }
    if let Some(abstract_html) = text(work, "abstract") {
        let abstract_markdown = html_to_markdown(abstract_html);
        if !abstract_markdown.trim().is_empty() {
            out.push_str(&format!("\n## Abstract\n\n{}\n", abstract_markdown.trim()));
        }
    }
    out
}

fn publication_date(work: &Value) -> Option<String> {
    for key in ["published-print", "published-online", "published", "issued"] {
        let Some(parts) = work
            .pointer(&format!("/{key}/date-parts/0"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        let numbers: Vec<u64> = parts.iter().filter_map(Value::as_u64).collect();
        if let Some(year) = numbers.first() {
            return Some(match numbers.as_slice() {
                [year, month, day, ..] => format!("{year:04}-{month:02}-{day:02}"),
                [year, month] => format!("{year:04}-{month:02}"),
                _ => year.to_string(),
            });
        }
    }
    None
}

fn authors(work: &Value) -> Vec<String> {
    work.get("author")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|author| {
            let given = text(author, "given").unwrap_or("");
            let family = text(author, "family").unwrap_or("");
            let name = format!("{given} {family}").trim().to_string();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

fn license_url(work: &Value) -> Option<&str> {
    work.get("license")
        .and_then(Value::as_array)?
        .first()?
        .get("URL")?
        .as_str()
}

fn field(out: &mut String, label: &str, value: Option<impl AsRef<str>>) {
    if let Some(value) = value {
        out.push_str(&format!("- **{label}:** {}\n", value.as_ref()));
    }
}

fn truncate(mut value: String, max: usize) -> (String, bool) {
    if value.len() <= max {
        return (value, false);
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    (value, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes_doi_resolver_urls() {
        assert_eq!(
            CrossrefFetcher::doi(&Url::parse("https://doi.org/10.1000/ABC%2Fdef").unwrap()),
            Some("10.1000/abc/def".into())
        );
        assert_eq!(
            CrossrefFetcher::doi(&Url::parse("http://dx.doi.org/10.12345/Test").unwrap()),
            Some("10.12345/test".into())
        );
    }

    #[test]
    fn rejects_non_resolvers_and_malformed_dois() {
        for input in [
            "https://example.com/10.1000/test",
            "https://doi.org/",
            "https://doi.org/11.1000/test",
            "https://doi.org/10.12/test",
            "https://doi.org/10.1000/",
        ] {
            assert!(CrossrefFetcher::doi(&Url::parse(input).unwrap()).is_none());
        }
    }

    #[test]
    fn renders_work_metadata_and_converts_abstract_html() {
        let work = serde_json::json!({
            "title":["Agent study"], "type":"journal-article", "publisher":"Example Press",
            "container-title":["Journal of Agents"], "published-print":{"date-parts":[[2025,3,7]]},
            "volume":"4", "issue":"2", "page":"10-20",
            "author":[{"given":"Ada","family":"Lovelace"},{"family":"Turing"}],
            "subject":["Computer Science"], "license":[{"URL":"https://license.example"}],
            "abstract":"<jats:p>We found <b>useful</b> results.</jats:p>"
        });
        let output = render_work("10.1000/test", &work);
        assert!(output.contains("# Agent study"));
        assert!(output.contains("- **Published:** 2025-03-07"));
        assert!(output.contains("- **Authors:** Ada Lovelace, Turing"));
        assert!(output.contains("- **Subjects:** Computer Science"));
        assert!(output.contains("## Abstract"));
        assert!(output.contains("We found **useful** results."));
    }

    #[test]
    fn supports_partial_dates_and_utf8_safe_truncation() {
        assert_eq!(
            publication_date(&serde_json::json!({"issued":{"date-parts":[[2024]]}})),
            Some("2024".into())
        );
        assert_eq!(
            publication_date(&serde_json::json!({"published-online":{"date-parts":[[2024,9]]}})),
            Some("2024-09".into())
        );
        assert_eq!(truncate("aéz".into(), 2), ("a".into(), true));
    }
}
