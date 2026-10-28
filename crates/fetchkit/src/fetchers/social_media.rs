//! Public social media metadata through fixed provider oEmbed endpoints.
//!
//! Embed HTML is deliberately ignored: it contains executable players, not article
//! content. Only canonical public resource URLs are sent to providers; tracking
//! queries, credentials and short-link redirects are never forwarded.

use crate::client::FetchOptions;
use crate::convert::html_to_markdown_with_base_url;
use crate::error::FetchError;
use crate::fetchers::default::{read_body_with_timeout, transport_request};
use crate::fetchers::{validate_url_policy, Fetcher};
use crate::types::{FetchRequest, FetchResponse, HttpMethod};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use serde::Deserialize;
use std::time::Duration;
use url::Url;

/// TikTok videos, Vimeo videos, and SoundCloud tracks/playlists via public oEmbed.
pub struct SocialMediaFetcher;

impl SocialMediaFetcher {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SocialMediaFetcher {
    fn default() -> Self {
        Self::new()
    }
}

struct Resource {
    provider: &'static str,
    endpoint: &'static str,
    format: &'static str,
    url: String,
}

fn numeric(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn resource(url: &Url) -> Option<Resource> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let path: Vec<_> = url
        .path()
        .trim_end_matches('/')
        .split('/')
        .skip(1)
        .collect();
    match (url.host_str()?, path.as_slice()) {
        ("www.tiktok.com" | "tiktok.com" | "m.tiktok.com", [user, "video", id])
            if user.strip_prefix('@').is_some_and(slug) && numeric(id) =>
        {
            Some(Resource {
                provider: "TikTok",
                endpoint: "https://www.tiktok.com/oembed",
                format: "tiktok_video",
                url: format!("https://www.tiktok.com/{user}/video/{id}"),
            })
        }
        ("vimeo.com" | "www.vimeo.com", [id]) | ("player.vimeo.com", ["video", id])
            if numeric(id) =>
        {
            Some(Resource {
                provider: "Vimeo",
                endpoint: "https://vimeo.com/api/oembed.json",
                format: "vimeo_video",
                url: format!("https://vimeo.com/{id}"),
            })
        }
        ("soundcloud.com" | "www.soundcloud.com" | "m.soundcloud.com", parts) => {
            let (user, name, playlist) = match parts {
                [user, name] => (*user, *name, false),
                [user, "sets", name] => (*user, *name, true),
                _ => return None,
            };
            if !slug(user)
                || !slug(name)
                || matches!(
                    user,
                    "discover"
                        | "stream"
                        | "you"
                        | "search"
                        | "charts"
                        | "upload"
                        | "settings"
                        | "pages"
                        | "terms-of-use"
                        | "jobs"
                )
                || (!playlist
                    && matches!(
                        name,
                        "sets"
                            | "tracks"
                            | "albums"
                            | "reposts"
                            | "likes"
                            | "popular-tracks"
                            | "followers"
                            | "following"
                    ))
            {
                return None;
            }
            Some(Resource {
                provider: "SoundCloud",
                endpoint: "https://soundcloud.com/oembed",
                format: if playlist {
                    "soundcloud_playlist"
                } else {
                    "soundcloud_track"
                },
                url: format!(
                    "https://soundcloud.com/{user}/{}{name}",
                    if playlist { "sets/" } else { "" }
                ),
            })
        }
        _ => None,
    }
}

#[derive(Deserialize)]
struct OEmbed {
    #[serde(rename = "type")]
    kind: String,
    title: Option<String>,
    author_name: Option<String>,
    author_url: Option<String>,
    thumbnail_url: Option<String>,
    description: Option<String>,
}

#[async_trait]
impl Fetcher for SocialMediaFetcher {
    fn name(&self) -> &'static str {
        "social_media"
    }

    fn matches(&self, url: &Url) -> bool {
        resource(url).is_some()
    }

    async fn fetch(
        &self,
        request: &FetchRequest,
        options: &FetchOptions,
    ) -> Result<FetchResponse, FetchError> {
        let request = request.normalized_for_fetch()?;
        let page_url = Url::parse(&request.url).map_err(|_| FetchError::InvalidUrlScheme)?;
        validate_url_policy(&page_url, options)?;
        let resource = resource(&page_url)
            .ok_or_else(|| FetchError::FetcherError("Unsupported social media URL".into()))?;
        let mut api_url = Url::parse(resource.endpoint).unwrap();
        api_url
            .query_pairs_mut()
            .append_pair("url", &resource.url)
            .append_pair("format", "json");
        let host = api_url.host_str().unwrap().to_owned();
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT))
                .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT)),
        );
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        // THREAT[TM-SSRF-010]: transport validates the complete derived URL and
        // pins DNS. Redirects and URLs in provider metadata are never fetched.
        let response = transport_request(
            api_url,
            if request.method == Some(HttpMethod::Head) {
                reqwest::Method::HEAD
            } else {
                reqwest::Method::GET
            },
            headers,
            options,
            Duration::from_secs(10),
            &host,
            443,
        )
        .await?;
        let status_code = response.status;
        if !(200..300).contains(&status_code) {
            return Ok(FetchResponse {
                url: request.url,
                status_code,
                error: Some(format!(
                    "{} oEmbed unavailable: HTTP {status_code}",
                    resource.provider
                )),
                ..Default::default()
            });
        }
        let mut result = FetchResponse {
            url: request.url,
            status_code,
            content_type: Some("text/markdown".into()),
            format: Some(resource.format.into()),
            ..Default::default()
        };
        if request.method == Some(HttpMethod::Head) {
            return Ok(result);
        }
        let limit = options.max_body_size.unwrap_or(10 * 1024 * 1024);
        let (body, incomplete) =
            read_body_with_timeout(response, Duration::from_secs(30), limit).await?;
        if incomplete {
            return Err(FetchError::FetcherError(format!(
                "Incomplete {} oEmbed metadata (body limit, timeout or stream error)",
                resource.provider
            )));
        }
        let metadata: OEmbed = serde_json::from_slice(&body).map_err(|_| {
            FetchError::FetcherError(format!("Invalid {} oEmbed metadata", resource.provider))
        })?;
        if !matches!(metadata.kind.as_str(), "video" | "rich") {
            return Err(FetchError::FetcherError(format!(
                "Unsupported {} oEmbed type",
                resource.provider
            )));
        }
        let mut content = render(&resource, &metadata);
        let truncated = content.len() > limit;
        let mut end = content.len().min(limit);
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        content.truncate(end);
        result.size = Some(content.len() as u64);
        result.content = Some(content);
        result.truncated = Some(truncated);
        Ok(result)
    }
}

fn render(resource: &Resource, metadata: &OEmbed) -> String {
    let title = metadata
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    let mut content = format!(
        "# {}\n\n- **Platform:** {}\n- **URL:** {}\n",
        title.unwrap_or(resource.provider),
        resource.provider,
        resource.url
    );
    for (label, value) in [
        ("Author", metadata.author_name.as_deref()),
        ("Author URL", metadata.author_url.as_deref()),
        ("Thumbnail", metadata.thumbnail_url.as_deref()),
    ] {
        if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
            content.push_str(&format!("- **{label}:** {value}\n"));
        }
    }
    if let Some(description) = metadata
        .description
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let description = html_to_markdown_with_base_url(description, &resource.url);
        if !description.trim().is_empty() {
            content.push_str(&format!("\n## Description\n\n{}\n", description.trim()));
        }
    }
    content.push_str("\n*Public metadata only; media and transcripts are not retrieved.*\n");
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_supported_resource_paths() {
        let fetcher = SocialMediaFetcher::new();
        for url in [
            "https://www.tiktok.com/@scout2015/video/6718335390845095173",
            "https://m.tiktok.com/@a.b/video/123/?tracking=secret",
            "https://vimeo.com/123",
            "https://player.vimeo.com/video/123",
            "https://soundcloud.com/forss/flickermood",
            "https://www.soundcloud.com/artist/sets/playlist/",
        ] {
            assert!(fetcher.matches(&Url::parse(url).unwrap()), "{url}");
        }
        for url in [
            "https://tiktok.com/@user",
            "https://tiktok.com/@user/video/nope",
            "https://tiktok.com/user/video/123",
            "https://vm.tiktok.com/abc",
            "https://tiktok.com.evil.test/@user/video/123",
            "https://vimeo.com/123/privatehash",
            "https://vimeo.com/channels/staffpicks",
            "https://soundcloud.com/artist",
            "https://soundcloud.com/search/sounds",
            "https://soundcloud.com/artist/tracks",
            "https://soundcloud.com/artist/sets",
            "https://soundcloud.com/artist/track/secret",
            "https://soundcloud.com/artist//track",
            "ftp://vimeo.com/123",
        ] {
            assert!(!fetcher.matches(&Url::parse(url).unwrap()), "{url}");
        }
    }
}
