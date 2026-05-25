/// Smart title extraction and blank detection for Inbox notes.
/// Pure functions — no GTK or DB dependencies, fully testable.

const MAX_TITLE_CHARS: usize = 80;

/// Derive a display title from note body text.
///
/// Resolution order:
/// 1. First Markdown heading line (`# …`, `## …`, `### …`)
/// 2. First meaningful non-empty, non-structural line
/// 3. Returns an empty string if the body is blank or unreadable
///    (caller should substitute a timestamp fallback in that case)
pub fn derive_title(body: &str) -> String {
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Strip Markdown heading markers (# / ## / ###) if followed by a space.
        let candidate = if let Some(rest) = trimmed.strip_prefix("### ") {
            rest.trim()
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            rest.trim()
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            rest.trim()
        } else if trimmed.starts_with("---") || trimmed.starts_with("===") {
            // Horizontal rules / setext underlines — skip
            continue;
        } else {
            trimmed
        };

        if !candidate.is_empty() {
            return truncate_to(candidate, MAX_TITLE_CHARS);
        }
    }
    String::new()
}

/// True when the text contains no meaningful content (only whitespace).
pub fn is_blank(text: &str) -> bool {
    text.chars().all(|c| c.is_whitespace())
}

/// Approximate word count (whitespace-separated tokens).
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

fn truncate_to(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h1_heading_becomes_title() {
        assert_eq!(derive_title("# My Great Note\nsome body"), "My Great Note");
    }

    #[test]
    fn h2_heading_becomes_title() {
        assert_eq!(derive_title("## Section\nbody"), "Section");
    }

    #[test]
    fn h3_heading_becomes_title() {
        assert_eq!(derive_title("### Sub\nbody"), "Sub");
    }

    #[test]
    fn first_line_when_no_heading() {
        assert_eq!(derive_title("Hello world\nmore text"), "Hello world");
    }

    #[test]
    fn skips_blank_lines_before_content() {
        assert_eq!(derive_title("\n\nActual content"), "Actual content");
    }

    #[test]
    fn heading_preferred_over_earlier_plain_line() {
        // Heading on second non-blank line still wins because we take the FIRST
        // non-blank line — which happens to be the heading here.
        assert_eq!(derive_title("# Title\nPlain line"), "Title");
    }

    #[test]
    fn plain_line_before_heading() {
        // The first non-blank line is plain text, not a heading.
        assert_eq!(derive_title("Plain line\n# Heading"), "Plain line");
    }

    #[test]
    fn blank_body_returns_empty() {
        assert_eq!(derive_title(""), "");
        assert_eq!(derive_title("   \n\n\t"), "");
    }

    #[test]
    fn long_line_is_truncated() {
        let long = "A".repeat(100);
        let title = derive_title(&long);
        assert!(title.chars().count() <= MAX_TITLE_CHARS + 1); // +1 for ellipsis
        assert!(title.ends_with('…'));
    }

    #[test]
    fn is_blank_empty_string() {
        assert!(is_blank(""));
        assert!(is_blank("   "));
        assert!(is_blank("\n\t\r\n"));
    }

    #[test]
    fn is_blank_non_empty() {
        assert!(!is_blank("a"));
        assert!(!is_blank("  hello  "));
    }

    #[test]
    fn word_count_empty() {
        assert_eq!(word_count(""), 0);
    }

    #[test]
    fn word_count_basic() {
        assert_eq!(word_count("one two three"), 3);
    }

    #[test]
    fn word_count_extra_whitespace() {
        assert_eq!(word_count("  hello   world  "), 2);
    }

    #[test]
    fn skips_horizontal_rules() {
        // Body starts with --- which looks like a rule — should fall through to next line.
        assert_eq!(derive_title("---\nActual title"), "Actual title");
    }

    #[test]
    fn hash_without_space_is_not_a_heading() {
        // #tag is not a Markdown heading.
        assert_eq!(derive_title("#tag something"), "#tag something");
    }
}
