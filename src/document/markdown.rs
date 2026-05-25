/// Parse Markdown-ish plain text into a `NoteDocument`.
///
/// This is Blot's practical internal parser — not CommonMark-compliant, but
/// sufficient for the block types Blot needs.  Unknown or unusual syntax is
/// preserved as `Unknown` blocks so no content is ever silently dropped.
use crate::document::model::{
    new_block_id, Block, BlockKind, ChecklistItem, ListItem, NoteDocument,
};

/// Parse `source` text into a structured `NoteDocument`.
pub fn parse(source: &str) -> NoteDocument {
    let mut blocks: Vec<Block> = Vec::new();
    let mut pending: Pending = Pending::None;

    for line in source.lines() {
        // ── Blank line — flush accumulator ────────────────────────────────
        if line.trim().is_empty() {
            flush(&mut pending, &mut blocks);
            continue;
        }

        // ── Heading ───────────────────────────────────────────────────────
        if let Some(block) = try_heading(line) {
            flush(&mut pending, &mut blocks);
            blocks.push(block);
            continue;
        }

        // ── Divider ───────────────────────────────────────────────────────
        if is_divider(line) {
            flush(&mut pending, &mut blocks);
            blocks.push(Block::new(BlockKind::Divider));
            continue;
        }

        // ── Image card ────────────────────────────────────────────────────
        if let Some(block) = try_image_card(line) {
            flush(&mut pending, &mut blocks);
            blocks.push(block);
            continue;
        }

        // ── Checklist item ────────────────────────────────────────────────
        if let Some(item) = try_checklist_item(line) {
            match &mut pending {
                Pending::Checklist(ref mut items) => items.push(item),
                _ => {
                    flush(&mut pending, &mut blocks);
                    pending = Pending::Checklist(vec![item]);
                }
            }
            continue;
        }

        // ── Bullet list item ──────────────────────────────────────────────
        if let Some(item) = try_bullet_item(line) {
            match &mut pending {
                Pending::BulletList(ref mut items) => items.push(item),
                _ => {
                    flush(&mut pending, &mut blocks);
                    pending = Pending::BulletList(vec![item]);
                }
            }
            continue;
        }

        // ── Numbered list item ────────────────────────────────────────────
        if let Some(item) = try_numbered_item(line) {
            match &mut pending {
                Pending::NumberedList(ref mut items) => items.push(item),
                _ => {
                    flush(&mut pending, &mut blocks);
                    pending = Pending::NumberedList(vec![item]);
                }
            }
            continue;
        }

        // ── Callout / block quote ─────────────────────────────────────────
        if let Some(callout_parts) = try_callout_start(line) {
            // `> [!style] optional title` — start a callout block.
            flush(&mut pending, &mut blocks);
            pending = Pending::Callout {
                style: callout_parts.0,
                title: callout_parts.1,
                lines: Vec::new(),
            };
            continue;
        }

        if let Some(content) = try_quote_line(line) {
            match &mut pending {
                Pending::Quote(ref mut lines) => lines.push(content),
                Pending::Callout { ref mut lines, .. } => lines.push(content),
                _ => {
                    flush(&mut pending, &mut blocks);
                    pending = Pending::Quote(vec![content]);
                }
            }
            continue;
        }

        // ── Standalone wiki-style note link: [[Target]] ───────────────────
        if let Some(block) = try_standalone_note_link(line) {
            flush(&mut pending, &mut blocks);
            blocks.push(block);
            continue;
        }

        // ── Standalone file link: [Label](path) (non-http) ───────────────
        if let Some(block) = try_standalone_file_link(line) {
            flush(&mut pending, &mut blocks);
            blocks.push(block);
            continue;
        }

        // ── Paragraph (default) ───────────────────────────────────────────
        match &mut pending {
            Pending::Paragraph(ref mut lines) => lines.push(line.to_string()),
            _ => {
                flush(&mut pending, &mut blocks);
                pending = Pending::Paragraph(vec![line.to_string()]);
            }
        }
    }

    // Flush any remaining accumulation.
    flush(&mut pending, &mut blocks);

    NoteDocument::new(blocks)
}

// ── Accumulator ───────────────────────────────────────────────────────────────

enum Pending {
    None,
    Paragraph(Vec<String>),
    BulletList(Vec<ListItem>),
    NumberedList(Vec<ListItem>),
    Checklist(Vec<ChecklistItem>),
    Quote(Vec<String>),
    Callout {
        style: Option<String>,
        title: Option<String>,
        lines: Vec<String>,
    },
}

fn flush(pending: &mut Pending, blocks: &mut Vec<Block>) {
    let kind = match std::mem::replace(pending, Pending::None) {
        Pending::None => return,
        Pending::Paragraph(lines) => {
            let text = lines.join("\n");
            if text.trim().is_empty() {
                return;
            }
            BlockKind::Paragraph { text }
        }
        Pending::BulletList(items) => {
            if items.is_empty() {
                return;
            }
            BlockKind::BulletList { items }
        }
        Pending::NumberedList(items) => {
            if items.is_empty() {
                return;
            }
            BlockKind::NumberedList { items }
        }
        Pending::Checklist(items) => {
            if items.is_empty() {
                return;
            }
            BlockKind::Checklist { items }
        }
        Pending::Quote(lines) => {
            if lines.is_empty() {
                return;
            }
            BlockKind::Quote { lines }
        }
        Pending::Callout {
            style,
            title,
            lines,
        } => BlockKind::Callout {
            style,
            title,
            lines,
        },
    };
    blocks.push(Block {
        id: new_block_id(),
        kind,
    });
}

// ── Line-level detectors ──────────────────────────────────────────────────────

fn try_heading(line: &str) -> Option<Block> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }
    let hash_count = trimmed.bytes().take_while(|&b| b == b'#').count();
    if hash_count > 6 {
        return None;
    }
    let rest = &trimmed[hash_count..];
    // Must be `# text` (space after hashes) — bare `#tag` is not a heading.
    if !rest.starts_with(' ') {
        return None;
    }
    let text = rest.trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(Block::new(BlockKind::Heading {
        level: hash_count as u8,
        text,
    }))
}

fn is_divider(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let first = chars[0];
    if first != '-' && first != '*' && first != '_' {
        return false;
    }
    chars.iter().all(|&c| c == first || c == ' ')
        && chars.iter().filter(|&&c| c == first).count() >= 3
}

fn try_image_card(line: &str) -> Option<Block> {
    let s = line.trim();
    if !s.starts_with("![") {
        return None;
    }
    // ![alt](path)
    let rest = &s[2..];
    let bracket_end = rest.find(']')?;
    let alt = rest[..bracket_end].to_string();
    let after_bracket = &rest[bracket_end + 1..];
    if !after_bracket.starts_with('(') || !after_bracket.ends_with(')') {
        return None;
    }
    let path = after_bracket[1..after_bracket.len() - 1].to_string();
    Some(Block::new(BlockKind::ImageCard { alt, path }))
}

fn try_checklist_item(line: &str) -> Option<ChecklistItem> {
    let s = line.trim();
    // Patterns: `- [ ] text`, `- [x] text`, `[ ] text`, `[x] text`
    let (checked, text) = if let Some(rest) = s.strip_prefix("- [x] ").or(s.strip_prefix("- [X] "))
    {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix("- [ ] ") {
        (false, rest)
    } else if let Some(rest) = s.strip_prefix("[x] ").or(s.strip_prefix("[X] ")) {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix("[ ] ") {
        (false, rest)
    } else {
        return None;
    };
    Some(ChecklistItem {
        text: text.to_string(),
        checked,
    })
}

fn try_bullet_item(line: &str) -> Option<ListItem> {
    let s = line.trim();
    if let Some(rest) = s.strip_prefix("- ").or(s.strip_prefix("* ")) {
        // Exclude checklist syntax
        if rest.starts_with('[') {
            return None;
        }
        Some(ListItem {
            text: rest.to_string(),
        })
    } else {
        None
    }
}

fn try_numbered_item(line: &str) -> Option<ListItem> {
    let s = line.trim();
    // Match `N. text` where N is one or more digits.
    let dot_pos = s.find(". ")?;
    if dot_pos == 0 {
        return None;
    }
    let prefix = &s[..dot_pos];
    if !prefix.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(ListItem {
        text: s[dot_pos + 2..].to_string(),
    })
}

/// Returns `(style, title)` if the line is a callout opener `> [!style] title`.
fn try_callout_start(line: &str) -> Option<(Option<String>, Option<String>)> {
    let s = line.trim();
    let inner = s.strip_prefix("> [!")?;
    let bracket_end = inner.find(']')?;
    let style = inner[..bracket_end].to_lowercase();
    let after = inner[bracket_end + 1..].trim();
    let title = if after.is_empty() {
        None
    } else {
        Some(after.to_string())
    };
    Some((Some(style), title))
}

/// Returns the content after `> ` for a quote/callout continuation line.
fn try_quote_line(line: &str) -> Option<String> {
    let s = line.trim();
    if s == ">" {
        return Some(String::new());
    }
    s.strip_prefix("> ").map(|rest| rest.to_string())
}

/// Detects a standalone `[[Target Name]]` note-link line.
fn try_standalone_note_link(line: &str) -> Option<Block> {
    let s = line.trim();
    if !s.starts_with("[[") || !s.ends_with("]]") {
        return None;
    }
    let inner = &s[2..s.len() - 2];
    if inner.contains('[') || inner.contains(']') {
        return None; // nested brackets — not a simple note link
    }
    let target = inner.trim().to_string();
    Some(Block::new(BlockKind::NoteLink {
        display: target.clone(),
        target,
    }))
}

/// Detects a standalone `[Label](path)` file-link line (non-http).
fn try_standalone_file_link(line: &str) -> Option<Block> {
    let s = line.trim();
    if !s.starts_with('[') {
        return None;
    }
    let bracket_end = s.find(']')?;
    let display = s[1..bracket_end].to_string();
    let after = &s[bracket_end + 1..];
    if !after.starts_with('(') || !after.ends_with(')') {
        return None;
    }
    let path = &after[1..after.len() - 1];
    // Only treat as a file link if the path is not an http/https URL.
    if path.starts_with("http://") || path.starts_with("https://") {
        return None;
    }
    // Skip wiki-style note links that might have leaked here.
    if path.starts_with("[[") {
        return None;
    }
    Some(Block::new(BlockKind::FileLink {
        display,
        path: path.to_string(),
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::model::*;

    fn kinds(doc: NoteDocument) -> Vec<BlockKind> {
        doc.blocks.into_iter().map(|b| b.kind).collect()
    }

    #[test]
    fn parses_h1() {
        let doc = parse("# Hello World");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::Heading {
                level: 1,
                text: "Hello World".into()
            }]
        );
    }

    #[test]
    fn parses_h2() {
        let doc = parse("## Section");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::Heading {
                level: 2,
                text: "Section".into()
            }]
        );
    }

    #[test]
    fn parses_h3() {
        let doc = parse("### Sub");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::Heading {
                level: 3,
                text: "Sub".into()
            }]
        );
    }

    #[test]
    fn hash_tag_not_heading() {
        let doc = parse("#tag");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::Paragraph {
                text: "#tag".into()
            }]
        );
    }

    #[test]
    fn parses_paragraph() {
        let doc = parse("Hello world\nHow are you");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::Paragraph {
                text: "Hello world\nHow are you".into()
            }]
        );
    }

    #[test]
    fn blank_line_separates_paragraphs() {
        let doc = parse("First\n\nSecond");
        assert_eq!(
            kinds(doc),
            vec![
                BlockKind::Paragraph {
                    text: "First".into()
                },
                BlockKind::Paragraph {
                    text: "Second".into()
                },
            ]
        );
    }

    #[test]
    fn parses_bullet_list() {
        let doc = parse("- item a\n- item b");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::BulletList {
                items: vec![
                    ListItem {
                        text: "item a".into()
                    },
                    ListItem {
                        text: "item b".into()
                    },
                ]
            }]
        );
    }

    #[test]
    fn parses_numbered_list() {
        let doc = parse("1. first\n2. second");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::NumberedList {
                items: vec![
                    ListItem {
                        text: "first".into()
                    },
                    ListItem {
                        text: "second".into()
                    },
                ]
            }]
        );
    }

    #[test]
    fn parses_checklist() {
        let doc = parse("- [ ] todo\n- [x] done");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::Checklist {
                items: vec![
                    ChecklistItem {
                        text: "todo".into(),
                        checked: false
                    },
                    ChecklistItem {
                        text: "done".into(),
                        checked: true
                    },
                ]
            }]
        );
    }

    #[test]
    fn checklist_without_dash() {
        let doc = parse("[ ] a\n[x] b");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::Checklist {
                items: vec![
                    ChecklistItem {
                        text: "a".into(),
                        checked: false
                    },
                    ChecklistItem {
                        text: "b".into(),
                        checked: true
                    },
                ]
            }]
        );
    }

    #[test]
    fn parses_divider() {
        let doc = parse("---");
        assert_eq!(kinds(doc), vec![BlockKind::Divider]);
    }

    #[test]
    fn divider_variations() {
        for s in &["---", "***", "___", "- - -", "* * *"] {
            let doc = parse(s);
            assert_eq!(kinds(doc), vec![BlockKind::Divider], "failed for: {s}");
        }
    }

    #[test]
    fn parses_quote() {
        let doc = parse("> Some quoted text\n> More");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::Quote {
                lines: vec!["Some quoted text".into(), "More".into()],
            }]
        );
    }

    #[test]
    fn parses_callout() {
        let doc = parse("> [!note] My Title\n> Content line");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::Callout {
                style: Some("note".into()),
                title: Some("My Title".into()),
                lines: vec!["Content line".into()],
            }]
        );
    }

    #[test]
    fn parses_image_card() {
        let doc = parse("![alt text](./image.png)");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::ImageCard {
                alt: "alt text".into(),
                path: "./image.png".into(),
            }]
        );
    }

    #[test]
    fn parses_wiki_note_link() {
        let doc = parse("[[Target Note]]");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::NoteLink {
                display: "Target Note".into(),
                target: "Target Note".into(),
            }]
        );
    }

    #[test]
    fn parses_file_link() {
        let doc = parse("[My File](./docs/file.txt)");
        assert_eq!(
            kinds(doc),
            vec![BlockKind::FileLink {
                display: "My File".into(),
                path: "./docs/file.txt".into(),
            }]
        );
    }

    #[test]
    fn http_link_becomes_paragraph() {
        let doc = parse("[Google](https://google.com)");
        assert!(matches!(kinds(doc)[0], BlockKind::Paragraph { .. }));
    }

    #[test]
    fn mixed_content_does_not_panic() {
        let src = "# Title\n\nParagraph.\n\n- bullet\n\n1. num\n\n---\n\n> quote\n\n> [!tip] Tip\n> Body\n\n[[Link]]\n\n![img](img.png)";
        let doc = parse(src);
        assert!(!doc.blocks.is_empty());
    }

    #[test]
    fn empty_source_gives_empty_document() {
        let doc = parse("");
        assert!(doc.blocks.is_empty());
    }

    #[test]
    fn blank_only_gives_empty_document() {
        let doc = parse("   \n\n   \n");
        assert!(doc.is_empty());
    }

    #[test]
    fn first_heading_text_found() {
        let doc = parse("# My Title\n\nSome text");
        assert_eq!(doc.first_heading_text(), Some("My Title"));
    }

    #[test]
    fn first_heading_text_absent() {
        let doc = parse("Just a paragraph");
        assert_eq!(doc.first_heading_text(), None);
    }

    #[test]
    fn consecutive_list_types_separate() {
        // Bullet then numbered: two separate blocks
        let doc = parse("- a\n\n1. b");
        assert_eq!(doc.blocks.len(), 2);
        assert!(matches!(doc.blocks[0].kind, BlockKind::BulletList { .. }));
        assert!(matches!(doc.blocks[1].kind, BlockKind::NumberedList { .. }));
    }
}
