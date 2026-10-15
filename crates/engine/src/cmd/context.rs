//! Bounded, revision-aware document discovery for agents. No layout or whole-story text allocation.
use crate::{CmdError, CmdResult, CommandSpec, Session};
use serde_json::{Value, json};
use wordcraft_doc::{Block, Blocks, Paragraph, StoryRef, Table};

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec::new("document.context", "Read Document Context", "File › Info", context)
        .params(r#"{"mode"?: "overview|read|search", "story"?: "body"|{"part": n}, "offset"?: paragraph ordinal, "textOffset"?: UTF-8 byte offset, "limit"?: 1-8, "query"?: string (literal, case-sensitive), "revision"?: current revision, "partsOffset"?: n, "path"?: paragraph path (read one paragraph at a known selection/search position)}. Results include next cursor; follow it until null for full coverage. Paths address table cells too. Overview pages headings; read pages paragraph text; search pages matches and scanning work. Default story is body. Never assumes a page covers the whole document."#).pure()]
}

enum Frame<'a> {
    Blocks(&'a Blocks, usize, Vec<u32>),
    Cells(&'a Table, usize, usize, Vec<u32>),
}
struct Paragraphs<'a> {
    stack: Vec<Frame<'a>>,
    work: usize,
}
impl<'a> Paragraphs<'a> {
    fn new(blocks: &'a Blocks) -> Self {
        Self { stack: vec![Frame::Blocks(blocks, 0, vec![])], work: 0 }
    }
    fn next(&mut self) -> Result<Option<(Vec<u32>, &'a Paragraph)>, CmdError> {
        while let Some(frame) = self.stack.pop() {
            self.work = self.work.saturating_add(1);
            if self.work > 2_000_000 {
                return Err(CmdError::Params("Document traversal limit reached; read a smaller story".into()));
            }
            match frame {
                Frame::Blocks(blocks, index, prefix) => {
                    let Some(block) = blocks.get(index) else { continue };
                    self.stack.push(Frame::Blocks(blocks, index.saturating_add(1), prefix.clone()));
                    let mut path = prefix;
                    path.push(u32::try_from(index).map_err(|_| CmdError::Params("Block index exceeds document addressing limits".into()))?);
                    match block.as_ref() {
                        Block::Para(p) => return Ok(Some((path, p))),
                        Block::Table(t) => {
                            if path.len() > 96 {
                                return Err(CmdError::Params("Table nesting exceeds context traversal limits".into()));
                            }
                            self.stack.push(Frame::Cells(t, 0, 0, path));
                        }
                    }
                }
                Frame::Cells(table, row, cell, prefix) => {
                    let Some(r) = table.rows.get(row) else { continue };
                    let Some(c) = r.cells.get(cell) else {
                        self.stack.push(Frame::Cells(table, row.saturating_add(1), 0, prefix));
                        continue;
                    };
                    self.stack.push(Frame::Cells(table, row, cell.saturating_add(1), prefix.clone()));
                    let mut path = prefix;
                    path.push(u32::try_from(row).map_err(|_| CmdError::Params("Row index too large".into()))?);
                    path.push(u32::try_from(cell).map_err(|_| CmdError::Params("Cell index too large".into()))?);
                    self.stack.push(Frame::Blocks(&c.blocks, 0, path));
                }
            }
        }
        Ok(None)
    }
}
fn number(v: &Value, key: &str) -> usize {
    v.get(key).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(0)
}
fn end_at(text: &str, start: usize, bytes: usize) -> usize {
    let mut end = start.saturating_add(bytes).min(text.len());
    while end > start && !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    end
}
fn context(s: &mut Session, v: &Value) -> CmdResult {
    if v.get("revision").and_then(Value::as_u64).is_some_and(|r| r != s.rev()) {
        return Err(CmdError::Params("Document changed; request a fresh overview and restart the range or search".into()));
    }
    let mode = v.get("mode").and_then(Value::as_str).unwrap_or("overview");
    if !matches!(mode, "overview" | "read" | "search") {
        return Err(CmdError::Params("mode must be overview, read or search".into()));
    }
    let story: StoryRef = match v.get("story") {
        Some(story) => serde_json::from_value(story.clone()).map_err(|_| CmdError::Params("Invalid story".into()))?,
        None => StoryRef::Body,
    };
    if mode == "read" && v.get("path").is_some() {
        let path: wordcraft_doc::Path = serde_json::from_value(v["path"].clone()).map_err(|_| CmdError::Params("Invalid paragraph path".into()))?;
        if path.0.len() > 100 {
            return Err(CmdError::Params("Paragraph path too deep".into()));
        }
        let p = s.doc.para(story, &path).ok_or_else(|| CmdError::Params("Paragraph not found".into()))?;
        let start = number(v, "textOffset");
        if start > p.text.len() || !p.text.is_char_boundary(start) {
            return Err(CmdError::Params("Invalid UTF-8 textOffset".into()));
        }
        let end = end_at(&p.text, start, 2048);
        return Ok(json!({"revision":s.rev(), "story":story, "path":path, "textOffset":start,
            "text":p.text.get(start..end).unwrap_or(""), "length":p.text.len(),
            "next":(end < p.text.len()).then(|| json!({"path":path,"textOffset":end}))}));
    }
    let blocks = s.doc.story(story).ok_or_else(|| CmdError::Params("Story does not exist".into()))?;
    let query = v.get("query").and_then(Value::as_str).unwrap_or("");
    if mode == "search" && (query.is_empty() || query.len() > 256) {
        return Err(CmdError::Params("Search needs a literal query of 1–256 UTF-8 bytes; search related terms separately".into()));
    }
    let offset = number(v, "offset");
    let mut text_offset = number(v, "textOffset");
    let limit = number(v, "limit").clamp(1, 8);
    let limit = if v.get("limit").is_none() { 8 } else { limit };
    let mut paragraphs = Paragraphs::new(blocks);
    let mut ordinal = 0usize;
    let mut scanned = 0usize;
    let mut items = Vec::new();
    let mut item_bytes = 0usize;
    let mut next = Value::Null;
    while let Some((path, p)) = paragraphs.next()? {
        if ordinal < offset {
            ordinal = ordinal.saturating_add(1);
            continue;
        }
        if text_offset > p.text.len() || !p.text.is_char_boundary(text_offset) {
            return Err(CmdError::Params("textOffset must be a UTF-8 boundary within this paragraph".into()));
        }
        if items.len() >= limit || scanned >= 262_144 {
            next = json!({"offset": ordinal, "textOffset": text_offset});
            break;
        }
        let level = s.doc.styles.resolve_para(&p.props).outline_level;
        let start = text_offset;
        let end = end_at(&p.text, start, if mode == "search" { 8192 } else { 1024 });
        let chunk = p.text.get(start..end).unwrap_or("");
        scanned = scanned.saturating_add(chunk.len()).saturating_add(1);
        let found = if mode == "search" { chunk.find(query) } else { Some(0) };
        if found.is_some() && (mode != "overview" || level.is_some_and(|l| l < 9)) {
            let match_offset = start.saturating_add(found.unwrap_or(0));
            let snippet_end = end_at(&p.text, match_offset, if mode == "search" { 512 } else { 1024 });
            let item = json!({"index": ordinal, "path": path, "story": story, "textOffset": match_offset,
                "text": p.text.get(match_offset..snippet_end).unwrap_or(""), "length": p.text.len(), "outlineLevel": level,
                "style": p.props.style.as_deref().map(|x| x.chars().take(100).collect::<String>())});
            let size = item.to_string().len();
            if item_bytes.saturating_add(size) > 12000 {
                next = json!({"offset":ordinal, "textOffset":start});
                break;
            }
            item_bytes = item_bytes.saturating_add(size);
            items.push(item);
        }
        if mode != "overview" && end < p.text.len() {
            // Overlap search windows so a phrase spanning the boundary is still found.
            let mut resume = if mode == "search" { end.saturating_sub(query.len().saturating_sub(1)) } else { end };
            while !p.text.is_char_boundary(resume) {
                resume = resume.saturating_sub(1);
            }
            next = json!({"offset": ordinal, "textOffset": resume});
            break;
        }
        text_offset = 0;
        ordinal = ordinal.saturating_add(1);
    }
    let parts_offset = number(v, "partsOffset");
    let parts: Vec<_> = s
        .doc
        .parts
        .iter()
        .skip(parts_offset)
        .take(16)
        .map(|(id, p)| json!({"story": StoryRef::Part(*id), "kind": p.kind, "blocks": p.blocks.len()}))
        .collect();
    Ok(json!({"documentId": s.document_id(), "revision": s.rev(), "mode": mode, "story": story,
        "title": s.doc.core.title.chars().take(200).collect::<String>(), "blocks": blocks.len(), "items": items, "next": next, "scannedBytes": scanned,
        "parts": parts, "partsNextOffset": (parts_offset.saturating_add(16) < s.doc.parts.len()).then_some(parts_offset.saturating_add(16)),
        "selection": {"anchor": super::pos_json(&s.sel.anchor), "focus": super::pos_json(&s.sel.focus)},
        "note": "Partial page, never the entire document. Follow next with the same mode, story, query and revision. Read parts separately. Text offsets and lengths are UTF-8 bytes; read document.paragraph for detailed formatting."}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordcraft_doc::Document;
    #[test]
    fn pages_reconstruct_large_unicode_paragraph_and_find_boundary_phrase() {
        let text = format!("{}needle{}", "é".repeat(4095), "界".repeat(8000));
        let mut s = Session::new(Document::from_text(&text));
        let mut args = json!({"mode":"read"});
        let mut rebuilt = String::new();
        loop {
            let page = context(&mut s, &args).unwrap();
            assert!(page.to_string().len() < 24000);
            for item in page["items"].as_array().unwrap() {
                rebuilt.push_str(item["text"].as_str().unwrap());
            }
            if page["next"].is_null() {
                break;
            }
            args["offset"] = page["next"]["offset"].clone();
            args["textOffset"] = page["next"]["textOffset"].clone();
        }
        assert_eq!(rebuilt, text);
        let first = context(&mut s, &json!({"mode":"search","query":"needle"})).unwrap();
        assert!(!first["next"].is_null());
        let mut args = first["next"].clone();
        args["mode"] = json!("search");
        args["query"] = json!("needle");
        assert_eq!(context(&mut s, &args).unwrap()["items"][0]["textOffset"], 8190);
        assert!(context(&mut s, &json!({"mode":"read","textOffset":1})).is_err());
        let stale = s.rev().saturating_add(1);
        assert!(context(&mut s, &json!({"revision":stale})).is_err());
    }
    #[test]
    fn table_paths_parts_and_cursor_pages_cover_every_paragraph() {
        use std::sync::Arc;
        use wordcraft_doc::{Document, Part, PartKind, Table};
        let mut doc = Document::from_text("before");
        let mut table = Table::new(1, 2, 300.0);
        table.rows[0].cells[0].blocks = Document::from_text("left").body;
        table.rows[0].cells[1].blocks = Document::from_text("right").body;
        doc.body.push(Arc::new(Block::Table(table)));
        doc.parts.insert(77, Part { kind: PartKind::Footnote, blocks: Document::from_text("note").body, ..Part::default() });
        let mut s = Session::new(doc);
        let mut args = json!({"mode":"read","limit":1});
        let mut texts = vec![];
        loop {
            let page = context(&mut s, &args).unwrap();
            for item in page["items"].as_array().unwrap() {
                texts.push(item["text"].as_str().unwrap().to_owned());
                assert!(s.doc.para(StoryRef::Body, &serde_json::from_value(item["path"].clone()).unwrap()).is_some());
            }
            if page["next"].is_null() {
                break;
            }
            args["offset"] = page["next"]["offset"].clone();
        }
        assert_eq!(texts, ["before", "left", "right"]);
        let note = context(&mut s, &json!({"mode":"read","story":{"part":77},"path":[0]})).unwrap();
        assert_eq!(note["text"], "note");
        assert!(context(&mut s, &json!({"mode":"read","story":{"part":999}})).is_err());
    }
    #[test]
    fn escaped_text_stays_bounded_and_paragraphs_after_2000_remain_accessible() {
        let mut s = Session::new(Document::from_text(&vec!["\u{0001}".repeat(1024); 8].join("\n")));
        let page = context(&mut s, &json!({"mode":"read"})).unwrap();
        assert!(page.to_string().len() < 24000);
        assert!(!page["next"].is_null());
        let text = (0..2200).map(|i| format!("paragraph {i}")).collect::<Vec<_>>().join("\n");
        let mut s = Session::new(Document::from_text(&text));
        let page = context(&mut s, &json!({"mode":"read","offset":2100,"limit":1})).unwrap();
        assert_eq!(page["items"][0]["text"], "paragraph 2100");
    }
    #[test]
    fn overview_never_dumps_body_text() {
        let mut s = Session::new(Document::from_text(&"private body".repeat(100000)));
        let overview = context(&mut s, &json!({})).unwrap();
        assert!(overview["items"].as_array().unwrap().is_empty());
        assert!(overview.to_string().len() < 4000);
    }
}
