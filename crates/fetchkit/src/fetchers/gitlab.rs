//! GitLab project, source file, issue, merge request, and release fetcher.

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::fetchers::default::{read_full_body, transport_request};
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use serde_json::Value;
use std::time::Duration;
use url::{form_urlencoded, Url};

const API_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_MAX_BODY_SIZE: usize = 5 * 1024 * 1024;

pub struct GitLabFetcher;

#[derive(Debug, PartialEq)]
enum Resource {
    Project {
        project: String,
    },
    Blob {
        project: String,
        reference: String,
        path: String,
    },
    Issue {
        project: String,
        iid: u64,
    },
    MergeRequest {
        project: String,
        iid: u64,
    },
    Release {
        project: String,
        tag: String,
    },
}

impl GitLabFetcher {
    pub fn new() -> Self {
        Self
    }

    fn parse_url(url: &Url) -> Option<Resource> {
        if url.host_str() != Some("gitlab.com") {
            return None;
        }
        let segments: Vec<&str> = url
            .path_segments()?
            .filter(|part| !part.is_empty())
            .collect();
        if let Some(marker) = segments.iter().position(|part| *part == "-") {
            if marker < 2 || marker + 2 >= segments.len() {
                return None;
            }
            let project = segments[..marker].join("/");
            return match segments[marker + 1] {
                "blob" if marker + 4 <= segments.len() => Some(Resource::Blob {
                    project,
                    reference: segments[marker + 2].into(),
                    path: segments[marker + 3..].join("/"),
                }),
                "issues" if marker + 3 == segments.len() => Some(Resource::Issue {
                    project,
                    iid: segments[marker + 2].parse().ok()?,
                }),
                "merge_requests" if marker + 3 == segments.len() => Some(Resource::MergeRequest {
                    project,
                    iid: segments[marker + 2].parse().ok()?,
                }),
                "releases" if marker + 3 == segments.len() => Some(Resource::Release {
                    project,
                    tag: segments[marker + 2].into(),
                }),
                _ => None,
            };
        }
        (segments.len() >= 2).then(|| Resource::Project {
            project: segments.join("/"),
        })
    }
}

impl Default for GitLabFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for GitLabFetcher {
    fn name(&self) -> &'static str {
        "gitlab"
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
        let resource = Self::parse_url(&page_url)
            .ok_or_else(|| FetchError::FetcherError("Not a supported GitLab URL".into()))?;
        let (api_url, format, raw) = api_url(&resource)?;
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
            "gitlab.com",
            443,
        )
        .await?;
        let status_code = response.status;
        if !(200..300).contains(&status_code) {
            let error = match status_code {
                404 => "GitLab resource not found".into(),
                403 | 429 => "GitLab API rate limit exceeded or access denied".into(),
                _ => format!("GitLab API error: HTTP {status_code}"),
            };
            return Ok(FetchResponse {
                url: request.url,
                status_code,
                error: Some(error),
                ..Default::default()
            });
        }
        let body = read_full_body(response, options).await?;
        let content = if raw {
            render_blob(&resource, &body)
        } else {
            let value: Value = serde_json::from_slice(&body).map_err(|error| {
                FetchError::FetcherError(format!("Failed to parse GitLab response: {error}"))
            })?;
            render_json(&resource, &value)
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

fn encode(value: &str) -> String {
    form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn api_url(resource: &Resource) -> Result<(Url, &'static str, bool), FetchError> {
    let (project, suffix, format, raw) = match resource {
        Resource::Project { project } => (project, String::new(), "gitlab_project", false),
        Resource::Blob {
            project,
            reference,
            path,
        } => (
            project,
            format!(
                "/repository/files/{}/raw?ref={}",
                encode(path),
                encode(reference)
            ),
            "gitlab_blob",
            true,
        ),
        Resource::Issue { project, iid } => {
            (project, format!("/issues/{iid}"), "gitlab_issue", false)
        }
        Resource::MergeRequest { project, iid } => (
            project,
            format!("/merge_requests/{iid}"),
            "gitlab_merge_request",
            false,
        ),
        Resource::Release { project, tag } => (
            project,
            format!("/releases/{}", encode(tag)),
            "gitlab_release",
            false,
        ),
    };
    let url = Url::parse(&format!(
        "https://gitlab.com/api/v4/projects/{}{}",
        encode(project),
        suffix
    ))
    .map_err(|_| FetchError::InvalidUrlScheme)?;
    Ok((url, format, raw))
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str().filter(|value| !value.is_empty())
}

fn render_json(resource: &Resource, value: &Value) -> String {
    match resource {
        Resource::Project { .. } => {
            let title = text(value, "name_with_namespace")
                .or_else(|| text(value, "name"))
                .unwrap_or("GitLab project");
            let mut out = format!("# {title}\n\n");
            field(&mut out, "Description", text(value, "description"));
            field(&mut out, "Default branch", text(value, "default_branch"));
            field(&mut out, "Visibility", text(value, "visibility"));
            field(&mut out, "Created", text(value, "created_at"));
            field(&mut out, "Last activity", text(value, "last_activity_at"));
            field(&mut out, "URL", text(value, "web_url"));
            number_field(&mut out, "Stars", value.get("star_count"));
            number_field(&mut out, "Forks", value.get("forks_count"));
            out
        }
        Resource::Issue { .. } | Resource::MergeRequest { .. } => {
            let kind = if matches!(resource, Resource::Issue { .. }) {
                "Issue"
            } else {
                "Merge request"
            };
            let mut out = format!(
                "# {kind} #{}: {}\n\n",
                value.get("iid").and_then(Value::as_u64).unwrap_or(0),
                text(value, "title").unwrap_or("Untitled")
            );
            field(&mut out, "State", text(value, "state"));
            field(
                &mut out,
                "Author",
                value.pointer("/author/username").and_then(Value::as_str),
            );
            field(&mut out, "Created", text(value, "created_at"));
            field(&mut out, "Updated", text(value, "updated_at"));
            field(&mut out, "URL", text(value, "web_url"));
            if let Some(labels) = value.get("labels").and_then(Value::as_array) {
                let labels: Vec<&str> = labels.iter().filter_map(Value::as_str).collect();
                if !labels.is_empty() {
                    out.push_str(&format!("- **Labels:** {}\n", labels.join(", ")));
                }
            }
            if let Some(description) = text(value, "description") {
                out.push_str(&format!("\n## Description\n\n{description}\n"));
            }
            out
        }
        Resource::Release { .. } => {
            let mut out = format!(
                "# {}\n\n",
                text(value, "name")
                    .or_else(|| text(value, "tag_name"))
                    .unwrap_or("GitLab release")
            );
            field(&mut out, "Tag", text(value, "tag_name"));
            field(&mut out, "Released", text(value, "released_at"));
            field(
                &mut out,
                "URL",
                value.pointer("/_links/self").and_then(Value::as_str),
            );
            if let Some(description) = text(value, "description") {
                out.push_str(&format!("\n## Release notes\n\n{description}\n"));
            }
            if let Some(links) = value.pointer("/assets/links").and_then(Value::as_array) {
                if !links.is_empty() {
                    out.push_str("\n## Assets\n\n");
                }
                for link in links {
                    if let (Some(name), Some(url)) = (
                        text(link, "name"),
                        text(link, "direct_asset_url").or_else(|| text(link, "url")),
                    ) {
                        out.push_str(&format!("- [{name}]({url})\n"));
                    }
                }
            }
            out
        }
        Resource::Blob { .. } => unreachable!(),
    }
}

fn render_blob(resource: &Resource, body: &[u8]) -> String {
    let Resource::Blob {
        path, reference, ..
    } = resource
    else {
        unreachable!()
    };
    let content = String::from_utf8_lossy(body);
    let language = path
        .rsplit('.')
        .next()
        .filter(|part| *part != path)
        .unwrap_or("");
    format!("# {path}\n\n- **Ref:** {reference}\n\n```{language}\n{content}\n```\n")
}

fn field(out: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        out.push_str(&format!("- **{label}:** {value}\n"));
    }
}

fn number_field(out: &mut String, label: &str, value: Option<&Value>) {
    if let Some(value) = value.and_then(Value::as_u64) {
        out.push_str(&format!("- **{label}:** {value}\n"));
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
    fn parses_supported_urls_including_subgroups() {
        assert_eq!(
            GitLabFetcher::parse_url(&Url::parse("https://gitlab.com/group/sub/project").unwrap()),
            Some(Resource::Project {
                project: "group/sub/project".into()
            })
        );
        assert_eq!(
            GitLabFetcher::parse_url(
                &Url::parse("https://gitlab.com/group/project/-/issues/42").unwrap()
            ),
            Some(Resource::Issue {
                project: "group/project".into(),
                iid: 42
            })
        );
        assert_eq!(
            GitLabFetcher::parse_url(
                &Url::parse("https://gitlab.com/group/project/-/merge_requests/7").unwrap()
            ),
            Some(Resource::MergeRequest {
                project: "group/project".into(),
                iid: 7
            })
        );
        assert_eq!(
            GitLabFetcher::parse_url(
                &Url::parse("https://gitlab.com/group/project/-/blob/main/src/lib.rs").unwrap()
            ),
            Some(Resource::Blob {
                project: "group/project".into(),
                reference: "main".into(),
                path: "src/lib.rs".into()
            })
        );
        assert_eq!(
            GitLabFetcher::parse_url(
                &Url::parse("https://gitlab.com/group/project/-/releases/v1.0").unwrap()
            ),
            Some(Resource::Release {
                project: "group/project".into(),
                tag: "v1.0".into()
            })
        );
    }

    #[test]
    fn rejects_other_hosts_and_malformed_resources() {
        for input in [
            "https://example.com/group/project",
            "https://gitlab.com/group",
            "https://gitlab.com/group/project/-/issues/nope",
            "https://gitlab.com/group/project/-/blob/main",
        ] {
            assert_eq!(GitLabFetcher::parse_url(&Url::parse(input).unwrap()), None);
        }
    }

    #[test]
    fn builds_encoded_api_urls() {
        let (url, format, raw) = api_url(&Resource::Blob {
            project: "group/sub/project".into(),
            reference: "main".into(),
            path: "src/lib.rs".into(),
        })
        .unwrap();
        assert_eq!(url.as_str(), "https://gitlab.com/api/v4/projects/group%2Fsub%2Fproject/repository/files/src%2Flib.rs/raw?ref=main");
        assert_eq!((format, raw), ("gitlab_blob", true));
    }

    #[test]
    fn renders_issue_and_release_assets() {
        let issue: Value = serde_json::json!({"iid": 3, "title": "Bug", "state": "opened", "author": {"username": "sam"}, "labels": ["bug"], "description": "Details"});
        let output = render_json(
            &Resource::Issue {
                project: "a/b".into(),
                iid: 3,
            },
            &issue,
        );
        assert!(output.contains("# Issue #3: Bug"));
        assert!(output.contains("- **Labels:** bug"));
        assert!(output.contains("## Description\n\nDetails"));

        let release: Value = serde_json::json!({"name": "Version 1", "tag_name": "v1", "assets": {"links": [{"name": "binary", "url": "https://example.test/binary"}]}});
        let output = render_json(
            &Resource::Release {
                project: "a/b".into(),
                tag: "v1".into(),
            },
            &release,
        );
        assert!(output.contains("- [binary](https://example.test/binary)"));
    }

    #[test]
    fn blob_rendering_and_utf8_truncation_are_bounded() {
        let resource = Resource::Blob {
            project: "a/b".into(),
            reference: "main".into(),
            path: "src/lib.rs".into(),
        };
        assert!(render_blob(&resource, b"fn main() {}").contains("```rs\nfn main() {}\n```"));
        assert_eq!(truncate("aéz".into(), 2), ("a".into(), true));
    }
}
