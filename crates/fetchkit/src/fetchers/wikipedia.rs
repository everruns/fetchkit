//! Wikipedia article fetcher
//!
//! Handles wikipedia.org/wiki/{title} URLs, returning clean article content
//! via the MediaWiki REST API.

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderValue, ACCEPT, USER_AGENT};
use serde::Deserialize;
use std::time::Duration;
use url::Url;

const API_TIMEOUT: Duration = Duration::from_secs(10);

/// Wikipedia fetcher
///
/// Matches `https://{lang}.wikipedia.org/wiki/{title}` and returns
/// article summary and content via the MediaWiki REST API.
pub struct WikipediaFetcher;

impl WikipediaFetcher {
    pub fn new() -> Self {
        Self
    }

    /// Extract language and title from a Wikipedia URL
    fn parse_url(url: &Url) -> Option<(String, String)> {
        let host = url.host_str()?;

        // Must be {lang}.wikipedia.org
        let lang = host.strip_suffix(".wikipedia.org")?;
        if lang.is_empty() || lang.contains('.') {
            return None;
        }

        let segments: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();

        // Must be /wiki/{title}
        if segments.len() < 2 || segments[0] != "wiki" {
            return None;
        }

        let title = segments[1..].join("/");
        if title.is_empty() {
            return None;
        }

        Some((lang.to_string(), title))
    }
}

impl Default for WikipediaFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct WikiSummary {
    title: String,
    extract: Option<String>,
    description: Option<String>,
    content_urls: Option<ContentUrls>,
}

#[derive(Debug, Deserialize)]
struct ContentUrls {
    desktop: Option<DesktopUrl>,
}

#[derive(Debug, Deserialize)]
struct DesktopUrl {
    page: Option<String>,
}

#[async_trait]
impl Fetcher for WikipediaFetcher {
    fn name(&self) -> &'static str {
        "wikipedia"
    }

    fn matches(&self, url: &Url) -> bool {
        Self::parse_url(url).is_some()
    }

    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        let url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;

        let (lang, title) = Self::parse_url(&url)
            .ok_or_else(|| FetchError::FetcherError("Not a valid Wikipedia URL".to_string()))?;

        let user_agent = options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT);
        let mut client_builder = reqwest::Client::builder()
            .connect_timeout(API_TIMEOUT)
            .timeout(API_TIMEOUT)
            .redirect(reqwest::redirect::Policy::limited(3));

        if !options.respect_proxy_env {
            client_builder = client_builder.no_proxy();
        }

        let client = client_builder
            .build()
            .map_err(FetchError::ClientBuildError)?;

        let ua_header = HeaderValue::from_str(user_agent)
            .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT));

        // Fetch summary via REST API
        let summary_url = format!(
            "https://{}.wikipedia.org/api/rest_v1/page/summary/{}",
            lang, title
        );

        let summary_resp = client
            .get(&summary_url)
            .header(USER_AGENT, ua_header.clone())
            .header(ACCEPT, HeaderValue::from_static("application/json"))
            .send()
            .await
            .map_err(FetchError::from_reqwest)?;

        let status_code = summary_resp.status().as_u16();
        if !summary_resp.status().is_success() {
            let error_msg = if status_code == 404 {
                format!("Article '{}' not found on {}.wikipedia.org", title, lang)
            } else {
                format!("Wikipedia API error: HTTP {}", status_code)
            };
            return Ok(FetchResponse {
                url: request.url.clone(),
                status_code,
                error: Some(error_msg),
                ..Default::default()
            });
        }

        let summary: WikiSummary = summary_resp.json().await.map_err(|e| {
            FetchError::FetcherError(format!("Failed to parse Wikipedia data: {}", e))
        })?;

        // Also fetch full HTML content and convert to markdown
        let html_url = format!(
            "https://{}.wikipedia.org/api/rest_v1/page/html/{}",
            lang, title
        );

        let full_content = match client
            .get(&html_url)
            .header(USER_AGENT, ua_header)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                let html = resp.text().await.ok();
                html.map(|h| crate::convert::html_to_markdown(&h))
            }
            _ => None,
        };

        let content = format_wikipedia_response(&summary, full_content.as_deref(), &lang);

        Ok(FetchResponse {
            url: request.url.clone(),
            status_code: 200,
            content_type: Some("text/markdown".to_string()),
            format: Some("wikipedia".to_string()),
            content: Some(content),
            ..Default::default()
        })
    }
}

fn format_wikipedia_response(
    summary: &WikiSummary,
    full_content: Option<&str>,
    lang: &str,
) -> String {
    let mut out = String::new();

    out.push_str(&format!("# {}\n\n", summary.title));

    if let Some(desc) = &summary.description {
        out.push_str(&format!("*{}*\n\n", desc));
    }

    out.push_str(&format!("- **Language:** {}\n", lang));

    if let Some(urls) = &summary.content_urls {
        if let Some(desktop) = &urls.desktop {
            if let Some(page) = &desktop.page {
                out.push_str(&format!("- **URL:** {}\n", page));
            }
        }
    }

    // Use full content if available, otherwise use summary extract
    if let Some(content) = full_content {
        out.push_str(&format!("\n---\n\n{}", content));
    } else if let Some(extract) = &summary.extract {
        out.push_str(&format!("\n## Summary\n\n{}\n", extract));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wikipedia_url() {
        let url = Url::parse("https://en.wikipedia.org/wiki/Rust_(programming_language)").unwrap();
        assert_eq!(
            WikipediaFetcher::parse_url(&url),
            Some(("en".to_string(), "Rust_(programming_language)".to_string()))
        );
    }

    #[test]
    fn test_parse_other_language() {
        let url = Url::parse("https://de.wikipedia.org/wiki/Berlin").unwrap();
        assert_eq!(
            WikipediaFetcher::parse_url(&url),
            Some(("de".to_string(), "Berlin".to_string()))
        );
    }

    #[test]
    fn test_rejects_non_wiki_path() {
        let url = Url::parse("https://en.wikipedia.org/w/index.php?title=Rust").unwrap();
        assert_eq!(WikipediaFetcher::parse_url(&url), None);
    }

    #[test]
    fn test_rejects_non_wikipedia() {
        let url = Url::parse("https://example.org/wiki/Test").unwrap();
        assert_eq!(WikipediaFetcher::parse_url(&url), None);
    }

    #[test]
    fn test_fetcher_matches() {
        let fetcher = WikipediaFetcher::new();

        let url = Url::parse("https://en.wikipedia.org/wiki/Rust").unwrap();
        assert!(fetcher.matches(&url));

        let url = Url::parse("https://example.com/wiki/Rust").unwrap();
        assert!(!fetcher.matches(&url));
    }

    #[test]
    fn test_format_wikipedia_response() {
        let summary = WikiSummary {
            title: "Rust (programming language)".to_string(),
            extract: Some("Rust is a systems programming language.".to_string()),
            description: Some("Programming language".to_string()),
            content_urls: None,
        };

        let output = format_wikipedia_response(&summary, None, "en");

        assert!(output.contains("# Rust (programming language)"));
        assert!(output.contains("*Programming language*"));
        assert!(output.contains("Rust is a systems programming language."));
    }
}
