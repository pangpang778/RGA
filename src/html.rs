#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HtmlSummary {
    pub text: String,
    pub links: Vec<String>,
    pub truncated: bool,
}

pub fn optimize_html_for_tokens(html: &str, max_chars: usize, text_only: bool) -> HtmlSummary {
    let without = strip_blocks(html, "script");
    let without = strip_blocks(&without, "style");
    let without = strip_comments(&without);
    let links = extract_attr_values(&without, "href");
    let text = if text_only {
        tags_to_text(&without)
    } else {
        collapse_ws(&without)
    };
    let truncated = text.chars().count() > max_chars;
    let text = if truncated {
        text.chars().take(max_chars).collect()
    } else {
        text
    };
    HtmlSummary {
        text,
        links,
        truncated,
    }
}

pub fn find_changed_elements(before: &str, after: &str) -> Vec<String> {
    if before == after {
        return Vec::new();
    }
    let b = tags_to_text(before);
    let a = tags_to_text(after);
    a.lines()
        .filter(|line| !line.trim().is_empty() && !b.contains(*line))
        .map(|s| s.trim().to_string())
        .collect()
}

fn strip_blocks(text: &str, tag: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut idx = 0usize;
    let mut out = String::new();
    while let Some(srel) = lower[idx..].find(&open) {
        let s = idx + srel;
        out.push_str(&text[idx..s]);
        if let Some(erel) = lower[s..].find(&close) {
            idx = s + erel + close.len();
        } else {
            idx = text.len();
            break;
        }
    }
    out.push_str(&text[idx..]);
    out
}

fn strip_comments(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(s) = rest.find("<!--") {
        out.push_str(&rest[..s]);
        if let Some(e) = rest[s + 4..].find("-->") {
            rest = &rest[s + 4 + e + 3..];
        } else {
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

fn tags_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    collapse_ws(&html_unescape(&out))
}

fn extract_attr_values(html: &str, attr: &str) -> Vec<String> {
    let needle = format!("{attr}=\"");
    let mut rest = html;
    let mut out = Vec::new();
    while let Some(s) = rest.find(&needle) {
        let after = &rest[s + needle.len()..];
        if let Some(e) = after.find('"') {
            out.push(after[..e].to_string());
            rest = &after[e + 1..];
        } else {
            break;
        }
    }
    out
}

fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn html_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_html_noise() {
        let s = optimize_html_for_tokens(
            "<html><script>x</script><a href=\"u\">Hi</a></html>",
            100,
            true,
        );
        assert_eq!(s.text, "Hi");
        assert_eq!(s.links, vec!["u"]);
    }
}
