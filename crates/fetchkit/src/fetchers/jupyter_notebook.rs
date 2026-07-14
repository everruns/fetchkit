//! Jupyter notebook fetcher for GitHub and GitLab blob URLs.

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::fetchers::default::{read_full_body, transport_request};
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde_json::Value;
use std::time::Duration;
use url::Url;

const TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_BODY_SIZE: usize = 5 * 1024 * 1024;

pub struct JupyterNotebookFetcher;

impl JupyterNotebookFetcher {
    pub fn new() -> Self {
        Self
    }

    fn raw_url(url: &Url) -> Option<(Url, &'static str)> {
        let segments: Vec<&str> = url.path_segments()?.collect();
        if !segments.last()?.to_ascii_lowercase().ends_with(".ipynb") {
            return None;
        }
        match url.host_str()? {
            "github.com" if segments.len() >= 5 && segments[2] == "blob" => {
                let raw = format!(
                    "https://raw.githubusercontent.com/{}/{}/{}/{}",
                    segments[0],
                    segments[1],
                    segments[3],
                    segments[4..].join("/")
                );
                Some((Url::parse(&raw).ok()?, "raw.githubusercontent.com"))
            }
            "gitlab.com" => {
                let marker = segments.iter().position(|part| *part == "-")?;
                if marker < 2
                    || segments.get(marker + 1) != Some(&"blob")
                    || marker + 4 > segments.len()
                {
                    return None;
                }
                let raw = format!(
                    "https://gitlab.com/{}/-/raw/{}/{}",
                    segments[..marker].join("/"),
                    segments[marker + 2],
                    segments[marker + 3..].join("/")
                );
                Some((Url::parse(&raw).ok()?, "gitlab.com"))
            }
            _ => None,
        }
    }
}

impl Default for JupyterNotebookFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for JupyterNotebookFetcher {
    fn name(&self) -> &'static str {
        "jupyter_notebook"
    }
    fn matches(&self, url: &Url) -> bool {
        Self::raw_url(url).is_some()
    }

    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        let request = request.normalized_for_fetch()?;
        let page_url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;
        let (raw_url, host) = Self::raw_url(&page_url)
            .ok_or_else(|| FetchError::FetcherError("Not a supported notebook URL".into()))?;
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT))
                .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT)),
        );
        let response = transport_request(
            raw_url,
            reqwest::Method::GET,
            headers,
            options,
            TIMEOUT,
            host,
            443,
        )
        .await?;
        let status_code = response.status;
        if !(200..300).contains(&status_code) {
            return Ok(FetchResponse {
                url: request.url,
                status_code,
                error: Some(format!("Notebook source returned HTTP {status_code}")),
                ..Default::default()
            });
        }
        let body = read_full_body(response, options).await?;
        let notebook: Value = serde_json::from_slice(&body).map_err(|error| {
            FetchError::FetcherError(format!("Invalid Jupyter notebook: {error}"))
        })?;
        let title = page_url
            .path_segments()
            .and_then(|mut parts| parts.next_back())
            .unwrap_or("Notebook");
        let content = render_notebook(title, &notebook)?;
        let (content, truncated) = truncate(
            content,
            options.max_body_size.unwrap_or(DEFAULT_MAX_BODY_SIZE),
        );
        let size = u64::try_from(content.len()).unwrap_or(u64::MAX);
        Ok(FetchResponse {
            url: request.url,
            status_code,
            content_type: Some("text/markdown".into()),
            format: Some("jupyter_notebook".into()),
            content: Some(content),
            size: Some(size),
            truncated: Some(truncated),
            ..Default::default()
        })
    }
}

fn source(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(lines)) => lines.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

fn render_notebook(title: &str, notebook: &Value) -> Result<String, FetchError> {
    let cells = notebook
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| FetchError::FetcherError("Notebook has no cells array".into()))?;
    let language = notebook
        .pointer("/metadata/language_info/name")
        .and_then(Value::as_str)
        .unwrap_or("python");
    let mut out = format!(
        "# {title}\n\n- **Kernel language:** {language}\n- **Cells:** {}\n\n",
        cells.len()
    );
    for cell in cells {
        match cell.get("cell_type").and_then(Value::as_str) {
            Some("markdown") => {
                out.push_str(&source(cell.get("source")));
                out.push_str("\n\n");
            }
            Some("code") => {
                let code = source(cell.get("source"));
                let count = cell
                    .get("execution_count")
                    .and_then(Value::as_u64)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| " ".into());
                out.push_str(&format!(
                    "## In [{count}]\n\n{}\n\n",
                    fenced(language, &code)
                ));
                let outputs: Vec<String> = cell
                    .get("outputs")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(text_output)
                    .collect();
                if !outputs.is_empty() {
                    out.push_str(&format!(
                        "### Output\n\n{}\n\n",
                        fenced("text", &outputs.join("\n"))
                    ));
                }
            }
            Some("raw") => {
                out.push_str(&format!(
                    "{}\n\n",
                    fenced("text", &source(cell.get("source")))
                ));
            }
            _ => {}
        }
    }
    Ok(out)
}

fn text_output(output: &Value) -> Option<String> {
    match output.get("output_type").and_then(Value::as_str) {
        Some("stream") => Some(source(output.get("text"))),
        Some("error") => {
            let traceback = source(output.get("traceback"));
            if traceback.is_empty() {
                Some(format!(
                    "{}: {}",
                    output.get("ename")?.as_str()?,
                    output.get("evalue")?.as_str()?
                ))
            } else {
                Some(traceback)
            }
        }
        Some("execute_result" | "display_data") => {
            let data = output.get("data")?;
            let text = source(data.get("text/plain"));
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn fenced(language: &str, content: &str) -> String {
    let longest = content
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}{language}\n{content}\n{fence}")
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
    fn maps_github_and_nested_gitlab_blob_urls_to_raw_sources() {
        let (url, host) = JupyterNotebookFetcher::raw_url(
            &Url::parse("https://github.com/org/repo/blob/main/notebooks/demo.ipynb").unwrap(),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://raw.githubusercontent.com/org/repo/main/notebooks/demo.ipynb"
        );
        assert_eq!(host, "raw.githubusercontent.com");
        let (url, host) = JupyterNotebookFetcher::raw_url(
            &Url::parse("https://gitlab.com/group/sub/repo/-/blob/dev/demo.ipynb").unwrap(),
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://gitlab.com/group/sub/repo/-/raw/dev/demo.ipynb"
        );
        assert_eq!(host, "gitlab.com");
    }

    #[test]
    fn rejects_non_notebooks_and_unsupported_hosts() {
        for input in [
            "https://github.com/org/repo/blob/main/demo.py",
            "https://example.com/demo.ipynb",
            "https://gitlab.com/org/repo/-/blob/main",
        ] {
            assert!(JupyterNotebookFetcher::raw_url(&Url::parse(input).unwrap()).is_none());
        }
    }

    #[test]
    fn renders_markdown_code_text_outputs_and_errors_but_not_images() {
        let notebook = serde_json::json!({"metadata":{"language_info":{"name":"rust"}},"cells":[
            {"cell_type":"markdown","source":["## Intro\n","Hello"]},
            {"cell_type":"code","execution_count":2,"source":"println!(\"hi\");","outputs":[
                {"output_type":"stream","text":["hi\n"]},
                {"output_type":"display_data","data":{"text/plain":"chart","image/png":"AAAA"}},
                {"output_type":"error","ename":"Error","evalue":"bad","traceback":["Error: bad"]}
            ]}
        ]});
        let output = render_notebook("demo.ipynb", &notebook).unwrap();
        assert!(output.contains("## Intro\nHello"));
        assert!(output.contains("## In [2]"));
        assert!(output.contains("```rust\nprintln!"));
        assert!(output.contains("hi\n\nchart\nError: bad"));
        assert!(!output.contains("AAAA"));
    }

    #[test]
    fn chooses_safe_fence_and_utf8_truncation() {
        assert!(fenced("text", "``` inside").starts_with("````text"));
        assert_eq!(truncate("aéz".into(), 2), ("a".into(), true));
    }
}
