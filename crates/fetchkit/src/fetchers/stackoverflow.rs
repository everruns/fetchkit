//! Stack Overflow Q&A fetcher
//!
//! Handles stackoverflow.com/questions/{id} URLs, returning structured
//! Q&A content stripped of noise via the Stack Exchange API.

use crate::client::FetchOptions;
use crate::convert::{html_to_markdown_with_base_url, html_to_text};
use crate::error::FetchError;
use crate::fetchers::default::{read_full_body, transport_request};
use crate::fetchers::Fetcher;
use crate::types::{FetchRequest, FetchResponse};
use crate::DEFAULT_USER_AGENT;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use std::time::Duration;
use url::Url;

const API_TIMEOUT: Duration = Duration::from_secs(10);
const STACKEXCHANGE_API_HOST: &str = "api.stackexchange.com";
const STACKEXCHANGE_API_PORT: u16 = 443;

/// Max answers to include
const MAX_ANSWERS: usize = 10;

/// Stack Overflow fetcher
///
/// Matches `https://stackoverflow.com/questions/{id}` and Stack Exchange
/// network sites, returning structured Q&A via the API.
pub struct StackOverflowFetcher;

impl StackOverflowFetcher {
    pub fn new() -> Self {
        Self
    }

    /// Extract site and question ID from a Stack Exchange URL
    fn parse_url(url: &Url) -> Option<(String, u64)> {
        let host = url.host_str()?;

        // Determine API site parameter from hostname
        let site = match host {
            "stackoverflow.com" | "www.stackoverflow.com" => "stackoverflow",
            "serverfault.com" | "www.serverfault.com" => "serverfault",
            "superuser.com" | "www.superuser.com" => "superuser",
            "askubuntu.com" | "www.askubuntu.com" => "askubuntu",
            "mathoverflow.net" | "www.mathoverflow.net" => "mathoverflow",
            _ if host.ends_with(".stackexchange.com") => {
                // e.g. "unix.stackexchange.com" -> site = "unix"
                // We'll pass the full subdomain as site parameter
                host.strip_suffix(".stackexchange.com")?
            }
            _ => return None,
        };

        let segments: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();

        // Must start with /questions/{id}
        if segments.len() < 2 || segments[0] != "questions" {
            return None;
        }

        let id: u64 = segments[1].parse().ok()?;

        Some((site.to_string(), id))
    }
}

impl Default for StackOverflowFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// --- Stack Exchange API types ---

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    items: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct Question {
    title: String,
    body: Option<String>,
    score: i64,
    view_count: Option<u64>,
    answer_count: u64,
    tags: Vec<String>,
    owner: Option<Owner>,
    link: String,
    is_answered: bool,
}

#[derive(Debug, Deserialize)]
struct Answer {
    body: Option<String>,
    score: i64,
    is_accepted: bool,
    owner: Option<Owner>,
}

#[derive(Debug, Deserialize)]
struct Owner {
    display_name: Option<String>,
    reputation: Option<u64>,
}

#[async_trait]
impl Fetcher for StackOverflowFetcher {
    fn name(&self) -> &'static str {
        "stackoverflow"
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

        let (site, question_id) = Self::parse_url(&url).ok_or_else(|| {
            FetchError::FetcherError("Not a valid Stack Exchange question URL".to_string())
        })?;

        let user_agent = options.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT);
        let ua_header = HeaderValue::from_str(user_agent)
            .unwrap_or_else(|_| HeaderValue::from_static(DEFAULT_USER_AGENT));

        // THREAT[TM-SSRF-010]: single-hop transport request, redirects not followed.
        // THREAT[TM-SSRF-005]: DNS pinned to the Stack Exchange API host per request.
        let api_get = |url: String| {
            let ua = ua_header.clone();
            async move {
                let parsed = Url::parse(&url).map_err(|_| FetchError::InvalidUrlScheme)?;
                let mut headers = HeaderMap::new();
                headers.insert(USER_AGENT, ua);
                transport_request(
                    parsed,
                    reqwest::Method::GET,
                    headers,
                    options,
                    API_TIMEOUT,
                    STACKEXCHANGE_API_HOST,
                    STACKEXCHANGE_API_PORT,
                )
                .await
            }
        };

        // Fetch question with body HTML. The Stack Exchange API's built-in
        // `withbody` filter is stable; FetchKit converts the HTML locally.
        let question_url = format!(
            "https://api.stackexchange.com/2.3/questions/{}?site={}&filter=withbody",
            question_id, site
        );

        let q_response = api_get(question_url).await?;

        let status_code = q_response.status;
        if !(200..300).contains(&status_code) {
            let error_msg = if status_code == 404 {
                format!("Question {} not found on {}", question_id, site)
            } else {
                format!("Stack Exchange API error: HTTP {}", status_code)
            };
            return Ok(FetchResponse {
                url: request.url.clone(),
                status_code,
                error: Some(error_msg),
                ..Default::default()
            });
        }

        let q_body = read_full_body(q_response, options).await?;
        let q_data: ApiResponse<Question> = serde_json::from_slice(&q_body).map_err(|e| {
            FetchError::FetcherError(format!("Failed to parse question data: {}", e))
        })?;

        let question = q_data.items.into_iter().next().ok_or_else(|| {
            FetchError::FetcherError(format!("Question {} not found", question_id))
        })?;

        // Fetch answers sorted by votes, with body HTML.
        let answers = if question.answer_count > 0 {
            let answers_url = format!(
                "https://api.stackexchange.com/2.3/questions/{}/answers?site={}&sort=votes&order=desc&pagesize={}&filter=withbody",
                question_id, site, MAX_ANSWERS
            );

            match api_get(answers_url).await {
                Ok(resp) if (200..300).contains(&resp.status) => {
                    match read_full_body(resp, options).await {
                        Ok(body) => serde_json::from_slice::<ApiResponse<Answer>>(&body)
                            .ok()
                            .map(|r| r.items),
                        Err(_) => None,
                    }
                }
                _ => None,
            }
        } else {
            None
        };

        let content = format_qa_response(&question, answers.as_deref());
        if is_minimal_qa_content(&question, answers.as_deref(), &content) {
            return Ok(FetchResponse {
                url: request.url.clone(),
                status_code: 502,
                error: Some("Stack Exchange API returned minimal or empty Q&A content".to_string()),
                ..Default::default()
            });
        }

        Ok(FetchResponse {
            url: request.url.clone(),
            status_code: 200,
            content_type: Some("text/markdown".to_string()),
            format: Some("stackoverflow_qa".to_string()),
            content: Some(content),
            ..Default::default()
        })
    }
}

fn format_qa_response(question: &Question, answers: Option<&[Answer]>) -> String {
    let mut out = String::new();

    // Title
    out.push_str(&format!("# {}\n\n", html_to_text(&question.title)));

    // Question metadata
    out.push_str("## Question\n\n");
    out.push_str(&format!("- **Score:** {}\n", question.score));

    if let Some(views) = question.view_count {
        out.push_str(&format!("- **Views:** {}\n", views));
    }

    out.push_str(&format!("- **Answers:** {}\n", question.answer_count));
    out.push_str(&format!(
        "- **Answered:** {}\n",
        if question.is_answered { "yes" } else { "no" }
    ));

    if !question.tags.is_empty() {
        out.push_str(&format!("- **Tags:** {}\n", question.tags.join(", ")));
    }

    if let Some(owner) = &question.owner {
        if let Some(name) = &owner.display_name {
            let rep = owner
                .reputation
                .map(|r| format!(" ({})", r))
                .unwrap_or_default();
            out.push_str(&format!("- **Asked by:** {}{}\n", name, rep));
        }
    }

    out.push_str(&format!("- **URL:** {}\n", question.link));

    // Question body
    if let Some(body) = markdown_body(question.body.as_deref(), &question.link) {
        out.push_str(&format!("\n{}\n", body));
    }

    // Answers
    if let Some(answers) = answers {
        if !answers.is_empty() {
            out.push_str(&format!("\n---\n\n## Answers ({})\n", answers.len()));

            for answer in answers {
                let accepted = if answer.is_accepted {
                    " ✓ Accepted"
                } else {
                    ""
                };
                let author = answer
                    .owner
                    .as_ref()
                    .and_then(|o| o.display_name.as_deref())
                    .unwrap_or("anonymous");

                out.push_str(&format!(
                    "\n### Score: {}{} — by {}\n\n",
                    answer.score, accepted, author
                ));

                if let Some(body) = markdown_body(answer.body.as_deref(), &question.link) {
                    out.push_str(&body);
                    out.push('\n');
                }
            }
        }
    }

    out
}

fn markdown_body(body_html: Option<&str>, base_url: &str) -> Option<String> {
    let body = body_html?;
    let markdown = html_to_markdown_with_base_url(body, base_url);
    let markdown = markdown.trim();
    (!markdown.is_empty()).then(|| markdown.to_string())
}

fn is_minimal_qa_content(question: &Question, answers: Option<&[Answer]>, content: &str) -> bool {
    let has_question_body = markdown_body(question.body.as_deref(), &question.link).is_some();
    let has_answer_body = answers
        .unwrap_or(&[])
        .iter()
        .any(|answer| markdown_body(answer.body.as_deref(), &question.link).is_some());

    !has_question_body && !has_answer_body || content.split_whitespace().count() < 20
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns::DnsPolicy;
    use crate::transport::{
        HttpTransport, TransportError, TransportMethod, TransportRequest, TransportResponse,
    };
    use async_trait::async_trait;
    use bytes::Bytes;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct Canned {
        status: u16,
        body: Vec<u8>,
    }

    struct MockTransport {
        calls: Mutex<Vec<(String, TransportMethod)>>,
        routes: Vec<(String, Canned)>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                routes: Vec::new(),
            }
        }

        fn route(mut self, contains: &str, status: u16, body: &[u8]) -> Self {
            self.routes.push((
                contains.to_string(),
                Canned {
                    status,
                    body: body.to_vec(),
                },
            ));
            self
        }

        fn calls(&self) -> Vec<(String, TransportMethod)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl HttpTransport for MockTransport {
        async fn execute(
            &self,
            req: TransportRequest,
        ) -> Result<TransportResponse, TransportError> {
            self.calls
                .lock()
                .unwrap()
                .push((req.url.to_string(), req.method));

            let canned = self
                .routes
                .iter()
                .find(|(needle, _)| req.url.as_str().contains(needle.as_str()))
                .map(|(_, canned)| canned.clone())
                .unwrap_or(Canned {
                    status: 404,
                    body: b"not found".to_vec(),
                });

            let body = Bytes::from(canned.body);
            let stream = futures::stream::once(async move { Ok(body) });

            Ok(TransportResponse {
                status: canned.status,
                url: req.url,
                headers: vec![("content-type".to_string(), "application/json".to_string())],
                body: Box::pin(stream),
            })
        }
    }

    fn options_with(transport: Arc<dyn HttpTransport>) -> FetchOptions {
        FetchOptions {
            enable_markdown: true,
            enable_text: true,
            dns_policy: DnsPolicy::allow_all(),
            transport: Some(transport),
            ..Default::default()
        }
    }

    #[test]
    fn test_parse_stackoverflow_url() {
        let url = Url::parse("https://stackoverflow.com/questions/12345/how-to-do-x").unwrap();
        assert_eq!(
            StackOverflowFetcher::parse_url(&url),
            Some(("stackoverflow".to_string(), 12345))
        );
    }

    #[test]
    fn test_parse_stackoverflow_url_no_slug() {
        let url = Url::parse("https://stackoverflow.com/questions/12345").unwrap();
        assert_eq!(
            StackOverflowFetcher::parse_url(&url),
            Some(("stackoverflow".to_string(), 12345))
        );
    }

    #[test]
    fn test_parse_stackexchange_url() {
        let url =
            Url::parse("https://unix.stackexchange.com/questions/999/shell-question").unwrap();
        assert_eq!(
            StackOverflowFetcher::parse_url(&url),
            Some(("unix".to_string(), 999))
        );
    }

    #[test]
    fn test_parse_other_se_sites() {
        let url = Url::parse("https://serverfault.com/questions/42/title").unwrap();
        assert_eq!(
            StackOverflowFetcher::parse_url(&url),
            Some(("serverfault".to_string(), 42))
        );

        let url = Url::parse("https://askubuntu.com/questions/1/title").unwrap();
        assert_eq!(
            StackOverflowFetcher::parse_url(&url),
            Some(("askubuntu".to_string(), 1))
        );
    }

    #[test]
    fn test_rejects_non_question_paths() {
        let url = Url::parse("https://stackoverflow.com/users/12345").unwrap();
        assert_eq!(StackOverflowFetcher::parse_url(&url), None);

        let url = Url::parse("https://stackoverflow.com/tags").unwrap();
        assert_eq!(StackOverflowFetcher::parse_url(&url), None);
    }

    #[test]
    fn test_rejects_non_numeric_id() {
        let url = Url::parse("https://stackoverflow.com/questions/abc/title").unwrap();
        assert_eq!(StackOverflowFetcher::parse_url(&url), None);
    }

    #[test]
    fn test_rejects_non_se_sites() {
        let url = Url::parse("https://example.com/questions/123").unwrap();
        assert_eq!(StackOverflowFetcher::parse_url(&url), None);
    }

    #[test]
    fn test_fetcher_matches() {
        let fetcher = StackOverflowFetcher::new();

        let url = Url::parse("https://stackoverflow.com/questions/12345/title").unwrap();
        assert!(fetcher.matches(&url));

        let url = Url::parse("https://unix.stackexchange.com/questions/1/title").unwrap();
        assert!(fetcher.matches(&url));

        let url = Url::parse("https://example.com/questions/1").unwrap();
        assert!(!fetcher.matches(&url));
    }

    #[test]
    fn test_format_qa_response() {
        let question = Question {
            title: "How to parse JSON in Rust?".to_string(),
            body: Some("<p>I need to parse JSON.</p>".to_string()),
            score: 15,
            view_count: Some(1000),
            answer_count: 2,
            tags: vec!["rust".to_string(), "json".to_string()],
            owner: Some(Owner {
                display_name: Some("alice".to_string()),
                reputation: Some(5000),
            }),
            link: "https://stackoverflow.com/questions/42".to_string(),
            is_answered: true,
        };

        let answers = vec![
            Answer {
                body: Some("<p>Use serde_json crate.</p>".to_string()),
                score: 20,
                is_accepted: true,
                owner: Some(Owner {
                    display_name: Some("bob".to_string()),
                    reputation: Some(10000),
                }),
            },
            Answer {
                body: Some("<p>Try simd-json for speed.</p>".to_string()),
                score: 5,
                is_accepted: false,
                owner: Some(Owner {
                    display_name: Some("charlie".to_string()),
                    reputation: None,
                }),
            },
        ];

        let output = format_qa_response(&question, Some(&answers));

        assert!(output.contains("# How to parse JSON in Rust?"));
        assert!(output.contains("**Score:** 15"));
        assert!(output.contains("**Views:** 1000"));
        assert!(output.contains("**Tags:** rust, json"));
        assert!(output.contains("**Asked by:** alice (5000)"));
        assert!(output.contains("I need to parse JSON."));
        assert!(output.contains("Accepted"));
        assert!(output.contains("Use serde_json crate."));
        assert!(output.contains("Try simd-json for speed."));
    }

    #[tokio::test]
    async fn test_fetch_stackoverflow_question_preserves_question_answers_and_code() {
        let question_json = br#"{
            "items": [{
                "tags": ["python", "namespaces"],
                "owner": {"reputation": 184993, "display_name": "Devoted"},
                "is_answered": true,
                "view_count": 5030222,
                "answer_count": 40,
                "score": 8438,
                "link": "https://stackoverflow.com/questions/419163/what-does-if-name-main-do",
                "title": "What does if __name__ == &quot;__main__&quot;: do?",
                "body": "<p>What does this do, and why should one include the <code>if</code> statement?</p>\n<pre class=\"lang-py prettyprint-override\"><code>if __name__ == &quot;__main__&quot;:\n    print(&quot;Hello, World!&quot;)\n</code></pre>"
            }]
        }"#;
        let answers_json = br#"{
            "items": [{
                "owner": {"reputation": 200000, "display_name": "bobince"},
                "is_accepted": true,
                "score": 5630,
                "body": "<p>When the Python interpreter reads a source file, it sets <code>__name__</code>.</p>\n<pre class=\"lang-py\"><code>if __name__ == \"__main__\":\n    main()\n</code></pre>"
            }]
        }"#;
        let mock = Arc::new(
            MockTransport::new()
                .route(
                    "/2.3/questions/419163?site=stackoverflow&filter=withbody",
                    200,
                    question_json,
                )
                .route(
                    "/2.3/questions/419163/answers?site=stackoverflow",
                    200,
                    answers_json,
                ),
        );
        let options = options_with(mock.clone());
        let fetcher = StackOverflowFetcher::new();
        let request = FetchRequest::new(
            "https://stackoverflow.com/questions/419163/what-does-if-name-main-do",
        )
        .as_markdown();

        let response = fetcher.fetch(&request, &options).await.unwrap();
        let content = response.content.as_deref().unwrap();

        assert_eq!(response.status_code, 200);
        assert_eq!(response.format.as_deref(), Some("stackoverflow_qa"));
        assert!(content.contains("What does if __name__ == \"__main__\": do?"));
        assert!(content.contains("What does this do"));
        assert!(content.contains("## Answers (1)"));
        assert!(content.contains("✓ Accepted"));
        assert!(content.contains("__main__"));
        assert!(content.contains("```"));
        assert!(content.contains("if __name__ == \"__main__\":"));
        assert!(content.contains("main()"));

        let calls = mock.calls();
        assert_eq!(calls.len(), 2);
        assert!(calls
            .iter()
            .all(|(_, method)| *method == TransportMethod::Get));
        assert!(calls.iter().all(|(url, _)| url.contains("filter=withbody")));
        assert!(calls
            .iter()
            .all(|(url, _)| !url.contains("withbody_markdown")));
    }

    #[test]
    fn test_minimal_stackoverflow_content_is_not_successful() {
        let question = Question {
            title: "Blocked".to_string(),
            body: None,
            score: 0,
            view_count: None,
            answer_count: 0,
            tags: vec![],
            owner: None,
            link: "https://stackoverflow.com/questions/1".to_string(),
            is_answered: false,
        };
        let content = format_qa_response(&question, None);

        assert!(is_minimal_qa_content(&question, None, &content));
    }
}
