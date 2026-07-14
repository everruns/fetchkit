//! PubMed and PubMed Central fetcher.

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
use url::Url;

const API_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_BODY_SIZE: usize = 5 * 1024 * 1024;

pub struct PubMedFetcher;

#[derive(Clone, Debug, PartialEq)]
enum ArticleId {
    PubMed(String),
    Pmc(String),
}

impl PubMedFetcher {
    pub fn new() -> Self {
        Self
    }

    fn parse_url(url: &Url) -> Option<ArticleId> {
        let segments: Vec<&str> = url
            .path_segments()?
            .filter(|segment| !segment.is_empty())
            .collect();
        match url.host_str()? {
            "pubmed.ncbi.nlm.nih.gov" if segments.len() == 1 => segments[0]
                .chars()
                .all(|character| character.is_ascii_digit())
                .then(|| ArticleId::PubMed(segments[0].into())),
            "www.ncbi.nlm.nih.gov" | "ncbi.nlm.nih.gov"
                if segments.len() == 2 && segments[0].eq_ignore_ascii_case("pmc") =>
            {
                normalize_pmcid(segments[1]).map(ArticleId::Pmc)
            }
            "pmc.ncbi.nlm.nih.gov" if segments.len() == 2 && segments[0] == "articles" => {
                normalize_pmcid(segments[1]).map(ArticleId::Pmc)
            }
            _ => None,
        }
    }
}

impl Default for PubMedFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetcher for PubMedFetcher {
    fn name(&self) -> &'static str {
        "pubmed"
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
        let article = Self::parse_url(&page_url)
            .ok_or_else(|| FetchError::FetcherError("Not a supported PubMed URL".into()))?;
        let (api_url, host, format) = api_url(&article)?;
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
            host,
            443,
        )
        .await?;
        let status_code = response.status;
        if !(200..300).contains(&status_code) {
            return Ok(FetchResponse {
                url: request.url,
                status_code,
                error: Some(match status_code {
                    404 => "Article not found".into(),
                    403 | 429 => "Literature API rate limit exceeded or access denied".into(),
                    _ => format!("Literature API error: HTTP {status_code}"),
                }),
                ..Default::default()
            });
        }
        let body = read_full_body(response, options).await?;
        let value: Value = serde_json::from_slice(&body).map_err(|error| {
            FetchError::FetcherError(format!("Failed to parse literature response: {error}"))
        })?;
        let content = match article {
            ArticleId::PubMed(ref id) => render_pubmed(id, &value)?,
            ArticleId::Pmc(ref id) => render_pmc(id, &value)?,
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

fn normalize_pmcid(value: &str) -> Option<String> {
    let value = value.strip_suffix('/').unwrap_or(value);
    let digits = value
        .strip_prefix("PMC")
        .or_else(|| value.strip_prefix("pmc"))?;
    (!digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit()))
        .then(|| format!("PMC{digits}"))
}

fn api_url(article: &ArticleId) -> Result<(Url, &'static str, &'static str), FetchError> {
    let (url, host, format) = match article {
        ArticleId::PubMed(id) => (
            format!("https://www.ebi.ac.uk/europepmc/webservices/rest/search?query=EXT_ID:{id}%20AND%20SRC:MED&resultType=core&format=json"),
            "www.ebi.ac.uk",
            "pubmed_article",
        ),
        ArticleId::Pmc(id) => (
            format!("https://www.ncbi.nlm.nih.gov/research/bionlp/RESTful/pmcoa.cgi/BioC_json/{id}/unicode"),
            "www.ncbi.nlm.nih.gov",
            "pmc_article",
        ),
    };
    Ok((
        Url::parse(&url).map_err(|_| FetchError::InvalidUrlScheme)?,
        host,
        format,
    ))
}

fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)?
        .as_str()
        .filter(|value| !value.trim().is_empty())
}

fn render_pubmed(id: &str, response: &Value) -> Result<String, FetchError> {
    let article = response
        .pointer("/resultList/result/0")
        .ok_or_else(|| FetchError::FetcherError(format!("PubMed article {id} not found")))?;
    let title = string(article, "title").unwrap_or("Untitled article");
    let mut out = format!("# {title}\n\n- **PMID:** {id}\n");
    field(&mut out, "Journal", string(article, "journalTitle"));
    field(
        &mut out,
        "Published",
        string(article, "firstPublicationDate"),
    );
    field(&mut out, "DOI", string(article, "doi"));
    field(&mut out, "PMCID", string(article, "pmcid"));
    field(&mut out, "Publication type", string(article, "pubType"));
    if let Some(authors) = article.get("authorList").and_then(Value::as_array) {
        let names: Vec<&str> = authors
            .iter()
            .filter_map(|author| string(author, "fullName"))
            .collect();
        if !names.is_empty() {
            out.push_str(&format!("- **Authors:** {}\n", names.join(", ")));
        }
    }
    if let Some(abstract_text) = string(article, "abstractText") {
        out.push_str(&format!("\n## Abstract\n\n{abstract_text}\n"));
    }
    if let Some(keywords) = article.get("keywordList").and_then(Value::as_array) {
        let keywords: Vec<&str> = keywords.iter().filter_map(Value::as_str).collect();
        if !keywords.is_empty() {
            out.push_str(&format!("\n## Keywords\n\n{}\n", keywords.join(", ")));
        }
    }
    Ok(out)
}

fn render_pmc(id: &str, response: &Value) -> Result<String, FetchError> {
    let document = response
        .as_array()
        .and_then(|collections| collections.first())
        .and_then(|collection| collection.get("documents"))
        .and_then(Value::as_array)
        .and_then(|documents| documents.first())
        .ok_or_else(|| FetchError::FetcherError(format!("PMC article {id} not found")))?;
    let passages = document
        .get("passages")
        .and_then(Value::as_array)
        .ok_or_else(|| FetchError::FetcherError("PMC article has no passages".into()))?;
    let title = passages
        .iter()
        .find(|passage| passage.pointer("/infons/type").and_then(Value::as_str) == Some("title"))
        .and_then(|passage| string(passage, "text"))
        .unwrap_or("PMC article");
    let mut out = format!("# {title}\n\n- **PMCID:** {id}\n");
    let mut current_section = String::new();
    for passage in passages {
        let Some(text) = string(passage, "text") else {
            continue;
        };
        let kind = passage
            .pointer("/infons/type")
            .and_then(Value::as_str)
            .unwrap_or("");
        if kind == "title" {
            continue;
        }
        let section = passage
            .pointer("/infons/section")
            .and_then(Value::as_str)
            .or_else(|| {
                passage
                    .pointer("/infons/section_type")
                    .and_then(Value::as_str)
            });
        if let Some(section) = section.filter(|section| !section.is_empty()) {
            if !section.eq_ignore_ascii_case(&current_section) {
                current_section = section.into();
                out.push_str(&format!("\n## {}\n\n", heading(section)));
            }
        } else if kind == "abstract" && current_section != "Abstract" {
            current_section = "Abstract".into();
            out.push_str("\n## Abstract\n\n");
        }
        out.push_str(text);
        out.push_str("\n\n");
    }
    Ok(out)
}

fn heading(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

fn field(out: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value {
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
    fn parses_pubmed_and_pmc_urls() {
        assert_eq!(
            PubMedFetcher::parse_url(
                &Url::parse("https://pubmed.ncbi.nlm.nih.gov/12345678/").unwrap()
            ),
            Some(ArticleId::PubMed("12345678".into()))
        );
        for input in [
            "https://pmc.ncbi.nlm.nih.gov/articles/PMC1234567/",
            "https://www.ncbi.nlm.nih.gov/pmc/PMC1234567/",
        ] {
            assert_eq!(
                PubMedFetcher::parse_url(&Url::parse(input).unwrap()),
                Some(ArticleId::Pmc("PMC1234567".into()))
            );
        }
    }

    #[test]
    fn rejects_search_pages_and_invalid_identifiers() {
        for input in [
            "https://pubmed.ncbi.nlm.nih.gov/?term=rust",
            "https://pubmed.ncbi.nlm.nih.gov/not-a-pmid/",
            "https://pmc.ncbi.nlm.nih.gov/articles/nope/",
            "https://example.com/1234",
        ] {
            assert!(PubMedFetcher::parse_url(&Url::parse(input).unwrap()).is_none());
        }
    }

    #[test]
    fn renders_pubmed_metadata_abstract_and_keywords() {
        let response = serde_json::json!({"resultList":{"result":[{
            "title":"A useful study", "journalTitle":"Science", "firstPublicationDate":"2025-01-02",
            "doi":"10.1/test", "pmcid":"PMC9", "pubType":"Journal Article",
            "authorList":[{"fullName":"Ada Lovelace"},{"fullName":"Alan Turing"}],
            "abstractText":"The findings.", "keywordList":["agents","fetching"]
        }]}});
        let output = render_pubmed("42", &response).unwrap();
        assert!(output.contains("# A useful study"));
        assert!(output.contains("- **Authors:** Ada Lovelace, Alan Turing"));
        assert!(output.contains("## Abstract\n\nThe findings."));
        assert!(output.contains("## Keywords\n\nagents, fetching"));
    }

    #[test]
    fn renders_pmc_sections_in_document_order() {
        let response = serde_json::json!([{"documents":[{"passages":[
            {"infons":{"type":"title"},"text":"Full article"},
            {"infons":{"type":"abstract"},"text":"Summary"},
            {"infons":{"type":"paragraph","section":"methods"},"text":"Procedure"},
            {"infons":{"type":"paragraph","section":"results"},"text":"Findings"}
        ]}]}]);
        let output = render_pmc("PMC42", &response).unwrap();
        assert!(output.contains("# Full article"));
        assert!(output.contains("## Abstract\n\nSummary"));
        assert!(output.contains("## Methods\n\nProcedure"));
        assert!(output.contains("## Results\n\nFindings"));
    }

    #[test]
    fn returns_errors_for_empty_results_and_truncates_utf8_safely() {
        assert!(render_pubmed("42", &serde_json::json!({"resultList":{"result":[]}})).is_err());
        assert!(render_pmc("PMC42", &serde_json::json!([])).is_err());
        assert_eq!(truncate("aéz".into(), 2), ("a".into(), true));
    }
}
