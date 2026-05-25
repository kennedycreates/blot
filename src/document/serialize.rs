/// Serialize a `NoteDocument` back to Markdown-ish plain text.
///
/// The output is the canonical source representation used in the editor.
/// `parse(to_source(doc))` should produce an equivalent document (though not
/// necessarily byte-identical, since normalisation may adjust whitespace).
use crate::document::model::{BlockKind, NoteDocument};

/// Render a `NoteDocument` to Markdown-ish source text.
pub fn to_source(doc: &NoteDocument) -> String {
    let mut out = String::new();
    let total = doc.blocks.len();

    for (i, block) in doc.blocks.iter().enumerate() {
        let block_text = render_block(&block.kind);
        out.push_str(&block_text);

        // Add a blank line between blocks unless the current block is empty
        // or this is the last block.
        if i + 1 < total && !block_text.trim().is_empty() {
            out.push('\n');
        }
    }

    // Trim trailing whitespace/newlines for a clean end.
    out.trim_end().to_string()
}

fn render_block(kind: &BlockKind) -> String {
    match kind {
        BlockKind::Paragraph { text } => format!("{text}\n"),

        BlockKind::Heading { level, text } => {
            let hashes = "#".repeat(*level as usize);
            format!("{hashes} {text}\n")
        }

        BlockKind::BulletList { items } => items
            .iter()
            .map(|item| format!("- {}\n", item.text))
            .collect(),

        BlockKind::NumberedList { items } => items
            .iter()
            .enumerate()
            .map(|(i, item)| format!("{}. {}\n", i + 1, item.text))
            .collect(),

        BlockKind::Checklist { items } => items
            .iter()
            .map(|item| {
                let mark = if item.checked { "[x]" } else { "[ ]" };
                format!("- {mark} {}\n", item.text)
            })
            .collect(),

        BlockKind::Divider => "---\n".to_string(),

        BlockKind::Quote { lines } => lines
            .iter()
            .map(|l| {
                if l.is_empty() {
                    ">\n".to_string()
                } else {
                    format!("> {l}\n")
                }
            })
            .collect(),

        BlockKind::Callout {
            style,
            title,
            lines,
        } => {
            let header = match (style.as_deref(), title.as_deref()) {
                (Some(s), Some(t)) => format!("> [!{s}] {t}\n"),
                (Some(s), None) => format!("> [!{s}]\n"),
                (None, Some(t)) => format!("> [!note] {t}\n"),
                (None, None) => "> [!note]\n".to_string(),
            };
            let body: String = lines
                .iter()
                .map(|l| {
                    if l.is_empty() {
                        ">\n".to_string()
                    } else {
                        format!("> {l}\n")
                    }
                })
                .collect();
            format!("{header}{body}")
        }

        BlockKind::ImageCard { alt, path } => format!("![{alt}]({path})\n"),

        BlockKind::NoteLink { target, .. } => format!("[[{target}]]\n"),

        BlockKind::FileLink { display, path } => format!("[{display}]({path})\n"),

        BlockKind::PaletteReference {
            display,
            palette_id,
        } => {
            // Rendered as a file-link-style reference for source roundtrip.
            format!("[{display}](palette://{palette_id})\n")
        }

        BlockKind::KindlingThreadReference { display, thread_id } => {
            format!("[{display}](kindling://{thread_id})\n")
        }

        BlockKind::AbacusFormulaReference {
            display,
            formula_id,
        } => {
            format!("[{display}](abacus://{formula_id})\n")
        }

        BlockKind::FixativeCaptureReference {
            display,
            capture_id,
        } => {
            format!("[{display}](fixative://{capture_id})\n")
        }

        BlockKind::Unknown { raw } => format!("{raw}\n"),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::markdown::parse;
    use crate::document::model::*;

    /// Full round-trip: parse source → serialize back → parse again.
    /// The two documents should be structurally equivalent.
    fn round_trip(source: &str) -> NoteDocument {
        let doc = parse(source);
        let source2 = to_source(&doc);
        parse(&source2)
    }

    #[test]
    fn heading_round_trip() {
        let doc = round_trip("# My Title");
        assert_eq!(
            doc.blocks[0].kind,
            BlockKind::Heading {
                level: 1,
                text: "My Title".into()
            }
        );
    }

    #[test]
    fn paragraph_round_trip() {
        let doc = round_trip("Hello world");
        assert_eq!(
            doc.blocks[0].kind,
            BlockKind::Paragraph {
                text: "Hello world".into()
            }
        );
    }

    #[test]
    fn bullet_list_round_trip() {
        let doc = round_trip("- alpha\n- beta");
        assert!(matches!(&doc.blocks[0].kind, BlockKind::BulletList { items } if items.len() == 2));
    }

    #[test]
    fn numbered_list_round_trip() {
        let doc = round_trip("1. first\n2. second");
        assert!(
            matches!(&doc.blocks[0].kind, BlockKind::NumberedList { items } if items.len() == 2)
        );
    }

    #[test]
    fn checklist_round_trip() {
        let doc = round_trip("- [ ] todo\n- [x] done");
        if let BlockKind::Checklist { items } = &doc.blocks[0].kind {
            assert_eq!(items[0].checked, false);
            assert_eq!(items[1].checked, true);
        } else {
            panic!("expected checklist");
        }
    }

    #[test]
    fn divider_round_trip() {
        let doc = round_trip("---");
        assert_eq!(doc.blocks[0].kind, BlockKind::Divider);
    }

    #[test]
    fn quote_round_trip() {
        let doc = round_trip("> line one\n> line two");
        assert!(matches!(&doc.blocks[0].kind, BlockKind::Quote { lines } if lines.len() == 2));
    }

    #[test]
    fn image_card_round_trip() {
        let doc = round_trip("![alt](img.png)");
        assert_eq!(
            doc.blocks[0].kind,
            BlockKind::ImageCard {
                alt: "alt".into(),
                path: "img.png".into()
            }
        );
    }

    #[test]
    fn note_link_round_trip() {
        let doc = round_trip("[[My Note]]");
        assert!(
            matches!(&doc.blocks[0].kind, BlockKind::NoteLink { target, .. } if target == "My Note")
        );
    }

    #[test]
    fn file_link_round_trip() {
        let doc = round_trip("[Readme](./README.md)");
        assert!(
            matches!(&doc.blocks[0].kind, BlockKind::FileLink { path, .. } if path == "./README.md")
        );
    }

    #[test]
    fn multi_block_round_trip() {
        let source = "# Title\n\nParagraph here.\n\n- a\n- b\n\n---\n\n> quote";
        let doc1 = parse(source);
        let src2 = to_source(&doc1);
        let doc2 = parse(&src2);
        assert_eq!(doc1.blocks.len(), doc2.blocks.len());
        for (b1, b2) in doc1.blocks.iter().zip(&doc2.blocks) {
            assert_eq!(b1.kind, b2.kind);
        }
    }

    #[test]
    fn unknown_block_preserved() {
        let doc = NoteDocument::new(vec![Block {
            id: "x".into(),
            kind: BlockKind::Unknown {
                raw: "some unknown syntax ~@~".into(),
            },
        }]);
        let src = to_source(&doc);
        assert!(src.contains("some unknown syntax ~@~"));
    }

    #[test]
    fn empty_document_serializes_to_empty_string() {
        let doc = NoteDocument::new(vec![]);
        assert_eq!(to_source(&doc), "");
    }
}
