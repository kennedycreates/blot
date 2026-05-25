/// Parsed, normalized search query.
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub raw: String,
    pub terms: Vec<String>,
}

impl SearchQuery {
    pub fn parse(input: &str) -> Self {
        let raw = input.trim().to_string();
        let terms: Vec<String> = raw
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        SearchQuery { raw, terms }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// True if all query terms appear somewhere in the combined title + body.
    pub fn matches_note(&self, title: &str, body: &str) -> bool {
        if self.terms.is_empty() {
            return true;
        }
        let combined = format!("{title} {body}").to_lowercase();
        self.terms.iter().all(|t| combined.contains(t.as_str()))
    }

    /// Score a title string (weight 2.0 — title matches rank higher).
    pub fn score_title(&self, title: &str) -> f32 {
        self.score_field(title, 2.0)
    }

    /// Score a body string (weight 1.0).
    pub fn score_body(&self, body: &str) -> f32 {
        self.score_field(body, 1.0)
    }

    fn score_field(&self, text: &str, base: f32) -> f32 {
        if self.terms.is_empty() || text.is_empty() {
            return 0.0;
        }
        let lower = text.to_lowercase();
        let mut score = 0.0f32;
        for term in &self.terms {
            if lower.contains(term.as_str()) {
                score += base;
                // Bonus for whole-word match (simple: surrounded by non-alphanumeric)
                if lower
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w == term.as_str())
                {
                    score += base * 0.5;
                }
                // Bonus for matching at the start of the field
                if lower.starts_with(term.as_str()) {
                    score += base;
                }
            }
        }
        score
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_gives_empty_query() {
        let q = SearchQuery::parse("   ");
        assert!(q.is_empty());
        assert!(q.terms.is_empty());
    }

    #[test]
    fn single_term_normalized() {
        let q = SearchQuery::parse("  Hello  ");
        assert_eq!(q.terms, vec!["hello"]);
    }

    #[test]
    fn multi_term_split() {
        let q = SearchQuery::parse("foo bar baz");
        assert_eq!(q.terms, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn matches_note_requires_all_terms() {
        let q = SearchQuery::parse("foo bar");
        assert!(q.matches_note("foo", "bar baz"));
        assert!(!q.matches_note("foo", "qux")); // "bar" missing
    }

    #[test]
    fn matches_note_empty_query_always_true() {
        let q = SearchQuery::parse("");
        assert!(q.matches_note("anything", "goes"));
    }

    #[test]
    fn title_scores_higher_than_body() {
        let q = SearchQuery::parse("rocket");
        let ts = q.score_title("rocket science");
        let bs = q.score_body("rocket science");
        assert!(ts > bs, "title score {ts} should exceed body score {bs}");
    }

    #[test]
    fn no_match_gives_zero() {
        let q = SearchQuery::parse("xyz");
        assert_eq!(q.score_title("nothing here"), 0.0);
        assert_eq!(q.score_body("nothing here"), 0.0);
    }

    #[test]
    fn case_insensitive_matching() {
        let q = SearchQuery::parse("HELLO");
        assert!(q.matches_note("Hello World", ""));
    }
}
