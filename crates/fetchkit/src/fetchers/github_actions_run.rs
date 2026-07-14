//! GitHub Actions workflow run fetcher.

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

pub struct GitHubActionsRunFetcher;

#[derive(Debug, Deserialize)]
struct WorkflowRun {
    name: Option<String>,
    display_title: String,
    event: String,
    status: String,
    conclusion: Option<String>,
    workflow_id: u64,
    run_number: u64,
    run_attempt: u64,
    head_branch: Option<String>,
    head_sha: String,
    html_url: String,
    created_at: String,
    updated_at: String,
    run_started_at: Option<String>,
    actor: User,
}

#[derive(Debug, Deserialize)]
struct User {
    login: String,
}

#[derive(Debug, Deserialize)]
struct JobsResponse {
    total_count: u64,
    jobs: Vec<Job>,
}

#[derive(Debug, Deserialize)]
struct Job {
    name: String,
    status: String,
    conclusion: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
    html_url: Option<String>,
    steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
struct Step {
    name: String,
    status: String,
    conclusion: Option<String>,
    number: u64,
}

impl GitHubActionsRunFetcher {
    pub fn new() -> Self {
        Self
    }

    fn parse_url(url: &Url) -> Option<(String, String, u64)> {
        if url.host_str()? != "github.com" {
            return None;
        }
        let segments: Vec<&str> = url.path_segments()?.collect();
        match segments.as_slice() {
            [owner, repo, "actions", "runs", id] if valid_name(owner) && valid_name(repo) => {
                id.parse().ok().filter(|id| *id > 0).map(|id| {
                    (
                        percent_decode(owner),
                        percent_decode(repo.trim_end_matches(".git")),
                        id,
                    )
                })
            }
            _ => None,
        }
    }
}

impl Default for GitHubActionsRunFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for GitHubActionsRunFetcher {
    fn name(&self) -> &'static str {
        "github_actions_run"
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
        let (owner, repo, run_id) = Self::parse_url(&page_url)
            .ok_or_else(|| FetchError::FetcherError("Not a GitHub Actions run URL".into()))?;
        let run_url = api_url(&owner, &repo, run_id, false)?;
        let jobs_url = api_url(&owner, &repo, run_id, true)?;
        let headers = api_headers(options);
        let run_response = transport_request(
            run_url,
            reqwest::Method::GET,
            headers.clone(),
            options,
            API_TIMEOUT,
            "api.github.com",
            443,
        )
        .await?;
        let status_code = run_response.status;
        if !(200..300).contains(&status_code) {
            return Ok(api_error(request.url, status_code, run_id));
        }
        let run_body = read_full_body(run_response, options).await?;
        let run: WorkflowRun = serde_json::from_slice(&run_body).map_err(|error| {
            FetchError::FetcherError(format!("Failed to parse GitHub Actions run: {error}"))
        })?;
        let jobs_response = transport_request(
            jobs_url,
            reqwest::Method::GET,
            headers,
            options,
            API_TIMEOUT,
            "api.github.com",
            443,
        )
        .await?;
        let jobs_status = jobs_response.status;
        if !(200..300).contains(&jobs_status) {
            return Ok(api_error(request.url, jobs_status, run_id));
        }
        let jobs_body = read_full_body(jobs_response, options).await?;
        let jobs: JobsResponse = serde_json::from_slice(&jobs_body).map_err(|error| {
            FetchError::FetcherError(format!("Failed to parse GitHub Actions jobs: {error}"))
        })?;
        let content = format_run(&owner, &repo, run_id, &run, &jobs);
        let (content, truncated) = truncate(
            content,
            options.max_body_size.unwrap_or(DEFAULT_MAX_BODY_SIZE),
        );
        let size = u64::try_from(content.len()).unwrap_or(u64::MAX);
        Ok(FetchResponse {
            url: request.url,
            status_code,
            content_type: Some("text/markdown".into()),
            format: Some("github_actions_run".into()),
            content: Some(content),
            size: Some(size),
            truncated: Some(truncated),
            ..Default::default()
        })
    }
}

fn api_url(owner: &str, repo: &str, run_id: u64, jobs: bool) -> Result<Url, FetchError> {
    let mut url =
        Url::parse("https://api.github.com/repos/").map_err(|_| FetchError::InvalidUrlScheme)?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| FetchError::InvalidUrlScheme)?;
        segments
            .push(owner)
            .push(repo)
            .push("actions")
            .push("runs")
            .push(&run_id.to_string());
        if jobs {
            segments.push("jobs");
        }
    }
    if jobs {
        url.query_pairs_mut().append_pair("per_page", "100");
    }
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

fn api_error(url: String, status_code: u16, run_id: u64) -> FetchResponse {
    FetchResponse {
        url,
        status_code,
        error: Some(match status_code {
            404 => format!("GitHub Actions run {run_id} not found or repository is private"),
            403 | 429 => "GitHub API rate limit exceeded or access denied".into(),
            _ => format!("GitHub API error: HTTP {status_code}"),
        }),
        ..Default::default()
    }
}

fn format_run(
    owner: &str,
    repo: &str,
    run_id: u64,
    run: &WorkflowRun,
    jobs: &JobsResponse,
) -> String {
    let workflow = run.name.as_deref().unwrap_or("GitHub Actions");
    let mut out = format!(
        "# {} — {}\n\n- **Repository:** {owner}/{repo}\n- **Workflow:** {workflow}\n- **Run:** #{} (attempt {})\n- **Run ID:** {run_id}\n- **Event:** {}\n- **Status:** {}\n- **Conclusion:** {}\n- **Actor:** @{}\n- **Head branch:** {}\n- **Head SHA:** `{}`\n- **Created:** {}\n- **Started:** {}\n- **Updated:** {}\n- **URL:** {}\n- **Workflow ID:** {}\n",
        run.display_title,
        workflow,
        run.run_number,
        run.run_attempt,
        run.event,
        run.status,
        run.conclusion.as_deref().unwrap_or("pending"),
        run.actor.login,
        run.head_branch.as_deref().unwrap_or("unknown"),
        run.head_sha,
        run.created_at,
        run.run_started_at.as_deref().unwrap_or("not started"),
        run.updated_at,
        run.html_url,
        run.workflow_id,
    );
    out.push_str(&format!("\n## Jobs ({})\n", jobs.total_count));
    for job in &jobs.jobs {
        out.push_str(&format!(
            "\n### {} — {}\n\n- **Status:** {}\n- **Started:** {}\n- **Completed:** {}\n",
            job.name,
            job.conclusion.as_deref().unwrap_or("pending"),
            job.status,
            job.started_at.as_deref().unwrap_or("not started"),
            job.completed_at.as_deref().unwrap_or("not completed"),
        ));
        if let Some(url) = &job.html_url {
            out.push_str(&format!("- **URL:** {url}\n"));
        }
        let noteworthy: Vec<&Step> = job
            .steps
            .iter()
            .filter(|step| {
                step.conclusion.as_deref().is_some_and(|conclusion| {
                    !matches!(conclusion, "success" | "skipped" | "neutral")
                }) || (step.status != "completed" && step.status != "queued")
            })
            .collect();
        if !noteworthy.is_empty() {
            out.push_str("\n**Failed or incomplete steps:**\n");
            for step in noteworthy {
                out.push_str(&format!(
                    "- {}. {} — {}\n",
                    step.number,
                    step.name,
                    step.conclusion.as_deref().unwrap_or(&step.status)
                ));
            }
        }
    }
    if jobs.jobs.len() < jobs.total_count as usize {
        out.push_str(&format!(
            "\n_Only the first {} of {} jobs are shown._\n",
            jobs.jobs.len(),
            jobs.total_count
        ));
    }
    out
}

fn valid_name(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".."
}

fn percent_decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .into_owned()
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
    fn parses_actions_run_url() {
        assert_eq!(
            GitHubActionsRunFetcher::parse_url(
                &Url::parse("https://github.com/everruns/fetchkit/actions/runs/123456").unwrap()
            ),
            Some(("everruns".into(), "fetchkit".into(), 123456))
        );
    }

    #[test]
    fn rejects_job_pages_invalid_ids_and_other_hosts() {
        for value in [
            "https://github.com/everruns/fetchkit/actions/runs/0",
            "https://github.com/everruns/fetchkit/actions/runs/nope",
            "https://github.com/everruns/fetchkit/actions/runs/1/job/2",
            "https://gitlab.com/everruns/fetchkit/actions/runs/1",
        ] {
            assert!(!GitHubActionsRunFetcher::new().matches(&Url::parse(value).unwrap()));
        }
    }

    #[test]
    fn formats_run_jobs_and_failed_steps() {
        let run = WorkflowRun {
            name: Some("CI".into()),
            display_title: "Fix parser".into(),
            event: "pull_request".into(),
            status: "completed".into(),
            conclusion: Some("failure".into()),
            workflow_id: 42,
            run_number: 8,
            run_attempt: 2,
            head_branch: Some("fix/parser".into()),
            head_sha: "abcdef".into(),
            html_url: "https://github.com/o/r/actions/runs/1".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:02:00Z".into(),
            run_started_at: Some("2026-01-01T00:01:00Z".into()),
            actor: User {
                login: "octocat".into(),
            },
        };
        let jobs = JobsResponse {
            total_count: 1,
            jobs: vec![Job {
                name: "test".into(),
                status: "completed".into(),
                conclusion: Some("failure".into()),
                started_at: Some("start".into()),
                completed_at: Some("end".into()),
                html_url: Some("https://github.com/o/r/actions/runs/1/job/2".into()),
                steps: vec![
                    Step {
                        name: "Checkout".into(),
                        status: "completed".into(),
                        conclusion: Some("success".into()),
                        number: 1,
                    },
                    Step {
                        name: "Tests".into(),
                        status: "completed".into(),
                        conclusion: Some("failure".into()),
                        number: 2,
                    },
                ],
            }],
        };
        let output = format_run("o", "r", 1, &run, &jobs);
        assert!(output.contains("# Fix parser — CI"));
        assert!(output.contains("- **Conclusion:** failure"));
        assert!(output.contains("### test — failure"));
        assert!(output.contains("- 2. Tests — failure"));
        assert!(!output.contains("1. Checkout"));
    }

    #[test]
    fn reports_job_pagination_and_truncates_utf8_safely() {
        let run: WorkflowRun = serde_json::from_value(serde_json::json!({
            "name":null,"display_title":"Run","event":"push","status":"queued","conclusion":null,
            "workflow_id":1,"run_number":1,"run_attempt":1,"head_branch":null,"head_sha":"a",
            "html_url":"url","created_at":"now","updated_at":"now","run_started_at":null,
            "actor":{"login":"bot"}
        }))
        .unwrap();
        let output = format_run(
            "o",
            "r",
            1,
            &run,
            &JobsResponse {
                total_count: 2,
                jobs: vec![],
            },
        );
        assert!(output.contains("Only the first 0 of 2 jobs are shown"));
        assert_eq!(truncate("aéz".into(), 2), ("a".into(), true));
    }
}
