//! GitHub repository fetcher
//!
//! Handles GitHub repository root URLs, returning repo metadata and README content.

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

/// First-byte timeout for API requests
const API_TIMEOUT: Duration = Duration::from_secs(10);

/// GitHub repository fetcher
///
/// Matches GitHub repository root URLs (`https://github.com/{owner}/{repo}`)
/// and returns repository metadata along with README content.
pub struct GitHubRepoFetcher;

impl GitHubRepoFetcher {
    /// Create a new GitHub repo fetcher
    pub fn new() -> Self {
        Self
    }

    /// Extract owner and repo from a GitHub URL
    fn parse_github_url(url: &Url) -> Option<(String, String)> {
        // Must be github.com
        if url.host_str() != Some("github.com") {
            return None;
        }

        // Get path segments
        let segments: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();

        // Must have exactly 2 segments (owner/repo)
        // Ignore URLs like /owner/repo/issues, /owner/repo/blob/main/file.rs
        if segments.len() != 2 {
            return None;
        }

        let owner = segments[0];
        let repo = segments[1];

        // Basic validation
        if owner.is_empty() || repo.is_empty() {
            return None;
        }

        // Ignore special GitHub paths
        let reserved = [
            "settings",
            "explore",
            "trending",
            "collections",
            "events",
            "sponsors",
            "notifications",
            "marketplace",
            "pulls",
            "issues",
            "codespaces",
            "features",
            "enterprise",
            "organizations",
            "pricing",
            "about",
            "team",
            "security",
            "login",
            "join",
        ];
        if reserved.contains(&owner) {
            return None;
        }

        Some((owner.to_string(), repo.to_string()))
    }
}

impl Default for GitHubRepoFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// GitHub API repository response (partial)
#[derive(Debug, Deserialize)]
struct GitHubRepo {
    #[allow(dead_code)]
    name: String,
    full_name: String,
    description: Option<String>,
    html_url: String,
    homepage: Option<String>,
    stargazers_count: u64,
    forks_count: u64,
    open_issues_count: u64,
    language: Option<String>,
    license: Option<GitHubLicense>,
    default_branch: String,
    created_at: String,
    updated_at: String,
    pushed_at: String,
    topics: Option<Vec<String>>,
    archived: bool,
    fork: bool,
    owner: GitHubOwner,
}

#[derive(Debug, Deserialize)]
struct GitHubLicense {
    name: String,
    spdx_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubOwner {
    login: String,
    #[serde(rename = "type")]
    owner_type: String,
}

/// GitHub API README response
#[derive(Debug, Deserialize)]
struct GitHubReadme {
    content: String,
    encoding: String,
}

#[async_trait]
impl Fetcher for GitHubRepoFetcher {
    fn name(&self) -> &'static str {
        "github_repo"
    }

    fn matches(&self, url: &Url) -> bool {
        Self::parse_github_url(url).is_some()
    }

    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        let url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;

        let (owner, repo) = Self::parse_github_url(&url).ok_or_else(|| {
            FetchError::FetcherError("Not a valid GitHub repository URL".to_string())
        })?;

        // Build HTTP client
        let user_agent = options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT);
        let client = reqwest::Client::builder()
            .connect_timeout(API_TIMEOUT)
            .timeout(API_TIMEOUT)
            .build()
            .map_err(FetchError::ClientBuildError)?;

        // Fetch repository metadata
        let repo_url = format!("https://api.github.com/repos/{}/{}", owner, repo);
        let repo_response = client
            .get(&repo_url)
            .header(
                USER_AGENT,
                HeaderValue::from_str(user_agent)
                    .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT)),
            )
            .header(
                ACCEPT,
                HeaderValue::from_static("application/vnd.github+json"),
            )
            .send()
            .await
            .map_err(FetchError::from_reqwest)?;

        let status_code = repo_response.status().as_u16();

        // Handle non-success status
        if !repo_response.status().is_success() {
            let error_msg = if status_code == 404 {
                format!("Repository {}/{} not found", owner, repo)
            } else if status_code == 403 {
                "GitHub API rate limit exceeded".to_string()
            } else {
                format!("GitHub API error: HTTP {}", status_code)
            };
            return Ok(FetchResponse {
                url: request.url.clone(),
                status_code,
                error: Some(error_msg),
                ..Default::default()
            });
        }

        // Parse repository data
        let repo_data: GitHubRepo = repo_response
            .json()
            .await
            .map_err(|e| FetchError::FetcherError(format!("Failed to parse repo data: {}", e)))?;

        // Fetch README (optional - don't fail if missing)
        let readme_url = format!("https://api.github.com/repos/{}/{}/readme", owner, repo);
        let readme_content = match client
            .get(&readme_url)
            .header(
                USER_AGENT,
                HeaderValue::from_str(user_agent)
                    .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT)),
            )
            .header(
                ACCEPT,
                HeaderValue::from_static("application/vnd.github+json"),
            )
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<GitHubReadme>().await {
                    Ok(readme) if readme.encoding == "base64" => {
                        // Decode base64 content
                        decode_base64_content(&readme.content)
                    }
                    _ => None,
                }
            }
            _ => None,
        };

        // Format response as markdown
        let content = format_github_repo_response(&repo_data, readme_content.as_deref());

        Ok(FetchResponse {
            url: request.url.clone(),
            status_code: 200,
            content_type: Some("text/markdown".to_string()),
            format: Some("github_repo".to_string()),
            content: Some(content),
            ..Default::default()
        })
    }
}

/// Decode base64-encoded content (GitHub API returns README as base64)
fn decode_base64_content(encoded: &str) -> Option<String> {
    // GitHub base64 has newlines, remove them
    let cleaned: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();

    // Simple base64 decode
    let decoded = base64_decode(&cleaned)?;
    String::from_utf8(decoded).ok()
}

/// Basic base64 decoder (avoiding extra dependency)
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn decode_char(c: u8) -> Option<u8> {
        if c == b'=' {
            return Some(0);
        }
        ALPHABET.iter().position(|&x| x == c).map(|p| p as u8)
    }

    let bytes: Vec<u8> = input.bytes().collect();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }

    let mut result = Vec::with_capacity(bytes.len() * 3 / 4);

    for chunk in bytes.chunks(4) {
        let a = decode_char(chunk[0])?;
        let b = decode_char(chunk[1])?;
        let c = decode_char(chunk[2])?;
        let d = decode_char(chunk[3])?;

        result.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            result.push((b << 4) | (c >> 2));
        }
        if chunk[3] != b'=' {
            result.push((c << 6) | d);
        }
    }

    Some(result)
}

/// Format GitHub repo data as markdown
fn format_github_repo_response(repo: &GitHubRepo, readme: Option<&str>) -> String {
    let mut output = String::new();

    // Header
    output.push_str(&format!("# {}\n\n", repo.full_name));

    // Description
    if let Some(ref desc) = repo.description {
        output.push_str(&format!("{}\n\n", desc));
    }

    // Metadata section
    output.push_str("## Repository Info\n\n");

    // Stats
    output.push_str(&format!(
        "- **Stars:** {}\n- **Forks:** {}\n- **Open Issues:** {}\n",
        repo.stargazers_count, repo.forks_count, repo.open_issues_count
    ));

    // Language
    if let Some(ref lang) = repo.language {
        output.push_str(&format!("- **Language:** {}\n", lang));
    }

    // License
    if let Some(ref license) = repo.license {
        let license_str = license
            .spdx_id
            .as_ref()
            .unwrap_or(&license.name)
            .to_string();
        output.push_str(&format!("- **License:** {}\n", license_str));
    }

    // Topics
    if let Some(ref topics) = repo.topics {
        if !topics.is_empty() {
            output.push_str(&format!("- **Topics:** {}\n", topics.join(", ")));
        }
    }

    // Links
    output.push_str(&format!("- **URL:** {}\n", repo.html_url));
    if let Some(ref homepage) = repo.homepage {
        if !homepage.is_empty() {
            output.push_str(&format!("- **Homepage:** {}\n", homepage));
        }
    }

    // Branch info
    output.push_str(&format!("- **Default Branch:** {}\n", repo.default_branch));

    // Owner info
    output.push_str(&format!(
        "- **Owner:** {} ({})\n",
        repo.owner.login, repo.owner.owner_type
    ));

    // Status flags
    if repo.archived {
        output.push_str("- **Status:** Archived\n");
    }
    if repo.fork {
        output.push_str("- **Fork:** Yes\n");
    }

    // Dates
    output.push_str(&format!("- **Created:** {}\n", repo.created_at));
    output.push_str(&format!("- **Last Updated:** {}\n", repo.updated_at));
    output.push_str(&format!("- **Last Push:** {}\n", repo.pushed_at));

    // README content
    if let Some(readme_content) = readme {
        output.push_str("\n---\n\n## README\n\n");
        output.push_str(readme_content);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_url_valid() {
        let url = Url::parse("https://github.com/owner/repo").unwrap();
        assert_eq!(
            GitHubRepoFetcher::parse_github_url(&url),
            Some(("owner".to_string(), "repo".to_string()))
        );
    }

    #[test]
    fn test_parse_github_url_with_trailing_slash() {
        // URL parser normalizes away trailing slash for path
        let url = Url::parse("https://github.com/owner/repo/").unwrap();
        // This actually has 3 segments: ["owner", "repo", ""]
        assert_eq!(GitHubRepoFetcher::parse_github_url(&url), None);
    }

    #[test]
    fn test_parse_github_url_too_many_segments() {
        let url = Url::parse("https://github.com/owner/repo/issues").unwrap();
        assert_eq!(GitHubRepoFetcher::parse_github_url(&url), None);

        let url = Url::parse("https://github.com/owner/repo/blob/main/README.md").unwrap();
        assert_eq!(GitHubRepoFetcher::parse_github_url(&url), None);
    }

    #[test]
    fn test_parse_github_url_too_few_segments() {
        let url = Url::parse("https://github.com/owner").unwrap();
        assert_eq!(GitHubRepoFetcher::parse_github_url(&url), None);

        let url = Url::parse("https://github.com/").unwrap();
        assert_eq!(GitHubRepoFetcher::parse_github_url(&url), None);
    }

    #[test]
    fn test_parse_github_url_reserved_paths() {
        let url = Url::parse("https://github.com/settings/profile").unwrap();
        assert_eq!(GitHubRepoFetcher::parse_github_url(&url), None);

        let url = Url::parse("https://github.com/explore/topics").unwrap();
        assert_eq!(GitHubRepoFetcher::parse_github_url(&url), None);
    }

    #[test]
    fn test_parse_github_url_wrong_host() {
        let url = Url::parse("https://gitlab.com/owner/repo").unwrap();
        assert_eq!(GitHubRepoFetcher::parse_github_url(&url), None);
    }

    #[test]
    fn test_fetcher_matches() {
        let fetcher = GitHubRepoFetcher::new();

        let url = Url::parse("https://github.com/rust-lang/rust").unwrap();
        assert!(fetcher.matches(&url));

        let url = Url::parse("https://github.com/rust-lang/rust/issues").unwrap();
        assert!(!fetcher.matches(&url));

        let url = Url::parse("https://example.com/foo/bar").unwrap();
        assert!(!fetcher.matches(&url));
    }

    #[test]
    fn test_base64_decode() {
        // "Hello, World!" in base64
        assert_eq!(
            base64_decode("SGVsbG8sIFdvcmxkIQ=="),
            Some(b"Hello, World!".to_vec())
        );

        // Empty string
        assert_eq!(base64_decode(""), Some(vec![]));

        // Invalid length
        assert_eq!(base64_decode("abc"), None);
    }

    #[test]
    fn test_format_github_repo_response() {
        let repo = GitHubRepo {
            name: "test-repo".to_string(),
            full_name: "owner/test-repo".to_string(),
            description: Some("A test repository".to_string()),
            html_url: "https://github.com/owner/test-repo".to_string(),
            homepage: None,
            stargazers_count: 100,
            forks_count: 10,
            open_issues_count: 5,
            language: Some("Rust".to_string()),
            license: Some(GitHubLicense {
                name: "MIT License".to_string(),
                spdx_id: Some("MIT".to_string()),
            }),
            default_branch: "main".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-06-01T00:00:00Z".to_string(),
            pushed_at: "2024-06-01T00:00:00Z".to_string(),
            topics: Some(vec!["rust".to_string(), "cli".to_string()]),
            archived: false,
            fork: false,
            owner: GitHubOwner {
                login: "owner".to_string(),
                owner_type: "User".to_string(),
            },
        };

        let output = format_github_repo_response(&repo, Some("# Test\n\nThis is a test README."));

        assert!(output.contains("# owner/test-repo"));
        assert!(output.contains("A test repository"));
        assert!(output.contains("**Stars:** 100"));
        assert!(output.contains("**Language:** Rust"));
        assert!(output.contains("**License:** MIT"));
        assert!(output.contains("## README"));
        assert!(output.contains("This is a test README."));
    }
}
