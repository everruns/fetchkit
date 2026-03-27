//! Live integration tests for fetchers against real endpoints.
//!
//! Gated behind `--features live-tests` so they never run during normal `cargo test`.
//! Each test module maps 1:1 to a fetcher source file; CI runs only the modules
//! whose fetcher changed.
//!
//! Assertions are structural (field presence, non-empty content, expected substrings)
//! rather than exact-match, so tests tolerate minor upstream changes.

#![cfg(feature = "live-tests")]

use fetchkit::{FetchOptions, FetchRequest, FetcherRegistry};

/// Shared options for live tests — default everything, both conversions on.
fn live_options() -> FetchOptions {
    FetchOptions {
        enable_markdown: true,
        enable_text: true,
        ..Default::default()
    }
}

fn registry() -> FetcherRegistry {
    FetcherRegistry::with_defaults()
}

// ---------------------------------------------------------------------------
// github_repo
// ---------------------------------------------------------------------------
mod live_github_repo {
    use super::*;

    #[tokio::test]
    async fn fetches_repo_metadata() {
        let req = FetchRequest::new("https://github.com/rust-lang/rust");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        // Repo name should appear somewhere in the output
        assert!(
            content.contains("rust-lang/rust") || content.to_lowercase().contains("rust"),
            "content should mention the repo"
        );
        assert!(!content.is_empty());
    }
}

// ---------------------------------------------------------------------------
// github_issue
// ---------------------------------------------------------------------------
mod live_github_issue {
    use super::*;

    #[tokio::test]
    async fn fetches_issue() {
        // Well-known issue: rust-lang/rust#1 (the very first issue)
        let req = FetchRequest::new("https://github.com/rust-lang/rust/issues/1");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(!content.is_empty());
    }
}

// ---------------------------------------------------------------------------
// github_code
// ---------------------------------------------------------------------------
mod live_github_code {
    use super::*;

    #[tokio::test]
    async fn fetches_source_file() {
        let req = FetchRequest::new("https://github.com/rust-lang/rust/blob/master/README.md");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(
            content.to_lowercase().contains("rust"),
            "README should mention Rust"
        );
    }
}

// ---------------------------------------------------------------------------
// twitter
// ---------------------------------------------------------------------------
mod live_twitter {
    use super::*;

    #[tokio::test]
    async fn fetches_tweet() {
        // Rust lang announcement tweet — stable, public
        let req = FetchRequest::new("https://x.com/rustlang/status/1821986021505405014");
        let result = registry().fetch(req, live_options()).await;

        // Twitter APIs are flaky; accept success or a graceful error
        match result {
            Ok(resp) => {
                assert!(resp.status_code == 200 || resp.status_code == 403);
                if resp.status_code == 200 {
                    assert!(resp.content.is_some());
                }
            }
            Err(_) => {
                // Third-party API unavailable — acceptable
            }
        }
    }
}

// ---------------------------------------------------------------------------
// stackoverflow
// ---------------------------------------------------------------------------
mod live_stackoverflow {
    use super::*;

    #[tokio::test]
    async fn fetches_question() {
        // "What is a NullPointerException" — one of the most famous SO questions
        let req = FetchRequest::new(
            "https://stackoverflow.com/questions/218384/what-is-a-nullpointerexception-and-how-do-i-fix-it",
        );
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(
            content.to_lowercase().contains("null"),
            "content should mention null"
        );
    }
}

// ---------------------------------------------------------------------------
// package_registry (crates.io)
// ---------------------------------------------------------------------------
mod live_package_registry {
    use super::*;

    #[tokio::test]
    async fn fetches_crate() {
        let req = FetchRequest::new("https://crates.io/crates/serde");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(
            content.to_lowercase().contains("serde"),
            "content should mention serde"
        );
    }

    #[tokio::test]
    async fn fetches_pypi_package() {
        let req = FetchRequest::new("https://pypi.org/project/requests/");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(
            content.to_lowercase().contains("requests"),
            "content should mention requests"
        );
    }

    #[tokio::test]
    async fn fetches_npm_package() {
        let req = FetchRequest::new("https://www.npmjs.com/package/express");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(
            content.to_lowercase().contains("express"),
            "content should mention express"
        );
    }
}

// ---------------------------------------------------------------------------
// wikipedia
// ---------------------------------------------------------------------------
mod live_wikipedia {
    use super::*;

    #[tokio::test]
    async fn fetches_article() {
        let req = FetchRequest::new("https://en.wikipedia.org/wiki/Rust_(programming_language)");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(
            content.to_lowercase().contains("rust"),
            "article should mention Rust"
        );
    }
}

// ---------------------------------------------------------------------------
// youtube
// ---------------------------------------------------------------------------
mod live_youtube {
    use super::*;

    #[tokio::test]
    async fn fetches_video_metadata() {
        // "Me at the zoo" — first YouTube video ever, very stable
        let req = FetchRequest::new("https://www.youtube.com/watch?v=jNQXAC9IVRw");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(!content.is_empty());
    }
}

// ---------------------------------------------------------------------------
// arxiv
// ---------------------------------------------------------------------------
mod live_arxiv {
    use super::*;

    #[tokio::test]
    async fn fetches_paper() {
        // "Attention Is All You Need"
        let req = FetchRequest::new("https://arxiv.org/abs/1706.03762");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(
            content.to_lowercase().contains("attention"),
            "paper should mention attention"
        );
    }
}

// ---------------------------------------------------------------------------
// hackernews
// ---------------------------------------------------------------------------
mod live_hackernews {
    use super::*;

    #[tokio::test]
    async fn fetches_story() {
        // HN item 1 — the very first story
        let req = FetchRequest::new("https://news.ycombinator.com/item?id=1");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(!content.is_empty());
    }
}

// ---------------------------------------------------------------------------
// rss_feed
// ---------------------------------------------------------------------------
mod live_rss_feed {
    use super::*;

    #[tokio::test]
    async fn fetches_rss() {
        // Rust blog RSS feed
        let req = FetchRequest::new("https://blog.rust-lang.org/feed.xml");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(
            content.to_lowercase().contains("rust"),
            "Rust blog feed should mention Rust"
        );
    }
}

// ---------------------------------------------------------------------------
// docs_site
// ---------------------------------------------------------------------------
mod live_docs_site {
    use super::*;

    #[tokio::test]
    async fn fetches_docs_rs() {
        let req = FetchRequest::new("https://docs.rs/serde/latest/serde/");
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(
            content.to_lowercase().contains("serde"),
            "docs.rs page should mention serde"
        );
    }
}

// ---------------------------------------------------------------------------
// default (generic HTTP)
// ---------------------------------------------------------------------------
mod live_default {
    use super::*;

    #[tokio::test]
    async fn fetches_plain_html() {
        let req = FetchRequest::new("https://example.com").as_markdown();
        let resp = registry().fetch(req, live_options()).await.unwrap();

        assert_eq!(resp.status_code, 200);
        let content = resp.content.expect("should have content");
        assert!(
            content.contains("Example Domain"),
            "example.com should contain 'Example Domain'"
        );
        assert_eq!(resp.format, Some("markdown".to_string()));
    }
}
