//! Apply LSP [`TextEdit`] / [`WorkspaceEdit`] to strings and open buffers.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context as _, bail};
use gpui_kit::component::input::{Rope, RopeExt as _};
use lsp_types::{
    DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range,
    TextDocumentEdit, TextEdit, Uri, WorkspaceEdit,
};

use super::uri::uri_to_path;

/// Apply `edits` to `text` (LSP UTF-16 positions). Edits are applied bottom-up.
pub fn apply_text_edits(text: &str, edits: &[TextEdit]) -> String {
    if edits.is_empty() {
        return text.to_owned();
    }
    let mut ordered: Vec<&TextEdit> = edits.iter().collect();
    ordered.sort_by(|a, b| {
        b.range
            .start
            .cmp(&a.range.start)
            .then(b.range.end.cmp(&a.range.end))
    });
    let mut rope = Rope::from(text);
    for edit in ordered {
        let start = rope.position_to_offset(&edit.range.start);
        let end = rope.position_to_offset(&edit.range.end);
        let start = start.min(rope.len());
        let end = end.min(rope.len()).max(start);
        rope.replace(start..end, &edit.new_text);
    }
    rope.to_string()
}

/// One edit turning `old` into `new`, trimmed to the differing lines so the
/// cursor and scroll outside them stay put. `None` when they are equal.
pub fn replace_edit(old: &str, new: &str) -> Option<TextEdit> {
    if old == new {
        return None;
    }
    let o: Vec<&str> = old.split_inclusive('\n').collect();
    let n: Vec<&str> = new.split_inclusive('\n').collect();
    let pre = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let suf = o[pre..]
        .iter()
        .rev()
        .zip(n[pre..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let end_line = o.len() - suf;
    let end = if end_line < o.len() || old.ends_with('\n') || o.is_empty() {
        Position::new(end_line as u32, 0)
    } else {
        // Replacing through an unterminated last line: end at its last character.
        let last = o[o.len() - 1];
        Position::new(o.len() as u32 - 1, last.encode_utf16().count() as u32)
    };
    Some(TextEdit {
        range: Range::new(Position::new(pre as u32, 0), end),
        new_text: n[pre..n.len() - suf].concat(),
    })
}

/// Paths + text edits from a [`WorkspaceEdit`] (documentChanges preferred).
pub fn workspace_edit_file_edits(
    edit: &WorkspaceEdit,
) -> anyhow::Result<Vec<(PathBuf, Vec<TextEdit>)>> {
    let mut by_path: HashMap<PathBuf, Vec<TextEdit>> = HashMap::new();

    if let Some(DocumentChanges::Edits(doc_edits)) = edit.document_changes.as_ref() {
        for doc in doc_edits {
            push_document_edit(&mut by_path, doc)?;
        }
    } else if let Some(DocumentChanges::Operations(ops)) = edit.document_changes.as_ref() {
        for op in ops {
            match op {
                lsp_types::DocumentChangeOperation::Edit(doc) => {
                    push_document_edit(&mut by_path, doc)?;
                }
                lsp_types::DocumentChangeOperation::Op(_) => {
                    bail!("workspace resource operations are not supported");
                }
            }
        }
    } else if let Some(changes) = edit.changes.as_ref() {
        for (uri, edits) in changes {
            let path = path_from_uri(uri)?;
            by_path.entry(path).or_default().extend(edits.clone());
        }
    }

    Ok(by_path.into_iter().collect())
}

fn push_document_edit(
    by_path: &mut HashMap<PathBuf, Vec<TextEdit>>,
    doc: &TextDocumentEdit,
) -> anyhow::Result<()> {
    let path = path_from_versioned(&doc.text_document)?;
    let edits: Vec<TextEdit> = doc
        .edits
        .iter()
        .map(|e| match e {
            OneOf::Left(edit) => edit.clone(),
            OneOf::Right(annotated) => annotated.text_edit.clone(),
        })
        .collect();
    by_path.entry(path).or_default().extend(edits);
    Ok(())
}

fn path_from_versioned(doc: &OptionalVersionedTextDocumentIdentifier) -> anyhow::Result<PathBuf> {
    path_from_uri(&doc.uri)
}

fn path_from_uri(uri: &Uri) -> anyhow::Result<PathBuf> {
    uri_to_path(uri).with_context(|| format!("edit URI is not a file path: {}", uri.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_edit_round_trips_and_stays_minimal() {
        for (old, new) in [
            ("a\n  b\nc\n", "a\nb\nc\n"),
            ("a\nb", "a\nb\n"),
            ("a\nb\n", "a\nb"),
            ("x\n", "y\n"),
            ("same\n", "same\n"),
            ("", "new\n"),
            ("é\n  é", "é\né\n"),
        ] {
            match replace_edit(old, new) {
                Some(e) => assert_eq!(apply_text_edits(old, &[e]), new, "{old:?} -> {new:?}"),
                None => assert_eq!(old, new),
            }
        }
        let e = replace_edit("a\n  b\nc\n", "a\nb\nc\n").unwrap();
        assert_eq!((e.range.start.line, e.range.end.line), (1, 2), "only line 2 touched");
    }

    fn edit(sl: u32, sc: u32, el: u32, ec: u32, new: &str) -> TextEdit {
        TextEdit {
            range: Range {
                start: Position {
                    line: sl,
                    character: sc,
                },
                end: Position {
                    line: el,
                    character: ec,
                },
            },
            new_text: new.into(),
        }
    }

    #[test]
    fn applies_edits_bottom_up() {
        let text = "hello\nworld\n";
        let out = apply_text_edits(
            text,
            &[edit(0, 0, 0, 5, "HELLO"), edit(1, 0, 1, 5, "WORLD")],
        );
        assert_eq!(out, "HELLO\nWORLD\n");
    }

    #[test]
    fn insert_at_start() {
        let out = apply_text_edits("abc", &[edit(0, 0, 0, 0, "xy")]);
        assert_eq!(out, "xyabc");
    }
}
