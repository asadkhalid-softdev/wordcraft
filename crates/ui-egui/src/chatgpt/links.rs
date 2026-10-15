//! Verified, document-scoped destinations for assistant-authored Markdown links.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wordcraft_engine::Selection;

use super::Reference;
use crate::WordApp;

pub(super) const MAX_LINKS: usize = 512;
const PREFIX: &str = "wordcraft://document/";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DocumentLink {
    token: String,
    pub reference: Reference,
}
impl DocumentLink {
    pub fn valid(&self) -> bool {
        valid_token(&self.token)
    }
}
fn valid_token(token: &str) -> bool {
    token.len() == 32 && token.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(super) fn token(url: &str) -> Option<&str> {
    url.strip_prefix(PREFIX).filter(|token| valid_token(token))
}
#[cfg(not(target_arch = "wasm32"))]
fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "Could not create document link identity")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
#[cfg(target_arch = "wasm32")]
fn new_token() -> Result<String, String> {
    Err("Document chat links require the desktop app".into())
}

pub(super) fn create(app: &mut WordApp, args: &Value) -> Result<Value, String> {
    if !app.has_document() || args.get("documentId").and_then(Value::as_u64) != Some(app.session.document_id()) {
        return Err("Read the current document before creating a link".into());
    }
    if args.get("revision").and_then(Value::as_u64) != Some(app.session.rev()) {
        return Err("Document changed; read the passage again before creating its link".into());
    }
    if app.chatgpt.links.len() >= MAX_LINKS {
        return Err("This conversation has reached its 512 document-link limit".into());
    }
    let label = args
        .get("label")
        .and_then(Value::as_str)
        .filter(|label| !label.trim().is_empty() && label.len() <= 120 && !label.chars().any(char::is_control))
        .ok_or("Use a nonempty document-link label of at most 120 bytes")?;
    let text = args
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty() && text.len() <= super::MAX_PROMPT)
        .ok_or("Provide the exact quoted passage (maximum 32,000 bytes)")?;
    let value = args.get("selection").filter(|value| value.to_string().len() <= 8192).ok_or("Provide a bounded document selection")?;
    let selection: Selection = serde_json::from_value(value.clone()).map_err(|_| "Invalid document-link selection")?;
    let id = app.chatgpt.next_reference.checked_add(1).ok_or("Clear the conversation to reset references")?;
    let reference = Reference::capture_range(&app.session, selection, id)?;
    if reference.text != text {
        return Err("Quoted text does not match the range; read the passage again".into());
    }
    let token = new_token()?;
    if app.chatgpt.links.iter().any(|link| link.token == token) {
        return Err("Could not create a unique document link; try again".into());
    }
    let url = format!("{PREFIX}{token}");
    let mut escaped = String::new();
    for ch in label.chars() {
        if "\\`*_{}[]()<>!#$|".contains(ch) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    app.chatgpt.next_reference = id;
    app.chatgpt.links.push(DocumentLink { token, reference });
    app.chatgpt.document = Some(app.session.document_id());
    app.chatgpt.mark_history();
    Ok(json!({"markdown":format!("[{escaped}]({url})"),"url":url,"label":label}))
}

pub(super) fn navigate(app: &mut WordApp, url: &str) -> Result<(), String> {
    let token = token(url).ok_or("Invalid document link")?;
    let link = app.chatgpt.links.iter().find(|link| link.token == token).ok_or("This link does not belong to the current conversation")?;
    let selection = link.reference.resolve(&app.session)?;
    app.execute("select.range", json!(selection))?;
    app.canvas.reveal_selection_start(&app.session.sel);
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::Services;
    use egui_kittest::kittest::Queryable;
    use wordcraft_doc::{Document, Pos, StoryRef};
    use wordcraft_engine::Session;
    fn app(text: &str) -> WordApp {
        WordApp::new(Session::new(Document::from_text(text)), Services::default())
    }
    fn args(app: &WordApp, start: usize, end: usize, quote: &str, label: &str) -> Value {
        json!({"documentId":app.session.document_id(),"revision":app.session.rev(),
            "selection":Selection{anchor:Pos::body(0,start),focus:Pos::body(0,end)},"text":quote,"label":label})
    }
    fn create_link(app: &mut WordApp, start: usize, end: usize, text: &str, label: &str) -> Result<Value, String> {
        let args = args(app, start, end, text, label);
        app.execute("ui.chatgpt.createReference", args)
    }
    #[test]
    fn citations_verify_text_without_editing_or_moving_and_relocate_after_edits() {
        let mut app = app("Hello world");
        let selection = app.session.sel.clone();
        let revision = app.session.rev();
        let link = create_link(&mut app, 6, 11, "world", "See [passage]").unwrap();
        assert_eq!(app.session.sel, selection);
        assert_eq!(app.session.rev(), revision);
        assert_eq!(app.session.doc.plain_text(StoryRef::Body), "Hello world");
        assert!(link["markdown"].as_str().unwrap().starts_with("[See \\[passage\\]]("));
        assert_eq!(super::super::markdown::normalize_math(link["markdown"].as_str().unwrap()), link["markdown"].as_str().unwrap());
        assert!(app.chatgpt.references.is_empty(), "Response citations are separate from composer attachments");
        app.execute("caret.docStart", json!({})).unwrap();
        app.execute("text.insert", json!({"text":"Intro: "})).unwrap();
        app.execute("ui.chatgpt.showLink", json!({"url":link["url"]})).unwrap();
        assert_eq!(app.session.selected_text(), "world");
        assert_eq!(app.session.sel.anchor.off, 13);
        assert_eq!(app.canvas.nav_to_pos, Some(app.session.sel.ordered().0));
        app.execute("text.insert", json!({"text":"changed"})).unwrap();
        assert!(app.execute("ui.chatgpt.showLink", json!({"url":link["url"]})).is_err());
    }
    #[test]
    fn stale_forged_hostile_and_cross_conversation_destinations_are_rejected() {
        let mut app = app("Hello world");
        let mut value = args(&app, 6, 11, "not world", "Passage");
        assert!(create(&mut app, &value).is_err());
        value["text"] = json!("world");
        value["revision"] = json!(u64::MAX);
        assert!(create(&mut app, &value).is_err());
        value["revision"] = json!(app.session.rev());
        value["selection"]["focus"]["off"] = json!(u64::MAX);
        assert!(create(&mut app, &value).is_err());
        let link = create_link(&mut app, 6, 11, "world", "Passage").unwrap();
        app.session.set_document(Document::from_text("Hello world"));
        assert!(navigate(&mut app, link["url"].as_str().unwrap()).is_err());
        app.chatgpt.clear();
        assert!(navigate(&mut app, link["url"].as_str().unwrap()).is_err());
        for url in [
            "wordcraft://document/1",
            "wordcraft://document/../../file",
            "javascript:alert(1)",
            "wordcraft://document/00000000000000000000000000000000?x=1",
        ] {
            assert!(token(url).is_none());
        }
        assert!(navigate(&mut app, "wordcraft://document/00000000000000000000000000000000").is_err());
    }
    #[test]
    fn unicode_ranges_and_reopened_reference_metadata_are_checked() {
        let mut original = app("Hello 世界");
        let link = create_link(&mut original, 6, 12, "世界", "Chinese passage").unwrap();
        let mut saved = serde_json::to_value(super::super::history::Archive::capture(&original.chatgpt)).unwrap();
        let mut restored = app("Hello 世界");
        let archive: super::super::history::Archive = serde_json::from_value(saved.clone()).unwrap();
        restored.chatgpt.links = archive.links;
        for link in &mut restored.chatgpt.links {
            link.reference.restore_document(restored.session.document_id());
        }
        navigate(&mut restored, link["url"].as_str().unwrap()).unwrap();
        assert_eq!(restored.session.selected_text(), "世界");
        saved.as_object_mut().unwrap().remove("links");
        let old: super::super::history::Archive = serde_json::from_value(saved).unwrap();
        assert!(old.links.is_empty(), "Older history files remain compatible");
    }
    #[test]
    fn markdown_links_click_inside_replies_without_opening_a_browser() {
        let mut app = app("Hello world");
        let link = create_link(&mut app, 6, 11, "world", "Jump to passage").unwrap();
        app.chatgpt.open = true;
        app.chatgpt.ready = true;
        app.chatgpt.account = Some("Preview account".into());
        app.chatgpt.push("ChatGPT", format!("A passage to review: {}.", link["markdown"].as_str().unwrap()), None);
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut WordApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        harness.run_steps(4);
        harness.get_by_label("Jump to passage").click();
        harness.run_steps(3);
        assert_eq!(harness.state().session.selected_text(), "world");
        assert_eq!(harness.state().chatgpt.messages.len(), 1, "Navigation must keep the transcript intact");
        harness.ctx.output(|o| assert!(!o.commands.iter().any(|command| matches!(command, egui::OutputCommand::OpenUrl(_)))));
        harness.state_mut().chatgpt.push("ChatGPT", "[Missing passage](wordcraft://document/00000000000000000000000000000000)".into(), None);
        harness.run_steps(4);
        harness.get_by_label("Missing passage").click();
        harness.run_steps(3);
        assert!(harness.state().chatgpt.error.as_deref().is_some_and(|error| error.contains("current conversation")));
        assert_eq!(harness.state().session.selected_text(), "world");
    }
}
