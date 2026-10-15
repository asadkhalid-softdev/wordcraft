//! Turn provider URL annotations into ordinary, clickable Markdown citations.
use serde_json::Value;

pub(super) fn text(part: &Value) -> String {
    let source = part.get("text").and_then(Value::as_str).unwrap_or("");
    let boundaries: Vec<usize> = source.char_indices().map(|(i, _)| i).chain(std::iter::once(source.len())).collect();
    let mut edits = Vec::new();
    let mut missing = Vec::new();
    for annotation in part.get("annotations").and_then(Value::as_array).into_iter().flatten().take(128) {
        if annotation.get("type").and_then(Value::as_str) != Some("url_citation") {
            continue;
        }
        let Some(url) = annotation.get("url").and_then(Value::as_str).and_then(safe_url) else { continue };
        let number = edits.len().saturating_add(missing.len()).saturating_add(1);
        let link = format!("[{number}](<{url}>)");
        let range = annotation
            .get("start_index")
            .and_then(Value::as_u64)
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| boundaries.get(i))
            .zip(annotation.get("end_index").and_then(Value::as_u64).and_then(|i| usize::try_from(i).ok()).and_then(|i| boundaries.get(i)));
        if let Some((&start, &end)) = range.filter(|(start, end)| start <= end) {
            let marker = source.get(start..end).is_some_and(|text| text.contains('\u{e200}'));
            edits.push((if marker { start } else { end }, end, format!(" {link}")));
        } else {
            missing.push(link);
        }
    }
    edits.sort_by_key(|(start, end, _)| (*start, *end));
    let mut result = String::new();
    let mut copied = 0usize;
    for (start, end, link) in edits {
        if start < copied {
            missing.push(link);
            continue;
        }
        result.push_str(source.get(copied..start).unwrap_or_default());
        result.push_str(&link);
        copied = end;
    }
    result.push_str(source.get(copied..).unwrap_or_default());
    if !missing.is_empty() {
        result.push_str("\n\nSources: ");
        result.push_str(&missing.join(" "));
    }
    strip_markers(&result)
}

/// Hide internal citation tokens, including partial tokens while a response streams.
pub(super) fn strip_markers(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('\u{e200}') {
        result.push_str(rest.get(..start).unwrap_or_default());
        let Some(end) = rest.get(start..).and_then(|s| s.find('\u{e201}')).map(|n| start.saturating_add(n).saturating_add('\u{e201}'.len_utf8()))
        else {
            return result;
        };
        rest = rest.get(end..).unwrap_or_default();
    }
    result.push_str(rest);
    result
}

fn safe_url(url: &str) -> Option<String> {
    if url.len() > 4096
        || !["https://", "http://"].iter().any(|prefix| url.to_ascii_lowercase().starts_with(prefix))
        || url.chars().any(char::is_control)
    {
        return None;
    }
    let mut result = String::new();
    for byte in url.bytes() {
        if byte.is_ascii_alphanumeric() || b":/?#@!$&'*+,-.;=%_~".contains(&byte) {
            result.push(char::from(byte));
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn unicode_annotations_produce_clickable_inline_citations_without_internal_tokens() {
        let source = "Café result citesearch0.";
        let start = source.chars().position(|c| c == '\u{e200}').unwrap();
        let end = source.chars().position(|c| c == '\u{e201}').unwrap() + 1;
        let result = text(
            &json!({"text": source, "annotations": [{"type": "url_citation", "start_index": start, "end_index": end, "url": "https://example.com/a?q=one two", "title": "Source"}]}),
        );
        assert!(result.contains("Café result"));
        assert!(result.contains("[1](<https://example.com/a?q=one%20two>)"));
        assert!(!result.contains('\u{e200}'));
        assert_eq!(strip_markers("Answer citepartial"), "Answer ");
        let missing = text(
            &json!({"text": "Claim", "annotations": [{"type": "url_citation", "url": "https://example.com", "start_index": u64::MAX, "end_index": u64::MAX}]}),
        );
        assert!(missing.contains("Sources: [1]"));
        let invalid = text(&json!({"text": "Claim", "annotations": [{"type": "url_citation", "url": "javascript:alert(1)"}]}));
        assert_eq!(invalid, "Claim");
    }
}
