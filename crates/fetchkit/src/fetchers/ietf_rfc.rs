//! IETF RFC fetcher using canonical RFC Editor plain-text publications.

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::fetchers::default::{read_full_body, transport_request};
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use std::time::Duration;
use url::Url;

const TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_BODY_SIZE: usize = 5 * 1024 * 1024;

pub struct IetfRfcFetcher;

impl IetfRfcFetcher {
    pub fn new() -> Self {
        Self
    }

    fn rfc_number(url: &Url) -> Option<u32> {
        let host = url.host_str()?;
        let segments: Vec<&str> = url
            .path_segments()?
            .filter(|segment| !segment.is_empty())
            .collect();
        let candidate = match host {
            "datatracker.ietf.org" => match segments.as_slice() {
                ["doc", "html", value] | ["doc", value] => *value,
                _ => return None,
            },
            "www.rfc-editor.org" | "rfc-editor.org" => match segments.as_slice() {
                ["rfc", value] => *value,
                _ => return None,
            },
            "www.ietf.org" | "ietf.org" => match segments.as_slice() {
                ["rfc", value] => *value,
                _ => return None,
            },
            "tools.ietf.org" => match segments.as_slice() {
                ["html", value] => *value,
                _ => return None,
            },
            _ => return None,
        };
        parse_rfc(candidate)
    }
}

impl Default for IetfRfcFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for IetfRfcFetcher {
    fn name(&self) -> &'static str {
        "ietf_rfc"
    }

    fn matches(&self, url: &Url) -> bool {
        Self::rfc_number(url).is_some()
    }

    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        let request = request.normalized_for_fetch()?;
        let page_url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;
        let number = Self::rfc_number(&page_url)
            .ok_or_else(|| FetchError::FetcherError("Not a supported RFC URL".into()))?;
        let source_url = Url::parse(&format!("https://www.rfc-editor.org/rfc/rfc{number}.txt"))
            .map_err(|_| FetchError::InvalidUrlScheme)?;
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT))
                .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT)),
        );
        headers.insert(ACCEPT, HeaderValue::from_static("text/plain"));
        let response = transport_request(
            source_url,
            reqwest::Method::GET,
            headers,
            options,
            TIMEOUT,
            "www.rfc-editor.org",
            443,
        )
        .await?;
        let status_code = response.status;
        if !(200..300).contains(&status_code) {
            return Ok(FetchResponse {
                url: request.url,
                status_code,
                error: Some(match status_code {
                    404 => format!("RFC {number} not found"),
                    _ => format!("RFC Editor returned HTTP {status_code}"),
                }),
                ..Default::default()
            });
        }
        let body = read_full_body(response, options).await?;
        let content = String::from_utf8(body.to_vec())
            .map_err(|_| FetchError::FetcherError("RFC publication is not valid UTF-8".into()))?;
        let content = normalize_text(&content);
        let (content, truncated) = truncate(
            content,
            options.max_body_size.unwrap_or(DEFAULT_MAX_BODY_SIZE),
        );
        let size = u64::try_from(content.len()).unwrap_or(u64::MAX);
        Ok(FetchResponse {
            url: request.url,
            status_code,
            content_type: Some("text/plain".into()),
            format: Some("ietf_rfc".into()),
            content: Some(content),
            size: Some(size),
            truncated: Some(truncated),
            ..Default::default()
        })
    }
}

fn parse_rfc(value: &str) -> Option<u32> {
    let value = value
        .strip_suffix(".html")
        .or_else(|| value.strip_suffix(".txt"))
        .or_else(|| value.strip_suffix(".xml"))
        .unwrap_or(value);
    let digits = value
        .strip_prefix("rfc")
        .or_else(|| value.strip_prefix("RFC"))?;
    let number = digits.parse::<u32>().ok()?;
    (number > 0).then_some(number)
}

fn normalize_text(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
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
    fn parses_supported_rfc_urls() {
        for input in [
            "https://datatracker.ietf.org/doc/html/rfc9110",
            "https://datatracker.ietf.org/doc/rfc9110/",
            "https://www.rfc-editor.org/rfc/rfc9110.html",
            "https://www.rfc-editor.org/rfc/rfc9110.txt",
            "https://www.ietf.org/rfc/rfc9110.txt",
            "https://tools.ietf.org/html/rfc9110",
        ] {
            assert_eq!(
                IetfRfcFetcher::rfc_number(&Url::parse(input).unwrap()),
                Some(9110),
                "{input}"
            );
        }
    }

    #[test]
    fn rejects_non_rfc_and_malformed_urls() {
        for input in [
            "https://datatracker.ietf.org/doc/draft-example/",
            "https://www.rfc-editor.org/",
            "https://www.rfc-editor.org/rfc/rfc0.txt",
            "https://www.rfc-editor.org/rfc/rfcnope.txt",
            "https://example.com/rfc9110",
        ] {
            assert!(
                IetfRfcFetcher::rfc_number(&Url::parse(input).unwrap()).is_none(),
                "{input}"
            );
        }
    }

    #[test]
    fn normalizes_line_endings_and_truncates_utf8_safely() {
        assert_eq!(normalize_text("one\r\ntwo\rthree"), "one\ntwo\nthree");
        assert_eq!(truncate("aéz".into(), 2), ("a".into(), true));
    }
}
