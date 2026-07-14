//! GitHub release fetcher.
//!
//! Converts a GitHub release page into compact Markdown using the GitHub API.

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::fetchers::default::{read_full_body, transport_request};
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use serde::Deserialize;
use std::time::Duration;
use url::Url;

const API_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_API_HOST: &str = "api.github.com";
const GITHUB_API_PORT: u16 = 443;
const DEFAULT_MAX_BODY_SIZE: usize = 5 * 1024 * 1024;

/// Fetches `github.com/{owner}/{repo}/releases/tag/{tag}` pages.
pub struct GitHubReleaseFetcher;

impl GitHubReleaseFetcher {
    pub fn new() -> Self {
        Self
    }

    fn parse_url(url: &Url) -> Option<(String, String, String)> {
        if url.host_str() != Some("github.com") {
            return None;
        }
        let segments: Vec<&str> = url.path_segments()?.collect();
        if segments.len() != 5 || segments[2] != "releases" || segments[3] != "tag" {
            return None;
        }
        if segments[0].is_empty() || segments[1].is_empty() || segments[4].is_empty() {
            return None;
        }
        Some((segments[0].into(), segments[1].into(), segments[4].into()))
    }
}

impl Default for GitHubReleaseFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct Release {
    name: Option<String>,
    tag_name: String,
    body: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
    author: User,
    created_at: String,
    published_at: Option<String>,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct User {
    login: String,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    download_count: u64,
    content_type: String,
}

#[async_trait]
impl Fetcher for GitHubReleaseFetcher {
    fn name(&self) -> &'static str {
        "github_release"
    }

    fn matches(&self, url: &Url) -> bool {
        Self::parse_url(url).is_some()
    }

    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        let request = request.normalized_for_fetch()?;
        let url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;
        let (owner, repo, tag) = Self::parse_url(&url)
            .ok_or_else(|| FetchError::FetcherError("Not a valid GitHub release URL".into()))?;
        let api_url = Url::parse(&format!(
            "https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}"
        ))
        .map_err(|_| FetchError::InvalidUrlScheme)?;

        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT))
                .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT)),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        let response = transport_request(
            api_url,
            reqwest::Method::GET,
            headers,
            options,
            API_TIMEOUT,
            GITHUB_API_HOST,
            GITHUB_API_PORT,
        )
        .await?;
        let status_code = response.status;
        if !(200..300).contains(&status_code) {
            let error = match status_code {
                404 => format!("{owner}/{repo} release {tag} not found"),
                403 => "GitHub API rate limit exceeded".into(),
                _ => format!("GitHub API error: HTTP {status_code}"),
            };
            return Ok(FetchResponse {
                url: request.url,
                status_code,
                error: Some(error),
                ..Default::default()
            });
        }

        let body = read_full_body(response, options).await?;
        let release: Release = serde_json::from_slice(&body).map_err(|error| {
            FetchError::FetcherError(format!("Failed to parse release data: {error}"))
        })?;
        let max = options.max_body_size.unwrap_or(DEFAULT_MAX_BODY_SIZE);
        let (content, truncated) = truncate(format_release(&release), max);
        let size = u64::try_from(content.len()).unwrap_or(u64::MAX);
        Ok(FetchResponse {
            url: request.url,
            status_code,
            content_type: Some("text/markdown".into()),
            format: Some("github_release".into()),
            content: Some(content),
            size: Some(size),
            truncated: Some(truncated),
            ..Default::default()
        })
    }
}

fn format_release(release: &Release) -> String {
    let title = release.name.as_deref().unwrap_or(&release.tag_name);
    let mut out = format!(
        "# {title}\n\n## Metadata\n\n- **Tag:** {}\n- **Author:** {}\n- **Created:** {}\n",
        release.tag_name, release.author.login, release.created_at
    );
    if let Some(published) = &release.published_at {
        out.push_str(&format!("- **Published:** {published}\n"));
    }
    out.push_str(&format!(
        "- **Draft:** {}\n- **Prerelease:** {}\n- **URL:** {}\n",
        release.draft, release.prerelease, release.html_url
    ));
    if let Some(body) = &release.body {
        if !body.is_empty() {
            out.push_str(&format!("\n## Release notes\n\n{body}\n"));
        }
    }
    if !release.assets.is_empty() {
        out.push_str("\n## Assets\n\n");
        for asset in &release.assets {
            out.push_str(&format!(
                "- [{}]({}) — {} bytes, {}, {} downloads\n",
                asset.name,
                asset.browser_download_url,
                asset.size,
                asset.content_type,
                asset.download_count
            ));
        }
    }
    out
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
    fn parses_release_url_and_preserves_encoded_tag() {
        let url = Url::parse("https://github.com/owner/repo/releases/tag/v1.0%2Bmeta").unwrap();
        assert_eq!(
            GitHubReleaseFetcher::parse_url(&url),
            Some(("owner".into(), "repo".into(), "v1.0%2Bmeta".into()))
        );
    }

    #[test]
    fn rejects_latest_and_extra_segments() {
        for value in [
            "https://github.com/owner/repo/releases/latest",
            "https://github.com/owner/repo/releases/tag/v1/extra",
            "https://gitlab.com/owner/repo/releases/tag/v1",
        ] {
            assert!(!GitHubReleaseFetcher::new().matches(&Url::parse(value).unwrap()));
        }
    }

    #[test]
    fn formats_metadata_notes_and_assets() {
        let release = Release {
            name: Some("Fetchkit 1.0".into()),
            tag_name: "v1.0.0".into(),
            body: Some("Notable changes.".into()),
            html_url: "https://github.com/everruns/fetchkit/releases/tag/v1.0.0".into(),
            draft: false,
            prerelease: true,
            author: User {
                login: "octocat".into(),
            },
            created_at: "2026-01-01T00:00:00Z".into(),
            published_at: Some("2026-01-02T00:00:00Z".into()),
            assets: vec![Asset {
                name: "fetchkit.tar.gz".into(),
                browser_download_url: "https://example.test/fetchkit.tar.gz".into(),
                size: 42,
                download_count: 7,
                content_type: "application/gzip".into(),
            }],
        };
        let output = format_release(&release);
        assert!(output.contains("# Fetchkit 1.0"));
        assert!(output.contains("- **Prerelease:** true"));
        assert!(output.contains("Notable changes."));
        assert!(output.contains("42 bytes, application/gzip, 7 downloads"));
    }

    #[test]
    fn truncates_on_utf8_boundary() {
        assert_eq!(truncate("aéz".into(), 2), ("a".into(), true));
        assert_eq!(truncate("okay".into(), 4), ("okay".into(), false));
    }
}
