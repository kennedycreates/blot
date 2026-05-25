use crate::search::query::SearchQuery;
use crate::search::result::SearchResult;
use std::path::Path;

/// Apply all ranking boosts to each result, then sort descending by score.
/// Call this after all results have their base scores set by providers.
pub fn rank(
    results: &mut Vec<SearchResult>,
    query: &SearchQuery,
    focused_workspace_path: Option<&Path>,
) {
    for r in results.iter_mut() {
        r.score += boost_focused_workspace(r, focused_workspace_path);
        r.score += boost_pinned(r);
        r.score += boost_recency(&r.updated_at);
    }
    // Stable descending sort: equal scores preserve insertion order.
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let _ = query; // query is available for future term-frequency boosts
}

fn boost_focused_workspace(result: &SearchResult, focused: Option<&Path>) -> f32 {
    match (&result.workspace_path, focused) {
        (Some(ws), Some(foc)) if ws == foc => 5.0,
        _ => 0.0,
    }
}

fn boost_pinned(result: &SearchResult) -> f32 {
    if result.is_pinned {
        2.0
    } else {
        0.0
    }
}

/// Tiny recency boost so recently-edited notes float up among otherwise
/// equal-scored results. Each year above 2020 adds 0.1.
fn boost_recency(updated_at: &str) -> f32 {
    if updated_at.len() < 4 {
        return 0.0;
    }
    let year: u32 = updated_at[..4].parse().unwrap_or(0);
    if year > 2020 {
        (year - 2020) as f32 * 0.1
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::result::{NoteLocation, NoteSourceKind};
    use std::path::PathBuf;

    fn make_result(score: f32, pinned: bool, ws_path: Option<&str>) -> SearchResult {
        SearchResult {
            note_id: "n".into(),
            title: "T".into(),
            snippet: String::new(),
            updated_at: "2026-05-22T00:00:00Z".into(),
            location: NoteLocation::Inbox,
            workspace_name: None,
            workspace_path: ws_path.map(PathBuf::from),
            is_pinned: pinned,
            source_kind: NoteSourceKind::InboxNote,
            has_checklist: false,
            has_image: false,
            has_links: false,
            score,
        }
    }

    #[test]
    fn higher_base_score_ranks_first() {
        let q = SearchQuery::parse("x");
        let mut results = vec![make_result(1.0, false, None), make_result(3.0, false, None)];
        rank(&mut results, &q, None);
        assert_eq!(
            results[0].score,
            results
                .iter()
                .map(|r| r.score)
                .fold(f32::NEG_INFINITY, f32::max)
        );
    }

    #[test]
    fn pinned_note_gets_boost() {
        let q = SearchQuery::parse("x");
        let mut results = vec![make_result(1.0, false, None), make_result(1.0, true, None)];
        rank(&mut results, &q, None);
        // The pinned one should come first (higher total score)
        assert!(results[0].is_pinned);
    }

    #[test]
    fn focused_workspace_note_gets_boost() {
        let q = SearchQuery::parse("x");
        let mut results = vec![
            make_result(1.0, false, Some("/other.water")),
            make_result(1.0, false, Some("/focused.water")),
        ];
        rank(&mut results, &q, Some(Path::new("/focused.water")));
        assert_eq!(
            results[0].workspace_path.as_deref(),
            Some(Path::new("/focused.water"))
        );
    }

    #[test]
    fn recency_boost_applied() {
        // Year 2026 → boost = 0.6; year 2021 → boost = 0.1
        assert!((boost_recency("2026") - 0.6).abs() < 0.001);
        assert!((boost_recency("2021") - 0.1).abs() < 0.001);
        assert_eq!(boost_recency("2020"), 0.0);
        assert_eq!(boost_recency(""), 0.0);
    }
}
