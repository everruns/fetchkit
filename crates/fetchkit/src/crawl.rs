//! Bounded same-origin crawl discovery for agent workflows.
//!
//! DECISION: crawl is deliberately shallow and same-origin. Agents need a small
//! discovery map, not an unbounded spider that surprises operators.

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::fetchers::FetcherRegistry;
use crate::types::{CrawlPage, CrawlResult, FetchRequest, FetchResponse};
use std::collections::HashSet;
use url::Url;

pub(crate) const DEFAULT_CRAWL_MAX_PAGES: usize = 5;
pub(crate) const MAX_CRAWL_MAX_PAGES: usize = 20;

pub(crate) async fn crawl_fetch_with_options(
    mut request: FetchRequest,
    options: FetchOptions,
) -> Result<FetchResponse, FetchError> {
    request.normalize_url_for_fetch()?;
    let max_pages = request
        .max_pages
        .unwrap_or(DEFAULT_CRAWL_MAX_PAGES)
        .clamp(1, MAX_CRAWL_MAX_PAGES);

    let registry = FetcherRegistry::with_defaults();
    let mut seed_request = request.clone();
    seed_request.crawl = None;
    seed_request.max_pages = None;
    if seed_request.as_markdown.is_none() {
        seed_request.as_markdown = Some(true);
    }
    if seed_request.content_focus.is_none() {
        seed_request.content_focus = Some("agent".to_string());
    }

    let mut seed_response = registry.fetch(seed_request, options.clone()).await?;
    let discovery_base_url =
        Url::parse(&seed_response.url).map_err(|_| FetchError::InvalidUrlScheme)?;
    let mut pages = vec![page_from_response(&seed_response)];
    let mut seen = HashSet::from([canonical_url_key(&seed_response.url)]);
    let mut candidates = discover_same_origin_links(&seed_response, &discovery_base_url);
    candidates.retain(|candidate| seen.insert(canonical_url_key(candidate.as_str())));

    let truncated = candidates.len() + pages.len() > max_pages;
    for url in candidates.into_iter().take(max_pages.saturating_sub(1)) {
        let mut page_request = FetchRequest::new(url.as_str()).as_markdown();
        page_request.content_focus = Some("agent".to_string());

        match registry.fetch(page_request, options.clone()).await {
            Ok(response) => pages.push(page_from_response(&response)),
            Err(err) => pages.push(CrawlPage {
                url: url.to_string(),
                error: Some(err.to_string()),
                ..Default::default()
            }),
        }
    }

    seed_response.crawl = Some(CrawlResult {
        seed_url: request.url,
        max_pages,
        pages,
        truncated: if truncated { Some(true) } else { None },
    });
    Ok(seed_response)
}

fn page_from_response(response: &FetchResponse) -> CrawlPage {
    let metadata = response.metadata.as_ref();
    CrawlPage {
        url: response.url.clone(),
        status_code: Some(response.status_code),
        title: metadata.and_then(|meta| meta.title.clone()),
        description: metadata.and_then(|meta| meta.description.clone()),
        content_type: response.content_type.clone(),
        word_count: response.word_count,
        quality_score: response.quality.as_ref().map(|quality| quality.score),
        error: response.error.clone(),
    }
}

fn discover_same_origin_links(response: &FetchResponse, seed_url: &Url) -> Vec<Url> {
    let Some(metadata) = response.metadata.as_ref() else {
        return Vec::new();
    };

    let mut urls = Vec::new();
    for link in &metadata.links {
        let Ok(url) = seed_url.join(&link.href) else {
            continue;
        };
        if is_same_origin(seed_url, &url) && is_fetchable_page_url(&url) {
            urls.push(url);
        }
    }
    urls
}

fn is_same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && normalized_host(left) == normalized_host(right)
        && left.port_or_known_default() == right.port_or_known_default()
}

fn normalized_host(url: &Url) -> Option<String> {
    url.host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
}

fn is_fetchable_page_url(url: &Url) -> bool {
    if url.scheme() != "http" && url.scheme() != "https" {
        return false;
    }

    let path = url.path().to_ascii_lowercase();
    ![
        ".avif", ".css", ".gif", ".ico", ".jpeg", ".jpg", ".js", ".pdf", ".png", ".svg", ".webp",
        ".zip",
    ]
    .iter()
    .any(|suffix| path.ends_with(suffix))
}

fn canonical_url_key(raw_url: &str) -> String {
    let Ok(mut url) = Url::parse(raw_url) else {
        return raw_url.to_string();
    };
    url.set_fragment(None);
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns::DnsPolicy;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_crawl_discovers_same_origin_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/html")
                    .set_body_string(
                        r#"<html><head><title>Home</title></head><body>
                <a href="/docs">Docs</a>
                <a href="https://outside.example/page">Outside</a>
                <a href="/image.png">Image</a>
                </body></html>"#,
                    ),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/docs"))
            .respond_with(ResponseTemplate::new(200).insert_header("content-type", "text/html").set_body_string(
                r#"<html><head><title>Docs</title></head><body><p>Useful documentation content for agents to inspect and summarize.</p></body></html>"#,
            ))
            .mount(&server)
            .await;

        let options = FetchOptions {
            enable_markdown: true,
            dns_policy: DnsPolicy::allow_all(),
            ..Default::default()
        };
        let response = crawl_fetch_with_options(
            FetchRequest::new(server.uri()).crawl(true).max_pages(5),
            options,
        )
        .await
        .unwrap();

        let crawl = response.crawl.unwrap();
        assert_eq!(crawl.pages.len(), 2);
        assert!(crawl
            .pages
            .iter()
            .any(|page| page.url.ends_with("/docs") && page.title.as_deref() == Some("Docs")));
        assert_eq!(crawl.truncated, None);
    }

    #[test]
    fn test_is_fetchable_page_url_skips_assets() {
        assert!(is_fetchable_page_url(
            &Url::parse("https://example.com/docs").unwrap()
        ));
        assert!(!is_fetchable_page_url(
            &Url::parse("https://example.com/app.js").unwrap()
        ));
    }
}
