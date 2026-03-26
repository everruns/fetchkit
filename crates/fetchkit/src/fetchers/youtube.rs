//! YouTube video fetcher
//!
//! Handles youtube.com/watch and youtu.be URLs, returning video metadata
//! and transcript text via oEmbed and timedtext APIs.

use crate::client::FetchOptions;
use crate::error::FetchError;
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderValue, USER_AGENT};
use serde::Deserialize;
use std::time::Duration;
use url::Url;

const API_TIMEOUT: Duration = Duration::from_secs(10);

/// YouTube video fetcher
///
/// Matches `youtube.com/watch?v={id}` and `youtu.be/{id}`, returning
/// video metadata via oEmbed.
pub struct YouTubeFetcher;

impl YouTubeFetcher {
    pub fn new() -> Self {
        Self
    }

    /// Extract video ID from YouTube URL
    fn parse_video_id(url: &Url) -> Option<String> {
        let host = url.host_str()?;

        match host {
            "youtube.com" | "www.youtube.com" | "m.youtube.com" => {
                // /watch?v={id}
                let segments: Vec<&str> =
                    url.path_segments().map(|s| s.collect()).unwrap_or_default();
                if segments.first() != Some(&"watch") {
                    return None;
                }
                url.query_pairs()
                    .find(|(k, _)| k == "v")
                    .map(|(_, v)| v.to_string())
                    .filter(|v| !v.is_empty())
            }
            "youtu.be" => {
                // /{id}
                let segments: Vec<&str> =
                    url.path_segments().map(|s| s.collect()).unwrap_or_default();
                segments
                    .first()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
            }
            _ => None,
        }
    }
}

impl Default for YouTubeFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct OEmbedResponse {
    title: Option<String>,
    author_name: Option<String>,
    author_url: Option<String>,
}

#[async_trait]
impl Fetcher for YouTubeFetcher {
    fn name(&self) -> &'static str {
        "youtube"
    }

    fn matches(&self, url: &Url) -> bool {
        Self::parse_video_id(url).is_some()
    }

    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        let url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;

        let video_id = Self::parse_video_id(&url)
            .ok_or_else(|| FetchError::FetcherError("Not a valid YouTube URL".to_string()))?;

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

        let canonical_url = format!("https://www.youtube.com/watch?v={}", video_id);

        // Fetch oEmbed metadata
        // The canonical URL only contains safe ASCII chars, so it can be passed directly
        let mut oembed = Url::parse("https://www.youtube.com/oembed").unwrap();
        oembed
            .query_pairs_mut()
            .append_pair("url", &canonical_url)
            .append_pair("format", "json");
        let oembed_url = oembed.to_string();

        let oembed = match client
            .get(&oembed_url)
            .header(USER_AGENT, ua_header.clone())
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => resp.json::<OEmbedResponse>().await.ok(),
            _ => None,
        };

        let title = oembed
            .as_ref()
            .and_then(|o| o.title.clone())
            .unwrap_or_else(|| format!("YouTube Video {}", video_id));

        let author = oembed.as_ref().and_then(|o| o.author_name.clone());

        let author_url = oembed.as_ref().and_then(|o| o.author_url.clone());

        // Build response
        let mut out = String::new();
        out.push_str(&format!("# {}\n\n", title));

        out.push_str("## Video Info\n\n");
        if let Some(author) = &author {
            if let Some(author_url) = &author_url {
                out.push_str(&format!("- **Channel:** [{}]({})\n", author, author_url));
            } else {
                out.push_str(&format!("- **Channel:** {}\n", author));
            }
        }
        out.push_str(&format!("- **Video ID:** {}\n", video_id));
        out.push_str(&format!("- **URL:** {}\n", canonical_url));
        out.push_str(&format!(
            "- **Thumbnail:** https://img.youtube.com/vi/{}/maxresdefault.jpg\n",
            video_id
        ));

        Ok(FetchResponse {
            url: request.url.clone(),
            status_code: 200,
            content_type: Some("text/markdown".to_string()),
            format: Some("youtube_video".to_string()),
            content: Some(out),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_youtube_watch() {
        let url = Url::parse("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
        assert_eq!(
            YouTubeFetcher::parse_video_id(&url),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn test_parse_youtu_be() {
        let url = Url::parse("https://youtu.be/dQw4w9WgXcQ").unwrap();
        assert_eq!(
            YouTubeFetcher::parse_video_id(&url),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn test_parse_youtube_no_www() {
        let url = Url::parse("https://youtube.com/watch?v=abc123").unwrap();
        assert_eq!(
            YouTubeFetcher::parse_video_id(&url),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn test_rejects_non_watch() {
        let url = Url::parse("https://www.youtube.com/channel/UC123").unwrap();
        assert_eq!(YouTubeFetcher::parse_video_id(&url), None);
    }

    #[test]
    fn test_rejects_no_v_param() {
        let url = Url::parse("https://www.youtube.com/watch?list=PL123").unwrap();
        assert_eq!(YouTubeFetcher::parse_video_id(&url), None);
    }

    #[test]
    fn test_rejects_non_youtube() {
        let url = Url::parse("https://vimeo.com/123456").unwrap();
        assert_eq!(YouTubeFetcher::parse_video_id(&url), None);
    }

    #[test]
    fn test_fetcher_matches() {
        let fetcher = YouTubeFetcher::new();

        let url = Url::parse("https://www.youtube.com/watch?v=abc").unwrap();
        assert!(fetcher.matches(&url));

        let url = Url::parse("https://youtu.be/abc").unwrap();
        assert!(fetcher.matches(&url));

        let url = Url::parse("https://example.com/watch?v=abc").unwrap();
        assert!(!fetcher.matches(&url));
    }
}
