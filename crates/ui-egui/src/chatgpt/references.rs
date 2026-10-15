//! In-memory text attachments with conservative, context-checked navigation after edits.
use serde_json::{Value, json};
use wordcraft_doc::{Document, Pos};
use wordcraft_engine::{Selection, Session};

use super::MAX_PROMPT;

pub(super) const MAX_REFERENCES: usize = 8;
const MAX_SEARCH: usize = 2_000_000;

#[derive(Clone, Debug)]
pub(super) struct Reference {
    pub id: u64,
    pub document: u64,
    pub selection: Selection,
    pub text: String,
    revision: u64,
    before: String,
    after: String,
}

impl Reference {
    pub fn capture(session: &Session, id: u64) -> Result<Self, String> {
        if session.sel.is_collapsed() || session.cell_range().is_some() || session.column_segments().is_some() {
            return Err("Select a continuous range of text to reference".into());
        }
        let (a, b) = session.sel.ordered();
        let text = range_text(&session.doc, &a, &b)?;
        if text.trim().is_empty() {
            return Err("Select some text to reference".into());
        }
        let first = session.doc.para_at(&a).ok_or("Selection is no longer available")?.text.as_str();
        let last = session.doc.para_at(&b).ok_or("Selection is no longer available")?.text.as_str();
        let before = first.get(..a.off).ok_or("Invalid selection boundary")?.chars().rev().take(24).collect::<Vec<_>>().into_iter().rev().collect();
        let after = last.get(b.off..).ok_or("Invalid selection boundary")?.chars().take(24).collect();
        Ok(Self { id, document: session.document_id(), selection: session.sel.clone(), text, revision: session.rev(), before, after })
    }

    pub fn label(&self) -> String {
        format!("Selected text {} · {} words", self.id, self.text.split_whitespace().count())
    }

    pub fn resolve(&self, session: &Session) -> Result<Selection, String> {
        let stale = "Referenced text has changed or is no longer uniquely identifiable. Select it again to attach a new reference.";
        if self.document != session.document_id() {
            return Err("This reference belongs to another document".into());
        }
        if self.revision == session.rev() {
            return Ok(self.selection.clone());
        }
        let story = self.selection.anchor.story;
        let mut text = String::new();
        let mut positions = Vec::new();
        for path in session.doc.para_paths(story) {
            let Some(para) = session.doc.para(story, &path) else { continue };
            if text.len().saturating_add(para.len()).saturating_add(1) > MAX_SEARCH {
                return Err("The document is too large to relocate this reference after editing. Select the text again.".into());
            }
            if !positions.is_empty() {
                text.push('\n');
            }
            let start = text.len();
            text.push_str(&para.text);
            positions.push((start, text.len(), path));
        }
        let mut found = None;
        for (attempt, (start, _)) in text.match_indices(&self.text).enumerate() {
            if attempt >= 128 {
                return Err(stale.into());
            }
            let end = start.saturating_add(self.text.len());
            let a = positions.iter().find(|(lo, hi, _)| start >= *lo && start <= *hi);
            let b = positions.iter().find(|(lo, hi, _)| end >= *lo && end <= *hi);
            let (Some((lo, _, pa)), Some((b_lo, hi, pb))) = (a, b) else { continue };
            if !text.get(*lo..start).is_some_and(|s| s.ends_with(&self.before)) || !text.get(end..*hi).is_some_and(|s| s.starts_with(&self.after)) {
                continue;
            }
            let selection = Selection {
                anchor: Pos::new(story, pa.clone(), start.saturating_sub(*lo)),
                focus: Pos::new(story, pb.clone(), end.saturating_sub(*b_lo)),
            };
            if range_text(&session.doc, &selection.anchor, &selection.focus).ok().as_deref() != Some(self.text.as_str()) {
                continue;
            }
            if found.is_some() {
                return Err(stale.into());
            }
            found = Some(selection);
        }
        found.ok_or_else(|| stale.into())
    }

    pub fn context(&self) -> Value {
        json!({"reference": self.label(), "selection": self.selection, "text": self.text})
    }
}

fn range_text(doc: &Document, a: &Pos, b: &Pos) -> Result<String, String> {
    if a.story != b.story || doc.clamp(a) != *a || doc.clamp(b) != *b {
        return Err("Invalid selection boundary".into());
    }
    let mut size = 0usize;
    let mut first = true;
    for path in doc.para_paths(a.story).into_iter().filter(|p| p >= &a.path && p <= &b.path) {
        let para = doc.para(a.story, &path).ok_or("Selection is no longer available")?;
        let start = if path == a.path { a.off } else { 0 };
        let end = if path == b.path { b.off } else { para.len() };
        size = size.saturating_add(end.saturating_sub(start)).saturating_add(usize::from(!first));
        first = false;
        if size > MAX_PROMPT {
            return Err("Selected text is too long (maximum 32,000 bytes)".into());
        }
    }
    let text = doc.copy_range(a, b).plain_text();
    if text.len() > MAX_PROMPT {
        return Err("Selected text is too long (maximum 32,000 bytes)".into());
    }
    Ok(text)
}
