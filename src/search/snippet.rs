const DEFAULT_MAX_CHARS: usize = 160;

/// Extract a short text snippet centered on the first query term match.
/// Returns up to `DEFAULT_MAX_CHARS` characters with `…` ellipsis at edges.
/// Falls back to the beginning of the text when no term is found.
pub fn extract_snippet(text: &str, terms: &[String]) -> String {
    extract_with_len(text, terms, DEFAULT_MAX_CHARS)
}

pub fn extract_with_len(text: &str, terms: &[String], max_chars: usize) -> String {
    // Collapse newlines into spaces for a clean single-line excerpt.
    let flat: String = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    if flat.is_empty() {
        return String::new();
    }

    let total_chars = flat.chars().count();

    // No query terms — return the beginning of the text.
    if terms.is_empty() {
        return truncate_chars(&flat, max_chars);
    }

    let lower = flat.to_lowercase();

    // Find the byte position of the earliest term match.
    let match_byte = terms.iter().filter_map(|t| lower.find(t.as_str())).min();

    let Some(byte_pos) = match_byte else {
        // No match (shouldn't happen after filtering, but be safe).
        return truncate_chars(&flat, max_chars);
    };

    let match_char = flat[..byte_pos].chars().count();

    // Build a window of `max_chars` centered on the match.
    let half = max_chars / 2;
    let start = match_char.saturating_sub(half);
    let end = (start + max_chars).min(total_chars);
    // Slide start back if we're near the end.
    let start = if end == total_chars && total_chars > max_chars {
        total_chars - max_chars
    } else {
        start
    };

    let snippet: String = flat.chars().skip(start).take(end - start).collect();
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < total_chars { "…" } else { "" };
    format!("{prefix}{snippet}{suffix}")
}

fn truncate_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(ts: &[&str]) -> Vec<String> {
        ts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_text_returns_empty() {
        assert_eq!(extract_snippet("", &terms(&["foo"])), "");
        assert_eq!(extract_snippet("   \n  ", &terms(&["foo"])), "");
    }

    #[test]
    fn no_terms_returns_start() {
        let s = "Hello world this is a test sentence.";
        let result = extract_snippet(s, &[]);
        assert!(result.starts_with("Hello world"));
    }

    #[test]
    fn short_text_returned_whole() {
        let s = "Short text.";
        assert_eq!(extract_snippet(s, &terms(&["short"])), "Short text.");
    }

    #[test]
    fn snippet_centered_on_match() {
        let body = "aaa ".repeat(40) + "TARGET" + &" bbb".repeat(40);
        let result = extract_with_len(&body, &terms(&["target"]), 60);
        assert!(result.contains("TARGET"), "expected TARGET in '{result}'");
        assert!(result.starts_with('…') || result.starts_with('a'));
    }

    #[test]
    fn newlines_collapsed() {
        let s = "line one\nline two\nline three";
        let result = extract_snippet(s, &terms(&["two"]));
        assert!(!result.contains('\n'));
        assert!(result.contains("two"));
    }

    #[test]
    fn no_match_returns_start_of_text() {
        let s = "hello world";
        let result = extract_snippet(s, &terms(&["xyz"]));
        assert_eq!(result, "hello world");
    }

    #[test]
    fn long_text_truncated_with_ellipsis() {
        let body: String = "word ".repeat(200);
        let result = extract_with_len(&body, &[], 50);
        assert!(result.ends_with('…'));
        assert!(result.chars().count() <= 52); // 50 chars + ellipsis
    }
}
