//! GitHub commit and comparison fetcher.

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

const API_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_BODY_SIZE: usize = 5 * 1024 * 1024;
const API_VERSION: &str = "2022-11-28";
const MAX_PATCH_CHARS: usize = 2_000;

enum Target {
    Commit {
        owner: String,
        repo: String,
        reference: String,
    },
    Compare {
        owner: String,
        repo: String,
        base: String,
        head: String,
    },
}

pub struct GitHubCommitFetcher;

#[derive(Debug, Deserialize)]
struct CommitResponse {
    sha: String,
    html_url: String,
    commit: CommitDetail,
    stats: Option<Stats>,
    #[serde(default)]
    files: Vec<FileChange>,
}

#[derive(Debug, Deserialize)]
struct CommitDetail {
    message: String,
    author: Signature,
    committer: Signature,
    verification: Verification,
}

#[derive(Debug, Deserialize)]
struct Signature {
    name: String,
    email: String,
    date: String,
}

#[derive(Debug, Deserialize)]
struct Verification {
    verified: bool,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct Stats {
    total: u64,
    additions: u64,
    deletions: u64,
}

#[derive(Debug, Deserialize)]
struct FileChange {
    filename: String,
    status: String,
    additions: u64,
    deletions: u64,
    changes: u64,
    patch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CompareResponse {
    html_url: String,
    status: String,
    ahead_by: u64,
    behind_by: u64,
    total_commits: u64,
    merge_base_commit: CommitResponse,
    commits: Vec<CommitResponse>,
    #[serde(default)]
    files: Vec<FileChange>,
}

impl GitHubCommitFetcher {
    pub fn new() -> Self {
        Self
    }

    fn parse_url(url: &Url) -> Option<Target> {
        if url.host_str()? != "github.com" {
            return None;
        }
        let segments: Vec<&str> = url.path_segments()?.collect();
        match segments.as_slice() {
            [owner, repo, "commit", reference]
                if valid_name(owner) && valid_name(repo) && valid_ref(reference) =>
            {
                Some(Target::Commit {
                    owner: decode(owner),
                    repo: decode(repo.trim_end_matches(".git")),
                    reference: decode(reference),
                })
            }
            [owner, repo, "compare", comparison] if valid_name(owner) && valid_name(repo) => {
                let comparison = decode(comparison);
                let (base, head) = comparison.split_once("...")?;
                if !valid_ref(base) || !valid_ref(head) {
                    return None;
                }
                Some(Target::Compare {
                    owner: decode(owner),
                    repo: decode(repo.trim_end_matches(".git")),
                    base: base.into(),
                    head: head.into(),
                })
            }
            _ => None,
        }
    }
}

impl Default for GitHubCommitFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for GitHubCommitFetcher {
    fn name(&self) -> &'static str {
        "github_commit"
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
        let page_url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;
        let target = Self::parse_url(&page_url)
            .ok_or_else(|| FetchError::FetcherError("Not a GitHub commit or compare URL".into()))?;
        let (owner, repo, endpoint, label, format) = match &target {
            Target::Commit {
                owner,
                repo,
                reference,
            } => (owner, repo, "commits", reference.clone(), "github_commit"),
            Target::Compare {
                owner,
                repo,
                base,
                head,
            } => (
                owner,
                repo,
                "compare",
                format!("{base}...{head}"),
                "github_compare",
            ),
        };
        let api_url = api_url(owner, repo, endpoint, &label)?;
        let response = transport_request(
            api_url,
            reqwest::Method::GET,
            api_headers(options),
            options,
            API_TIMEOUT,
            "api.github.com",
            443,
        )
        .await?;
        let status_code = response.status;
        if !(200..300).contains(&status_code) {
            return Ok(api_error(request.url, status_code, &label));
        }
        let body = read_full_body(response, options).await?;
        let content = match target {
            Target::Commit { owner, repo, .. } => {
                let value: CommitResponse = serde_json::from_slice(&body).map_err(|error| {
                    FetchError::FetcherError(format!("Failed to parse GitHub commit: {error}"))
                })?;
                format_commit(&owner, &repo, &value)
            }
            Target::Compare {
                owner,
                repo,
                base,
                head,
            } => {
                let value: CompareResponse = serde_json::from_slice(&body).map_err(|error| {
                    FetchError::FetcherError(format!("Failed to parse GitHub comparison: {error}"))
                })?;
                format_compare(&owner, &repo, &base, &head, &value)
            }
        };
        let (content, truncated) = truncate(
            content,
            options.max_body_size.unwrap_or(DEFAULT_MAX_BODY_SIZE),
        );
        let size = u64::try_from(content.len()).unwrap_or(u64::MAX);
        Ok(FetchResponse {
            url: request.url,
            status_code,
            content_type: Some("text/markdown".into()),
            format: Some(format.into()),
            content: Some(content),
            size: Some(size),
            truncated: Some(truncated),
            ..Default::default()
        })
    }
}

fn api_url(owner: &str, repo: &str, endpoint: &str, value: &str) -> Result<Url, FetchError> {
    let mut url =
        Url::parse("https://api.github.com/repos/").map_err(|_| FetchError::InvalidUrlScheme)?;
    url.path_segments_mut()
        .map_err(|_| FetchError::InvalidUrlScheme)?
        .push(owner)
        .push(repo)
        .push(endpoint)
        .push(value);
    Ok(url)
}

fn api_headers(options: &FetchOptions) -> HeaderMap {
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
    headers.insert(
        "x-github-api-version",
        HeaderValue::from_static(API_VERSION),
    );
    headers
}

fn api_error(url: String, status_code: u16, label: &str) -> FetchResponse {
    FetchResponse {
        url,
        status_code,
        error: Some(match status_code {
            404 => format!("GitHub commit or comparison not found: {label}"),
            403 | 429 => "GitHub API rate limit exceeded or access denied".into(),
            422 => format!("GitHub could not compare the requested references: {label}"),
            _ => format!("GitHub API error: HTTP {status_code}"),
        }),
        ..Default::default()
    }
}

fn format_commit(owner: &str, repo: &str, value: &CommitResponse) -> String {
    let title = value.commit.message.lines().next().unwrap_or("Commit");
    let mut out = format!("# {title}\n\n- **Repository:** {owner}/{repo}\n- **SHA:** `{}`\n- **Author:** {} <{}>\n- **Authored:** {}\n- **Committer:** {} <{}>\n- **Committed:** {}\n- **Verified:** {} ({})\n- **URL:** {}\n", value.sha, value.commit.author.name, value.commit.author.email, value.commit.author.date, value.commit.committer.name, value.commit.committer.email, value.commit.committer.date, value.commit.verification.verified, value.commit.verification.reason, value.html_url);
    if value.commit.message.lines().count() > 1 {
        out.push_str(&format!("\n## Message\n\n{}\n", value.commit.message));
    }
    append_stats_files(&mut out, value.stats.as_ref(), &value.files);
    out
}

fn format_compare(
    owner: &str,
    repo: &str,
    base: &str,
    head: &str,
    value: &CompareResponse,
) -> String {
    let mut out = format!("# Compare {base}...{head}\n\n- **Repository:** {owner}/{repo}\n- **Status:** {}\n- **Ahead:** {}\n- **Behind:** {}\n- **Commits:** {}\n- **Merge base:** `{}`\n- **URL:** {}\n", value.status, value.ahead_by, value.behind_by, value.total_commits, value.merge_base_commit.sha, value.html_url);
    if !value.commits.is_empty() {
        out.push_str("\n## Commits\n");
        for commit in &value.commits {
            let title = commit.commit.message.lines().next().unwrap_or("Commit");
            out.push_str(&format!(
                "\n- `{}` {} — {} ({})",
                short_sha(&commit.sha),
                title,
                commit.commit.author.name,
                commit.commit.author.date
            ));
        }
        out.push('\n');
    }
    append_stats_files(&mut out, None, &value.files);
    out
}

fn append_stats_files(out: &mut String, stats: Option<&Stats>, files: &[FileChange]) {
    if let Some(stats) = stats {
        out.push_str(&format!(
            "\n## Changes\n\n- **Total:** {}\n- **Additions:** +{}\n- **Deletions:** -{}\n",
            stats.total, stats.additions, stats.deletions
        ));
    }
    if files.is_empty() {
        return;
    }
    if stats.is_none() {
        let additions: u64 = files.iter().map(|file| file.additions).sum();
        let deletions: u64 = files.iter().map(|file| file.deletions).sum();
        out.push_str(&format!("\n## Changes\n\n- **Files:** {}\n- **Additions:** +{additions}\n- **Deletions:** -{deletions}\n", files.len()));
    }
    out.push_str("\n## Files\n");
    for file in files {
        out.push_str(&format!(
            "\n### {} ({})\n\n+{} -{} ({} changes)\n",
            file.filename, file.status, file.additions, file.deletions, file.changes
        ));
        if let Some(patch) = &file.patch {
            let (patch, clipped) = truncate_chars(patch, MAX_PATCH_CHARS);
            out.push_str(&format!("\n```diff\n{patch}"));
            if clipped {
                out.push_str("\n... patch truncated");
            }
            out.push_str("\n```\n");
        }
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".."
}
fn valid_ref(value: &str) -> bool {
    !value.is_empty() && !value.chars().any(char::is_whitespace)
}
fn decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .into_owned()
}
fn short_sha(value: &str) -> &str {
    value.get(..7).unwrap_or(value)
}
fn truncate_chars(value: &str, max: usize) -> (&str, bool) {
    if value.chars().count() <= max {
        return (value, false);
    }
    let end = value
        .char_indices()
        .nth(max)
        .map_or(value.len(), |(index, _)| index);
    (&value[..end], true)
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
    fn parses_commit_and_compare_urls() {
        assert!(
            matches!(GitHubCommitFetcher::parse_url(&Url::parse("https://github.com/o/r/commit/abc123").unwrap()), Some(Target::Commit { reference, .. }) if reference == "abc123")
        );
        assert!(
            matches!(GitHubCommitFetcher::parse_url(&Url::parse("https://github.com/o/r/compare/main...feature%2Fx").unwrap()), Some(Target::Compare { base, head, .. }) if base == "main" && head == "feature/x")
        );
    }

    #[test]
    fn rejects_malformed_and_non_github_urls() {
        for value in [
            "https://github.com/o/r/commit/",
            "https://github.com/o/r/compare/main..head",
            "https://github.com/o/r/compare/...head",
            "https://gitlab.com/o/r/commit/abc",
        ] {
            assert!(
                !GitHubCommitFetcher::new().matches(&Url::parse(value).unwrap()),
                "{value}"
            );
        }
    }

    #[test]
    fn formats_commit_metadata_files_and_bounded_patch() {
        let value: CommitResponse = serde_json::from_value(serde_json::json!({
            "sha":"abcdef123", "html_url":"https://github.com/o/r/commit/abcdef123",
            "commit":{"message":"Fix parser\n\nHandle edge case.","author":{"name":"Ada","email":"ada@example.com","date":"2026-01-01"},"committer":{"name":"Ada","email":"ada@example.com","date":"2026-01-02"},"verification":{"verified":true,"reason":"valid"}},
            "stats":{"total":3,"additions":2,"deletions":1},
            "files":[{"filename":"src/lib.rs","status":"modified","additions":2,"deletions":1,"changes":3,"patch":"@@ -1 +1 @@\n-old\n+new"}]
        })).unwrap();
        let output = format_commit("o", "r", &value);
        assert!(output.contains("# Fix parser"));
        assert!(output.contains("**Verified:** true (valid)"));
        assert!(output.contains("### src/lib.rs (modified)"));
        assert!(output.contains("```diff"));
    }

    #[test]
    fn formats_comparison_and_handles_unicode_limits() {
        let value: CompareResponse = serde_json::from_value(serde_json::json!({
            "html_url":"url","status":"ahead","ahead_by":1,"behind_by":0,"total_commits":1,
            "merge_base_commit":{"sha":"111111111","html_url":"url","commit":{"message":"Base","author":{"name":"A","email":"a@e","date":"d"},"committer":{"name":"A","email":"a@e","date":"d"},"verification":{"verified":false,"reason":"unsigned"}},"stats":null,"files":[]},
            "commits":[{"sha":"abcdefghi","html_url":"url","commit":{"message":"Feature","author":{"name":"Ada","email":"a@e","date":"today"},"committer":{"name":"Ada","email":"a@e","date":"today"},"verification":{"verified":false,"reason":"unsigned"}},"stats":null,"files":[]}],
            "files":[]
        })).unwrap();
        let output = format_compare("o", "r", "main", "head", &value);
        assert!(output.contains("# Compare main...head"));
        assert!(output.contains("`abcdefg` Feature — Ada (today)"));
        assert_eq!(truncate_chars("aéz", 2), ("aé", true));
        assert_eq!(truncate("aéz".into(), 2), ("a".into(), true));
    }
}
