//! ArXiv paper fetcher
//!
//! Handles arxiv.org/abs/{id} and arxiv.org/pdf/{id} URLs, returning
//! structured paper metadata via the arXiv API.

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderValue, USER_AGENT};
use std::time::Duration;
use url::Url;

const API_TIMEOUT: Duration = Duration::from_secs(10);

/// ArXiv paper fetcher
///
/// Matches `arxiv.org/abs/{id}` and `arxiv.org/pdf/{id}`, returning
/// paper metadata via the arXiv API.
pub struct ArXivFetcher;

impl ArXivFetcher {
    pub fn new() -> Self {
        Self
    }

    /// Extract paper ID from an arXiv URL
    fn parse_url(url: &Url) -> Option<String> {
        let host = url.host_str()?;
        if host != "arxiv.org" && host != "www.arxiv.org" {
            return None;
        }

        let segments: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();

        // /abs/{id} or /pdf/{id}
        if segments.len() < 2 {
            return None;
        }

        match segments[0] {
            "abs" | "pdf" => {
                let id = segments[1..].join("/");
                // Strip .pdf suffix if present
                let id = id.strip_suffix(".pdf").unwrap_or(&id);
                if id.is_empty() {
                    None
                } else {
                    Some(id.to_string())
                }
            }
            _ => None,
        }
    }
}

impl Default for ArXivFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for ArXivFetcher {
    fn name(&self) -> &'static str {
        "arxiv"
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

        let paper_id = Self::parse_url(&url)
            .ok_or_else(|| FetchError::FetcherError("Not a valid arXiv URL".to_string()))?;

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

        // Fetch via arXiv API (returns Atom XML)
        let api_url = format!("http://export.arxiv.org/api/query?id_list={}", paper_id);

        let response = client
            .get(&api_url)
            .header(USER_AGENT, ua_header)
            .send()
            .await
            .map_err(FetchError::from_reqwest)?;

        if !response.status().is_success() {
            return Ok(FetchResponse {
                url: request.url.clone(),
                status_code: response.status().as_u16(),
                error: Some(format!("arXiv API error: HTTP {}", response.status())),
                ..Default::default()
            });
        }

        let xml = response
            .text()
            .await
            .map_err(|e| FetchError::RequestError(e.to_string()))?;

        let content = parse_arxiv_response(&xml, &paper_id);

        Ok(FetchResponse {
            url: request.url.clone(),
            status_code: 200,
            content_type: Some("text/markdown".to_string()),
            format: Some("arxiv_paper".to_string()),
            content: Some(content),
            ..Default::default()
        })
    }
}

/// Parse arXiv Atom XML response into markdown
/// Uses simple string extraction to avoid XML parser dependency
fn parse_arxiv_response(xml: &str, paper_id: &str) -> String {
    let mut out = String::new();

    // Extract title
    let title = extract_xml_tag(xml, "title")
        .and_then(|titles| titles.into_iter().nth(1)) // First title is feed title, second is paper
        .unwrap_or_else(|| format!("arXiv:{}", paper_id));
    let title = title.split_whitespace().collect::<Vec<_>>().join(" "); // Normalize whitespace

    out.push_str(&format!("# {}\n\n", title));

    // Authors
    let authors: Vec<String> = extract_xml_tag(xml, "name")
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .collect();
    if !authors.is_empty() {
        out.push_str(&format!("**Authors:** {}\n\n", authors.join(", ")));
    }

    // Metadata
    out.push_str("## Metadata\n\n");
    out.push_str(&format!("- **arXiv ID:** {}\n", paper_id));
    out.push_str(&format!(
        "- **Abstract URL:** https://arxiv.org/abs/{}\n",
        paper_id
    ));
    out.push_str(&format!(
        "- **PDF URL:** https://arxiv.org/pdf/{}\n",
        paper_id
    ));
    out.push_str(&format!(
        "- **HTML URL:** https://ar5iv.labs.arxiv.org/html/{}\n",
        paper_id
    ));

    // Categories
    if let Some(categories) = extract_xml_attr(xml, "category", "term") {
        if !categories.is_empty() {
            out.push_str(&format!("- **Categories:** {}\n", categories.join(", ")));
        }
    }

    // Published/updated dates
    if let Some(dates) = extract_xml_tag(xml, "published") {
        if let Some(date) = dates.first() {
            out.push_str(&format!("- **Published:** {}\n", date.trim()));
        }
    }
    if let Some(dates) = extract_xml_tag(xml, "updated") {
        if let Some(date) = dates.first() {
            out.push_str(&format!("- **Updated:** {}\n", date.trim()));
        }
    }

    // DOI
    if let Some(dois) = extract_xml_tag(xml, "arxiv:doi") {
        if let Some(doi) = dois.first() {
            out.push_str(&format!("- **DOI:** {}\n", doi.trim()));
        }
    }

    // Journal ref
    if let Some(refs) = extract_xml_tag(xml, "arxiv:journal_ref") {
        if let Some(journal_ref) = refs.first() {
            out.push_str(&format!("- **Journal:** {}\n", journal_ref.trim()));
        }
    }

    // Abstract (summary tag)
    if let Some(summaries) = extract_xml_tag(xml, "summary") {
        if let Some(abstract_text) = summaries.first() {
            let cleaned = abstract_text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!("\n## Abstract\n\n{}\n", cleaned));
        }
    }

    out
}

/// Extract text content from XML tags (simple approach, no XML parser)
fn extract_xml_tag(xml: &str, tag: &str) -> Option<Vec<String>> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut results = Vec::new();
    let mut search_from = 0;

    while let Some(start_pos) = xml[search_from..].find(&open) {
        let abs_start = search_from + start_pos;
        // Find the end of the opening tag (after >)
        let tag_content_start = xml[abs_start..].find('>')? + abs_start + 1;

        if let Some(end_pos) = xml[tag_content_start..].find(&close) {
            let content = &xml[tag_content_start..tag_content_start + end_pos];
            results.push(content.to_string());
            search_from = tag_content_start + end_pos + close.len();
        } else {
            break;
        }
    }

    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}

/// Extract attribute values from self-closing XML tags
fn extract_xml_attr(xml: &str, tag: &str, attr: &str) -> Option<Vec<String>> {
    let pattern = format!("<{} ", tag);
    let attr_pattern = format!("{}=\"", attr);
    let mut results = Vec::new();
    let mut search_from = 0;

    while let Some(pos) = xml[search_from..].find(&pattern) {
        let abs_pos = search_from + pos;
        let tag_end = xml[abs_pos..]
            .find("/>")
            .or_else(|| xml[abs_pos..].find('>'));

        if let Some(end) = tag_end {
            let tag_content = &xml[abs_pos..abs_pos + end];
            if let Some(attr_pos) = tag_content.find(&attr_pattern) {
                let value_start = attr_pos + attr_pattern.len();
                if let Some(value_end) = tag_content[value_start..].find('"') {
                    results.push(tag_content[value_start..value_start + value_end].to_string());
                }
            }
            search_from = abs_pos + end;
        } else {
            break;
        }
    }

    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_abs_url() {
        let url = Url::parse("https://arxiv.org/abs/2301.07041").unwrap();
        assert_eq!(
            ArXivFetcher::parse_url(&url),
            Some("2301.07041".to_string())
        );
    }

    #[test]
    fn test_parse_pdf_url() {
        let url = Url::parse("https://arxiv.org/pdf/2301.07041").unwrap();
        assert_eq!(
            ArXivFetcher::parse_url(&url),
            Some("2301.07041".to_string())
        );
    }

    #[test]
    fn test_parse_pdf_url_with_extension() {
        let url = Url::parse("https://arxiv.org/pdf/2301.07041.pdf").unwrap();
        assert_eq!(
            ArXivFetcher::parse_url(&url),
            Some("2301.07041".to_string())
        );
    }

    #[test]
    fn test_parse_old_format() {
        let url = Url::parse("https://arxiv.org/abs/hep-th/9901001").unwrap();
        assert_eq!(
            ArXivFetcher::parse_url(&url),
            Some("hep-th/9901001".to_string())
        );
    }

    #[test]
    fn test_rejects_non_arxiv() {
        let url = Url::parse("https://example.org/abs/2301.07041").unwrap();
        assert_eq!(ArXivFetcher::parse_url(&url), None);
    }

    #[test]
    fn test_rejects_non_paper_paths() {
        let url = Url::parse("https://arxiv.org/list/cs.AI/recent").unwrap();
        assert_eq!(ArXivFetcher::parse_url(&url), None);
    }

    #[test]
    fn test_fetcher_matches() {
        let fetcher = ArXivFetcher::new();

        let url = Url::parse("https://arxiv.org/abs/2301.07041").unwrap();
        assert!(fetcher.matches(&url));

        let url = Url::parse("https://arxiv.org/pdf/2301.07041").unwrap();
        assert!(fetcher.matches(&url));

        let url = Url::parse("https://example.com/abs/123").unwrap();
        assert!(!fetcher.matches(&url));
    }

    #[test]
    fn test_extract_xml_tag() {
        let xml = "<entry><title>Test Paper</title><summary>Abstract text</summary></entry>";
        let titles = extract_xml_tag(xml, "title").unwrap();
        assert_eq!(titles, vec!["Test Paper"]);

        let summaries = extract_xml_tag(xml, "summary").unwrap();
        assert_eq!(summaries, vec!["Abstract text"]);
    }

    #[test]
    fn test_extract_xml_attr() {
        let xml = r#"<entry><category term="cs.AI"/><category term="cs.LG"/></entry>"#;
        let categories = extract_xml_attr(xml, "category", "term").unwrap();
        assert_eq!(categories, vec!["cs.AI", "cs.LG"]);
    }

    #[test]
    fn test_parse_arxiv_response() {
        let xml = r#"<?xml version="1.0"?>
<feed>
<title>ArXiv Query</title>
<entry>
<title>Attention Is All You Need</title>
<summary>We propose a new architecture...</summary>
<name>Ashish Vaswani</name>
<name>Noam Shazeer</name>
<category term="cs.CL"/>
<category term="cs.AI"/>
<published>2017-06-12T00:00:00Z</published>
</entry>
</feed>"#;

        let output = parse_arxiv_response(xml, "1706.03762");
        assert!(output.contains("# Attention Is All You Need"));
        assert!(output.contains("Ashish Vaswani"));
        assert!(output.contains("cs.CL"));
        assert!(output.contains("We propose a new architecture"));
        assert!(output.contains("1706.03762"));
    }
}
