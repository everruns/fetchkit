//! Mira study comparing reader outputs on fixed public web pages.
//! Every enabled target performs real internet HTTP fetches during the run.

use fetchkit::{FetchOptions, FetchRequest, PageMetadata};
use mira::scorer::{scorer, succeeded};
use mira::subject::subject_fn;
use mira::{eval, Eval, Sample, Score, Target, Transcript};
use std::process::Command;
use std::time::{Duration, Instant};

const TARGET_FETCHKIT: &str = "fetchkit";
const TARGET_JINA: &str = "jina-reader";
const TARGET_FIRECRAWL: &str = "firecrawl";
const TARGET_TRAF: &str = "trafilatura-readability";
const TARGET_CRAWL4AI: &str = "crawl4ai";

const TRAFILATURA_CMD_ENV: &str = "FETCHKIT_BENCH_TRAF_CMD";
const CRAWL4AI_CMD_ENV: &str = "FETCHKIT_BENCH_CRAWL4AI_CMD";

#[derive(Clone, Copy)]
struct Fixture {
    id: &'static str,
    url: &'static str,
    required_terms: &'static [&'static str],
    boilerplate_terms: &'static [&'static str],
    expected_links: &'static [&'static str],
    table_markers: &'static [&'static str],
    code_markers: &'static [&'static str],
    metadata_markers: &'static [&'static str],
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        id: "rust-learn",
        url: "https://www.rust-lang.org/learn/",
        required_terms: &[
            "The Rust Programming Language",
            "Rust By Example",
            "Rustlings",
            "Cargo Book",
        ],
        boilerplate_terms: &["Privacy Notice", "Code of Conduct", "Foundation"],
        expected_links: &[
            "https://doc.rust-lang.org/book/",
            "https://doc.rust-lang.org/rust-by-example/",
        ],
        table_markers: &[],
        code_markers: &["rustup doc"],
        metadata_markers: &["Learn Rust"],
    },
    Fixture {
        id: "reqwest-docs",
        url: "https://docs.rs/reqwest/latest/reqwest/",
        required_terms: &["Client", "Response", "RequestBuilder", "redirect"],
        boilerplate_terms: &["Keyboard Shortcuts", "Settings", "Feature flags"],
        expected_links: &["struct.Client.html", "struct.Response.html"],
        table_markers: &[],
        code_markers: &["```rust", "reqwest::get", ".await"],
        metadata_markers: &["reqwest"],
    },
    Fixture {
        id: "wiki-rust",
        url: "https://en.wikipedia.org/wiki/Rust_(programming_language)",
        required_terms: &[
            "Rust is a multi-paradigm",
            "Graydon Hoare",
            "Mozilla Research",
            "memory safety",
        ],
        boilerplate_terms: &["Contents", "Appearance", "Donate"],
        expected_links: &["/wiki/Graydon_Hoare", "/wiki/Mozilla"],
        table_markers: &["Designed by", "First appeared", "Stable release"],
        code_markers: &[],
        metadata_markers: &["Rust (programming language)"],
    },
    Fixture {
        id: "github-fetchkit",
        url: "https://github.com/everruns/fetchkit",
        required_terms: &[
            "fetchkit",
            "AI-friendly web content fetching",
            "MCP server",
            "Rust library",
        ],
        boilerplate_terms: &["Sign in", "Notifications", "Fork"],
        expected_links: &[
            "/everruns/fetchkit/tree/main/crates",
            "/everruns/fetchkit/blob/main/README.md",
        ],
        table_markers: &[],
        code_markers: &["cargo install", "fetchkit fetch"],
        metadata_markers: &["everruns/fetchkit"],
    },
    Fixture {
        id: "stackoverflow-python-main",
        url: "https://stackoverflow.com/questions/419163/what-does-if-name-main-do",
        required_terms: &[
            "if __name__ == \"__main__\"",
            "__main__",
            "module",
            "import",
        ],
        boilerplate_terms: &["Teams", "Collectives", "Hot Network Questions"],
        expected_links: &["/questions/419163", "/users/"],
        table_markers: &[],
        code_markers: &["```python", "__name__"],
        metadata_markers: &["What does if __name__ == \"__main__\" do?"],
    },
];

#[eval]
fn reader_comparison() -> Eval {
    let mut builder = Eval::new("reader-comparison")
        .describe("Fetches fixed public web pages with FetchKit and configured reader tools")
        .targets([
            Target::cli(TARGET_FETCHKIT).meta("fetch_mode", "public_http"),
            Target::cli(TARGET_JINA).meta("fetch_mode", "public_http_jina_reader"),
            Target::cli(TARGET_FIRECRAWL)
                .available(env_present("FIRECRAWL_API_KEY"))
                .meta("requires", "FIRECRAWL_API_KEY"),
            Target::cli(TARGET_TRAF)
                .available(trafilatura_available())
                .meta("fetch_mode", "public_http_cli"),
            Target::cli(TARGET_CRAWL4AI)
                .available(crawl4ai_available())
                .meta("fetch_mode", "public_http_cli"),
        ])
        .subject(subject_fn(|sample, cx| async move {
            run_subject(&sample, &cx.target.label).await
        }))
        .scorer(succeeded())
        .scorer(metric_report_scorer("link_preservation"))
        .scorer(metric_report_scorer("main_content_precision"))
        .scorer(metric_report_scorer("table_retention"))
        .scorer(metric_report_scorer("code_retention"))
        .scorer(metric_report_scorer("metadata_score"))
        .scorer(token_count_scorer());

    for fixture in FIXTURES {
        builder = builder.add_sample(sample_for(*fixture));
    }

    builder.build()
}

fn sample_for(fixture: Fixture) -> Sample {
    Sample::new(fixture.id, fixture.url)
        .meta("url", fixture.url)
        .tag("reader")
}

async fn run_subject(sample: &Sample, target: &str) -> Transcript {
    let Some(fixture) = fixture_by_id(&sample.id) else {
        return Transcript::failed(format!("unknown fixture {}", sample.id));
    };

    let started = Instant::now();
    let output = match fetch_output(target, fixture).await {
        Ok(output) => output,
        Err(err) => return Transcript::infra_error(err),
    };

    let duration_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    let scores = fixture_scores(&output, fixture);
    let tokens = token_count(&output);

    let mut transcript = Transcript::response(output).with_duration_ms(duration_ms);
    transcript.usage.output_tokens = tokens.try_into().unwrap_or(u64::MAX);
    transcript.record_metric("token_count", tokens as f64);
    transcript.record_metric("link_preservation", scores.link_preservation);
    transcript.record_metric("main_content_precision", scores.main_content_precision);
    transcript.record_metric("table_retention", scores.table_retention);
    transcript.record_metric("code_retention", scores.code_retention);
    transcript.record_metric("metadata_score", scores.metadata_score);
    transcript
}

async fn fetch_output(target: &str, fixture: Fixture) -> Result<String, String> {
    match target {
        TARGET_FETCHKIT => fetchkit_output(fixture).await,
        TARGET_JINA => jina_output(fixture).await,
        TARGET_FIRECRAWL => firecrawl_output(fixture).await,
        TARGET_TRAF => trafilatura_output(fixture),
        TARGET_CRAWL4AI => crawl4ai_output(fixture),
        other => Err(format!("unknown target {other}")),
    }
}

async fn fetchkit_output(fixture: Fixture) -> Result<String, String> {
    let request = FetchRequest::new(fixture.url)
        .as_markdown()
        .content_focus("main");
    let options = FetchOptions {
        enable_markdown: true,
        user_agent: Some("FetchKit reader benchmark".to_string()),
        ..Default::default()
    };

    let response = fetchkit::fetch_with_options(request, options)
        .await
        .map_err(|err| format!("fetchkit failed fetching {}: {err}", fixture.url))?;
    let mut out = String::new();
    out.push_str("---\n");
    push_frontmatter(&mut out, "url", &response.url);
    push_metadata_frontmatter(&mut out, response.metadata.as_ref());
    out.push_str("---\n\n");
    out.push_str(response.content.as_deref().unwrap_or_default());
    Ok(out)
}

async fn jina_output(fixture: Fixture) -> Result<String, String> {
    let reader_url = format!("https://r.jina.ai/http://{}", fixture.url);
    http_get_text(&reader_url).await
}

async fn firecrawl_output(fixture: Fixture) -> Result<String, String> {
    let api_key =
        std::env::var("FIRECRAWL_API_KEY").map_err(|_| "FIRECRAWL_API_KEY is required")?;
    let client = reqwest::Client::new();
    let response = client
        .post("https://api.firecrawl.dev/v1/scrape")
        .bearer_auth(api_key)
        .json(&serde_json::json!({
            "url": fixture.url,
            "formats": ["markdown"],
            "onlyMainContent": true
        }))
        .send()
        .await
        .map_err(|err| format!("firecrawl request failed: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("firecrawl body read failed: {err}"))?;
    if !status.is_success() {
        return Err(format!("firecrawl returned {status}: {body}"));
    }
    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|err| format!("firecrawl JSON parse failed: {err}"))?;
    json.pointer("/data/markdown")
        .or_else(|| json.pointer("/markdown"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "firecrawl response did not include markdown".to_string())
}

fn trafilatura_output(fixture: Fixture) -> Result<String, String> {
    if let Ok(template) = std::env::var(TRAFILATURA_CMD_ENV) {
        return run_command_template(&template, fixture.url);
    }
    run_command("trafilatura", &["-u", fixture.url, "--markdown"])
}

fn crawl4ai_output(fixture: Fixture) -> Result<String, String> {
    if let Ok(template) = std::env::var(CRAWL4AI_CMD_ENV) {
        return run_command_template(&template, fixture.url);
    }
    run_command("crawl4ai", &[fixture.url])
}

async fn http_get_text(url: &str) -> Result<String, String> {
    let response = reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(45))
        .send()
        .await
        .map_err(|err| format!("GET {url} failed: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("GET {url} body read failed: {err}"))?;
    if status.is_success() {
        Ok(body)
    } else {
        Err(format!("GET {url} returned {status}: {body}"))
    }
}

fn push_frontmatter(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

fn push_metadata_frontmatter(out: &mut String, metadata: Option<&PageMetadata>) {
    let Some(metadata) = metadata else {
        return;
    };
    if let Some(title) = metadata.title.as_deref() {
        push_frontmatter(out, "title", title);
    }
    if let Some(author) = metadata.author.as_deref() {
        push_frontmatter(out, "author", author);
    }
    if let Some(date) = metadata.published_date.as_deref() {
        push_frontmatter(out, "published", date);
    }
}

fn fixture_by_id(id: &str) -> Option<Fixture> {
    FIXTURES.iter().copied().find(|fixture| fixture.id == id)
}

#[derive(Default)]
struct FixtureScores {
    link_preservation: f64,
    main_content_precision: f64,
    table_retention: f64,
    code_retention: f64,
    metadata_score: f64,
}

fn fixture_scores(output: &str, fixture: Fixture) -> FixtureScores {
    FixtureScores {
        link_preservation: hit_rate(output, fixture.expected_links),
        main_content_precision: main_content_precision(output, fixture),
        table_retention: hit_rate(output, fixture.table_markers),
        code_retention: hit_rate(output, fixture.code_markers),
        metadata_score: hit_rate(output, fixture.metadata_markers),
    }
}

fn hit_rate(output: &str, markers: &[&str]) -> f64 {
    if markers.is_empty() {
        return 1.0;
    }

    let output = output.to_lowercase();
    let hits = markers
        .iter()
        .filter(|marker| output.contains(&marker.to_lowercase()))
        .count();
    hits as f64 / markers.len() as f64
}

fn main_content_precision(output: &str, fixture: Fixture) -> f64 {
    let output = output.to_lowercase();
    let required_hits = fixture
        .required_terms
        .iter()
        .filter(|term| output.contains(&term.to_lowercase()))
        .count();
    let boilerplate_hits = fixture
        .boilerplate_terms
        .iter()
        .filter(|term| output.contains(&term.to_lowercase()))
        .count();

    if required_hits == 0 {
        return 0.0;
    }

    required_hits as f64 / (required_hits + boilerplate_hits) as f64
}

fn token_count(output: &str) -> usize {
    output.split_whitespace().count()
}

fn token_count_scorer() -> Box<dyn mira::scorer::Scorer> {
    scorer("token_count", |_, transcript| {
        Score::pass(
            "token_count",
            format!("{} tokens", transcript.usage.output_tokens),
        )
    })
}

fn metric_report_scorer(metric: &'static str) -> Box<dyn mira::scorer::Scorer> {
    scorer(metric, move |_, transcript| {
        match transcript.metric(metric) {
            Some(value) => Score::graded(metric, value, 0.0, format!("{metric}={value:.2}")),
            None => Score::fail(metric, "metric missing"),
        }
    })
}

fn trafilatura_available() -> bool {
    std::env::var(TRAFILATURA_CMD_ENV).is_ok() || command_available("trafilatura")
}

fn crawl4ai_available() -> bool {
    std::env::var(CRAWL4AI_CMD_ENV).is_ok() || command_available("crawl4ai")
}

fn env_present(name: &str) -> bool {
    std::env::var(name)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn command_available(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run_command(binary: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(|err| format!("{binary} failed to start: {err}"))?;
    command_output(binary, output)
}

fn run_command_template(template: &str, url: &str) -> Result<String, String> {
    let command = template.replace("{url}", &shell_quote(url));
    let output = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .map_err(|err| format!("command template failed to start: {err}"))?;
    command_output(&command, output)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn command_output(command: &str, output: std::process::Output) -> Result<String, String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(format!(
            "{command} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    mira::Study::registered().serve().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_are_public_urls() {
        for fixture in FIXTURES {
            assert!(fixture.url.starts_with("https://"));
            assert!(!fixture.required_terms.is_empty());
        }
    }
}
