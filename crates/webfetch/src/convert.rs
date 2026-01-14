//! HTML conversion utilities

/// Check if content is HTML based on content type and body
pub fn is_html(content_type: &Option<String>, body: &str) -> bool {
    // Check Content-Type
    if let Some(ct) = content_type {
        let ct_lower = ct.to_lowercase();
        if ct_lower.contains("text/html") || ct_lower.contains("application/xhtml") {
            return true;
        }
    }

    // Check body start
    let trimmed = body.trim_start();
    trimmed.starts_with("<!DOCTYPE") || trimmed.starts_with("<html")
}

/// Convert HTML to markdown
pub fn html_to_markdown(html: &str) -> String {
    let mut output = String::new();
    let mut in_skip_element = 0;
    let mut skip_elements: Vec<String> = Vec::new();
    let mut list_depth: usize = 0;
    let mut in_pre = false;
    let mut in_blockquote = false;

    let mut chars = html.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            // Parse tag
            let mut tag = String::new();
            while let Some(&next) = chars.peek() {
                if next == '>' {
                    chars.next();
                    break;
                }
                tag.push(chars.next().unwrap());
            }

            let tag_lower = tag.to_lowercase();
            let is_closing = tag_lower.starts_with('/');
            let tag_name = if is_closing {
                tag_lower[1..].split_whitespace().next().unwrap_or("")
            } else {
                tag_lower.split_whitespace().next().unwrap_or("")
            };

            // Handle skip elements
            let skip_tags = ["script", "style", "noscript", "iframe", "svg"];
            if skip_tags.contains(&tag_name) {
                if is_closing {
                    if let Some(pos) = skip_elements.iter().rposition(|t| t == tag_name) {
                        skip_elements.remove(pos);
                        in_skip_element = skip_elements.len();
                    }
                } else if !tag.ends_with('/') {
                    skip_elements.push(tag_name.to_string());
                    in_skip_element = skip_elements.len();
                }
                continue;
            }

            if in_skip_element > 0 {
                continue;
            }

            // Handle markdown conversion
            match tag_name {
                "h1" => {
                    if !is_closing {
                        output.push_str("\n# ");
                    } else {
                        output.push_str("\n\n");
                    }
                }
                "h2" => {
                    if !is_closing {
                        output.push_str("\n## ");
                    } else {
                        output.push_str("\n\n");
                    }
                }
                "h3" => {
                    if !is_closing {
                        output.push_str("\n### ");
                    } else {
                        output.push_str("\n\n");
                    }
                }
                "h4" => {
                    if !is_closing {
                        output.push_str("\n#### ");
                    } else {
                        output.push_str("\n\n");
                    }
                }
                "h5" => {
                    if !is_closing {
                        output.push_str("\n##### ");
                    } else {
                        output.push_str("\n\n");
                    }
                }
                "h6" => {
                    if !is_closing {
                        output.push_str("\n###### ");
                    } else {
                        output.push_str("\n\n");
                    }
                }
                "p" | "div" | "section" | "article" | "main" | "header" | "footer" => {
                    if is_closing {
                        output.push_str("\n\n");
                    }
                }
                "br" => {
                    output.push('\n');
                }
                "hr" => {
                    output.push_str("\n---\n");
                }
                "ul" | "ol" => {
                    if is_closing {
                        list_depth = list_depth.saturating_sub(1);
                        if list_depth == 0 {
                            output.push('\n');
                        }
                    } else {
                        list_depth += 1;
                    }
                }
                "li" => {
                    if !is_closing {
                        output.push('\n');
                        for _ in 0..list_depth.saturating_sub(1) {
                            output.push_str("  ");
                        }
                        output.push_str("- ");
                    }
                }
                "strong" | "b" => {
                    output.push_str("**");
                }
                "em" | "i" => {
                    output.push('*');
                }
                "pre" => {
                    if !is_closing {
                        output.push_str("\n```\n");
                        in_pre = true;
                    } else {
                        output.push_str("\n```\n");
                        in_pre = false;
                    }
                }
                "code" => {
                    if !in_pre {
                        output.push('`');
                    }
                }
                "blockquote" => {
                    if !is_closing {
                        in_blockquote = true;
                        output.push_str("\n> ");
                    } else {
                        in_blockquote = false;
                        output.push('\n');
                    }
                }
                "a" => {
                    if !is_closing {
                        // Extract href
                        if let Some(href) = extract_attribute(&tag, "href") {
                            output.push('[');
                            // We'll close with ]() format - naive implementation
                            // Push href placeholder, will be formatted after link text
                            output.push_str(&format!("]({})", href));
                        }
                    }
                }
                _ => {}
            }
        } else if in_skip_element == 0 {
            // Text content
            let decoded = decode_entity(c, &mut chars);
            if in_blockquote && decoded == '\n' {
                output.push_str("\n> ");
            } else {
                output.push(decoded);
            }
        }
    }

    clean_whitespace(&output)
}

/// Convert HTML to plain text
pub fn html_to_text(html: &str) -> String {
    let mut output = String::new();
    let mut in_skip_element = 0;
    let mut skip_elements: Vec<String> = Vec::new();

    let mut chars = html.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '<' {
            // Parse tag
            let mut tag = String::new();
            while let Some(&next) = chars.peek() {
                if next == '>' {
                    chars.next();
                    break;
                }
                tag.push(chars.next().unwrap());
            }

            let tag_lower = tag.to_lowercase();
            let is_closing = tag_lower.starts_with('/');
            let tag_name = if is_closing {
                tag_lower[1..].split_whitespace().next().unwrap_or("")
            } else {
                tag_lower.split_whitespace().next().unwrap_or("")
            };

            // Handle skip elements
            let skip_tags = ["script", "style", "noscript", "iframe", "svg"];
            if skip_tags.contains(&tag_name) {
                if is_closing {
                    if let Some(pos) = skip_elements.iter().rposition(|t| t == tag_name) {
                        skip_elements.remove(pos);
                        in_skip_element = skip_elements.len();
                    }
                } else if !tag.ends_with('/') {
                    skip_elements.push(tag_name.to_string());
                    in_skip_element = skip_elements.len();
                }
                continue;
            }

            if in_skip_element > 0 {
                continue;
            }

            // Handle newline-inducing elements
            let newline_tags = [
                "p", "div", "br", "h1", "h2", "h3", "h4", "h5", "h6", "li", "tr",
            ];
            if newline_tags.contains(&tag_name) && (is_closing || tag_name == "br") {
                output.push('\n');
            } else if newline_tags.contains(&tag_name) && !is_closing {
                // Opening tags like h1-h6, p, etc. also add newline
                if matches!(tag_name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p") {
                    output.push('\n');
                }
            }
        } else if in_skip_element == 0 {
            // Text content
            let decoded = decode_entity(c, &mut chars);
            output.push(decoded);
        }
    }

    clean_whitespace(&output)
}

/// Extract attribute value from tag
fn extract_attribute(tag: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=", attr);
    let tag_lower = tag.to_lowercase();

    if let Some(start) = tag_lower.find(&pattern) {
        let rest = &tag[start + pattern.len()..];
        let rest = rest.trim_start();

        if let Some(rest) = rest.strip_prefix('"') {
            if let Some(end) = rest.find('"') {
                return Some(rest[..end].to_string());
            }
        } else if let Some(rest) = rest.strip_prefix('\'') {
            if let Some(end) = rest.find('\'') {
                return Some(rest[..end].to_string());
            }
        } else {
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '>')
                .unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// Decode HTML entity starting from ampersand
fn decode_entity(c: char, chars: &mut std::iter::Peekable<std::str::Chars>) -> char {
    if c != '&' {
        return c;
    }

    let mut entity = String::new();
    while let Some(&next) = chars.peek() {
        if next == ';' {
            chars.next();
            break;
        }
        if next.is_whitespace() || entity.len() > 10 {
            // Not a valid entity
            return '&';
        }
        entity.push(chars.next().unwrap());
    }

    match entity.as_str() {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "#39" => '\'',
        "nbsp" => ' ',
        "mdash" => '—',
        "ndash" => '–',
        "copy" => '©',
        "reg" => '®',
        _ => {
            // Check for numeric entities
            if let Some(num_str) = entity.strip_prefix('#') {
                if let Some(stripped) = num_str.strip_prefix('x') {
                    // Hex entity
                    if let Ok(code) = u32::from_str_radix(stripped, 16) {
                        if let Some(ch) = char::from_u32(code) {
                            return ch;
                        }
                    }
                } else if let Ok(code) = num_str.parse::<u32>() {
                    if let Some(ch) = char::from_u32(code) {
                        return ch;
                    }
                }
            }
            // Unknown entity - return original
            '&'
        }
    }
}

/// Clean whitespace: collapse runs, trim, keep max 2 newlines
pub fn clean_whitespace(s: &str) -> String {
    let mut result = String::new();
    let mut last_was_space = false;
    let mut newline_count = 0;

    for c in s.chars() {
        if c == '\n' {
            // Remove trailing space before newline
            if last_was_space && result.ends_with(' ') {
                result.pop();
            }
            newline_count += 1;
            // Treat newline as space for next char collapsing
            last_was_space = true;
            if newline_count <= 2 {
                result.push(c);
            }
        } else if c.is_whitespace() {
            newline_count = 0;
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            newline_count = 0;
            last_was_space = false;
            result.push(c);
        }
    }

    result.trim().to_string()
}

/// Filter excessive newlines: keep at most 2 consecutive newlines
pub fn filter_excessive_newlines(s: &str) -> String {
    let mut result = String::new();
    let mut newline_count = 0;

    for c in s.chars() {
        if c == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                result.push(c);
            }
        } else {
            newline_count = 0;
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_html_by_content_type() {
        assert!(is_html(&Some("text/html".to_string()), ""));
        assert!(is_html(&Some("text/html; charset=utf-8".to_string()), ""));
        assert!(is_html(&Some("application/xhtml+xml".to_string()), ""));
        assert!(!is_html(&Some("text/plain".to_string()), ""));
        assert!(!is_html(&Some("application/json".to_string()), ""));
    }

    #[test]
    fn test_is_html_by_body() {
        assert!(is_html(&None, "<!DOCTYPE html><html>"));
        assert!(is_html(&None, "  <!DOCTYPE html>"));
        assert!(is_html(&None, "<html><body>"));
        assert!(!is_html(&None, "Hello world"));
        assert!(!is_html(&None, "{\"json\": true}"));
    }

    #[test]
    fn test_html_to_markdown_headers() {
        let html = "<h1>Title</h1><h2>Subtitle</h2>";
        let md = html_to_markdown(html);
        assert!(md.contains("# Title"));
        assert!(md.contains("## Subtitle"));
    }

    #[test]
    fn test_html_to_markdown_paragraphs() {
        let html = "<p>First paragraph</p><p>Second paragraph</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("First paragraph"));
        assert!(md.contains("Second paragraph"));
    }

    #[test]
    fn test_html_to_markdown_lists() {
        let html = "<ul><li>Item 1</li><li>Item 2</li></ul>";
        let md = html_to_markdown(html);
        assert!(md.contains("- Item 1"));
        assert!(md.contains("- Item 2"));
    }

    #[test]
    fn test_html_to_markdown_emphasis() {
        let html = "<p><strong>bold</strong> and <em>italic</em></p>";
        let md = html_to_markdown(html);
        assert!(md.contains("**bold**"));
        assert!(md.contains("*italic*"));
    }

    #[test]
    fn test_html_to_markdown_code() {
        let html = "<pre>code block</pre>";
        let md = html_to_markdown(html);
        assert!(md.contains("```"));
        assert!(md.contains("code block"));
    }

    #[test]
    fn test_html_to_markdown_skip_script() {
        let html = "<p>Before</p><script>alert('bad');</script><p>After</p>";
        let md = html_to_markdown(html);
        assert!(md.contains("Before"));
        assert!(md.contains("After"));
        assert!(!md.contains("alert"));
    }

    #[test]
    fn test_html_to_text_simple() {
        let html = "<p>Hello</p><p>World</p>";
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn test_html_to_text_skip_script() {
        let html = "<p>Before</p><script>alert('bad');</script><p>After</p>";
        let text = html_to_text(html);
        assert!(text.contains("Before"));
        assert!(text.contains("After"));
        assert!(!text.contains("alert"));
    }

    #[test]
    fn test_entity_decoding() {
        let html = "<p>&amp; &lt; &gt; &quot; &apos; &nbsp; &mdash; &ndash; &copy; &reg;</p>";
        let text = html_to_text(html);
        assert!(text.contains('&'));
        assert!(text.contains('<'));
        assert!(text.contains('>'));
        assert!(text.contains('"'));
        assert!(text.contains('\''));
        assert!(text.contains('—'));
        assert!(text.contains('–'));
        assert!(text.contains('©'));
        assert!(text.contains('®'));
    }

    #[test]
    fn test_filter_excessive_newlines() {
        let input = "line1\n\n\n\n\nline2";
        let output = filter_excessive_newlines(input);
        assert_eq!(output, "line1\n\nline2");
    }

    #[test]
    fn test_clean_whitespace() {
        let input = "  hello   world  \n\n\n\n  test  ";
        let output = clean_whitespace(input);
        assert_eq!(output, "hello world\n\ntest");
    }

    #[test]
    fn test_extract_attribute() {
        assert_eq!(
            extract_attribute("a href=\"https://example.com\" class=\"link\"", "href"),
            Some("https://example.com".to_string())
        );
        assert_eq!(
            extract_attribute("img src='image.png'", "src"),
            Some("image.png".to_string())
        );
        assert_eq!(
            extract_attribute("div class=test", "class"),
            Some("test".to_string())
        );
    }
}
