use super::*;
use crate::Services;
use wordcraft_doc::{Document, StoryRef};
use wordcraft_engine::Session;

fn app() -> WordApp {
    WordApp::new(Session::new(Document::from_text("Hello world")), Services::default())
}

fn run(app: &mut WordApp, command: &str, params: Value) -> Result<Value, String> {
    execute_tool(app, "wordcraft_execute", &json!({"command": command, "params": params}))
}

#[test]
fn agent_discovers_formats_and_edits_with_normal_undo() {
    let mut app = app();
    let found = execute_tool(&mut app, "wordcraft_commands", &json!({"search": "format.bold"})).unwrap();
    assert!(found["items"].as_array().unwrap().iter().any(|c| c["id"] == "format.bold"));
    run(&mut app, "select.text", json!({"text": "Hello"})).unwrap();
    run(&mut app, "format.bold", json!({"value": true})).unwrap();
    assert_eq!(app.session.run("format.state", &json!({})).unwrap()["bold"], true);
    run(&mut app, "text.insert", json!({"text": "Goodbye"})).unwrap();
    assert_eq!(app.session.doc.plain_text(StoryRef::Body), "Goodbye world");
    run(&mut app, "edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.plain_text(StoryRef::Body), "Hello world");
    let inspect = execute_tool(&mut app, "wordcraft_inspect", &json!({"path": "/blocks", "limit": 1})).unwrap();
    assert_eq!(inspect["items"][0]["text"], "Hello world");
}

#[test]
fn all_engine_commands_are_discoverable_and_ui_recursion_is_refused() {
    let mut app = app();
    let mut ids = Vec::new();
    for offset in (0..app.session.registry.all().len() + crate::printing::commands(&app).len()).step_by(100) {
        let found = execute_tool(&mut app, "wordcraft_commands", &json!({"offset": offset, "limit": 100})).unwrap();
        ids.extend(found["items"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_owned()));
    }
    assert_eq!(ids.len(), app.session.registry.all().len() + crate::printing::commands(&app).len());
    assert!(ids.iter().any(|id| id == "ui.printSubmit"));
    assert!(run(&mut app, "ui.printSettings", json!({})).is_ok());
    for command in app.session.registry.all() {
        assert!(ids.iter().any(|id| id == command.id));
    }
    assert!(run(&mut app, "ui.chatgpt.send", json!({"text": "recurse"})).is_err());
    assert!(run(&mut app, "ui.dialog", json!({"name": "font"})).is_err());
    assert!(run(&mut app, "unknown.command", json!({})).is_err());
    assert!(execute_tool(&mut app, "wordcraft_execute", &Value::Null).is_err());
    assert!(run(&mut app, "format.bold", json!([])).is_err());
    run(&mut app, "text.insert", json!({"text": "Keep me "})).unwrap();
    for command in ["file.close", "file.new"] {
        assert!(run(&mut app, command, json!({})).is_err());
        assert!(run(&mut app, command, json!({"force": "true"})).is_err());
        assert!(app.session.doc.plain_text(StoryRef::Body).contains("Keep me"));
    }
    run(&mut app, "file.new", json!({"force": true})).unwrap();
    assert!(app.session.doc.plain_text(StoryRef::Body).is_empty());
}

#[test]
fn stale_inactive_and_cross_document_tool_calls_cannot_edit() {
    let mut app = app();
    app.chatgpt.busy = true;
    app.chatgpt.thread = Some("thread".into());
    app.chatgpt.turn = Some("turn".into());
    app.chatgpt.document = Some(app.session.document_id());
    let mut call = json!({"threadId": "thread", "turnId": "turn", "tool": "wordcraft_execute", "arguments": {"command": "text.insert", "params": {"text": "safe"}}});
    app.chatgpt.busy = false;
    assert!(tool_call(&mut app, &call).is_err());
    app.chatgpt.busy = true;
    call["turnId"] = json!("old-turn");
    assert!(tool_call(&mut app, &call).is_err());
    call["turnId"] = json!("turn");
    call["threadId"] = json!("old-thread");
    assert!(tool_call(&mut app, &call).is_err());
    call["threadId"] = json!("thread");
    app.session.run("file.new", &json!({})).unwrap();
    assert!(tool_call(&mut app, &call).is_err());
    assert!(app.session.doc.plain_text(StoryRef::Body).is_empty());
    poll(&mut app, &egui::Context::default());
    assert!(!app.chatgpt.busy);
    assert!(app.chatgpt.thread.is_none());
}

#[test]
fn streamed_replies_are_deduplicated_bounded_and_clearable() {
    let mut chat = State { busy: true, thread: Some("t".into()), ..Default::default() };
    let ctx = egui::Context::default();
    chat.notification(&ctx, "item/agentMessage/delta", &json!({"threadId": "other", "itemId": "i", "delta": "ignored"})).unwrap();
    assert!(chat.messages.is_empty());
    for text in ["Hello ", "world"] {
        chat.notification(&ctx, "item/agentMessage/delta", &json!({"threadId": "t", "itemId": "i", "delta": text})).unwrap();
    }
    chat.notification(&ctx, "item/completed", &json!({"threadId": "t", "item": {"id": "i", "type": "agentMessage", "text": "Hello world!"}}))
        .unwrap();
    assert_eq!(chat.messages.len(), 1);
    assert_eq!(chat.messages.first().unwrap().text, "Hello world!");
    chat.notification(&ctx, "turn/completed", &json!({"threadId": "t", "turn": {"status": "completed"}})).unwrap();
    assert!(!chat.busy);
    chat.clear();
    assert!(chat.messages.is_empty());
    assert!(chat.thread.is_none());
    let mut text = "ab".to_owned();
    append_bounded(&mut text, "😀xyz", 5);
    assert_eq!(text, "ab");
}

#[test]
fn hostile_pagination_and_outputs_are_bounded() {
    assert_eq!(page(json!([1, 2]), &json!({"offset": u64::MAX}))["items"], json!([]));
    assert_eq!(page(json!([1, 2]), &json!({"limit": u64::MAX}))["items"], json!([1, 2]));
    assert_eq!(page(json!("😀a"), &json!({"offset": 1}))["text"], "a");
    assert_eq!(tool_result(Ok(json!("x".repeat(MAX_OUTPUT + 1))))["success"], true);
    assert_eq!(tool_result(Err("failed".into()))["success"], false);
}

#[test]
fn sidebar_commands_collapse_without_clearing_and_clear_without_editing() {
    let mut app = app();
    app.session.run("view.focus", &json!({"value": true})).unwrap();
    app.execute("ui.chatgpt.toggle", json!({"value": true})).unwrap();
    assert!(!app.session.view.focus_mode, "opening chat leaves Focus mode so the pane is visible");
    app.chatgpt.push("You", "Message".into(), None);
    app.execute("ui.chatgpt.toggle", json!({"value": false})).unwrap();
    assert_eq!(app.chatgpt.messages.len(), 1);
    app.execute("ui.chatgpt.clear", json!({})).unwrap();
    assert!(app.chatgpt.messages.is_empty());
    assert_eq!(app.session.doc.plain_text(StoryRef::Body), "Hello world");
    assert!(app.execute("ui.chatgpt.send", json!({"text": "Edit"})).is_err());
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("user".into());
    assert!(app.chatgpt.submit(&egui::Context::default(), &"x".repeat(MAX_PROMPT + 1), app.session.document_id()).is_err());
    assert!(!app.chatgpt.busy);
}

#[test]
fn typing_in_chat_leaves_the_document_untouched_and_menu_clears_history() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut app = app();
    app.chatgpt.open = true;
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("test@example.com".into());
    app.chatgpt.push("You", "Earlier message".into(), None);
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut WordApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(4);
    harness.get_by_label("Message ChatGPT").focus();
    harness.run_steps(2);
    harness.event(egui::Event::Text("Revise this".into()));
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(harness.state().chatgpt.draft.starts_with("Revise this"));
    assert_eq!(harness.state().session.doc.plain_text(StoryRef::Body), "Hello world");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (client, outgoing) = desktop::Client::test_pair();
        harness.state_mut().chatgpt.client = Some(client);
        harness.event(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { ctrl: true, command: true, ..Default::default() },
        });
        harness.run_steps(2);
        assert_eq!(outgoing.try_recv().unwrap()["method"], "thread/start");
        assert!(harness.state().chatgpt.draft.is_empty());
        assert_eq!(harness.state().session.doc.plain_text(StoryRef::Body), "Hello world");
    }
    harness.get_by_label("Chat options").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("Stop").is_none());
    assert!(harness.query_by_label("Disconnect").is_none());
    assert!(!harness.get_by_label("Sign out").accesskit_node().is_disabled());
    assert!(!commands(harness.state()).iter().any(|command| command["id"] == "ui.chatgpt.stop"));
    // Disable the fake transport before Clear; this test must never launch a real client.
    harness.state_mut().chatgpt.ready = false;
    harness.get_by_label("Clear conversation").click();
    harness.run_steps(2);
    assert!(harness.state().chatgpt.messages.is_empty());
    assert_eq!(harness.state().session.doc.plain_text(StoryRef::Body), "Hello world");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn protocol_round_trip_from_account_to_edit_and_clear() {
    let mut app = app();
    let ctx = egui::Context::default();
    let (client, outgoing) = desktop::Client::test_pair();
    app.chatgpt.client = Some(client);
    app.chatgpt.request(&ctx, "account/read", json!({"refreshToken": false}), Request::Account).unwrap();
    let request = outgoing.try_recv().unwrap();
    receive(
        &mut app,
        &ctx,
        json!({"id": request["id"], "result": {"account": {"type": "chatgpt", "email": "test@example.com", "planType": "plus"}}}),
    )
    .unwrap();
    assert_eq!(app.chatgpt.account.as_deref(), Some("test@example.com"));
    app.chatgpt.submit(&ctx, "Insert Revised at the start", app.session.document_id()).unwrap();
    let start = outgoing.try_recv().unwrap();
    assert_eq!(start["method"], "thread/start");
    assert_eq!(start["params"]["ephemeral"], true);
    assert_eq!(start["params"]["dynamicTools"].as_array().unwrap().len(), 6);
    receive(&mut app, &ctx, json!({"id": start["id"], "result": {"thread": {"id": "thread"}}})).unwrap();
    let turn = outgoing.try_recv().unwrap();
    assert_eq!(turn["method"], "turn/start");
    receive(&mut app, &ctx, json!({"id": turn["id"], "result": {"turn": {"id": "turn"}}})).unwrap();
    let call = json!({"id": "tool-1", "method": "item/tool/call", "params": {
        "threadId": "thread", "turnId": "turn", "tool": "wordcraft_execute",
        "arguments": {"command": "text.insert", "params": {"text": "Revised "}}
    }});
    receive(&mut app, &ctx, call.clone()).unwrap();
    let reply = outgoing.try_recv().unwrap();
    assert_eq!(reply["id"], "tool-1");
    assert_eq!(reply["result"]["success"], true);
    assert_eq!(app.session.doc.plain_text(StoryRef::Body), "Revised Hello world");
    app.chatgpt.clear();
    assert!(app.chatgpt.messages.is_empty());
    assert!(app.chatgpt.pending.is_empty());
    assert!(app.chatgpt.thread.is_none());
    assert!(app.chatgpt.turn.is_none());
    assert!(app.chatgpt.client.is_none());
    assert!(tool_call(&mut app, &call["params"]).is_err());
    assert_eq!(app.session.doc.plain_text(StoryRef::Body), "Revised Hello world");
    receive(&mut app, &ctx, json!({"method": "turn/completed", "params": {"threadId": "thread", "turn": {"status": "interrupted"}}})).unwrap();
    assert!(!app.chatgpt.busy);
}

#[test]
fn settings_keep_credentials_out_of_status_and_preferences_and_clear_on_bad_import() {
    let mut app = app();
    app.execute("ui.chatgpt.settings", json!({})).unwrap();
    assert!(app.chatgpt.settings_open);
    app.chatgpt.auth_json = "not valid credential JSON".into();
    let status = app.execute("ui.chatgpt.status", json!({})).unwrap().to_string();
    assert!(!status.contains("not valid"));
    assert!(!serde_json::to_string(&app.ui).unwrap().contains("not valid"));
    let error = app.execute("ui.chatgpt.import", json!({"json": "must not be used"})).unwrap_err();
    assert!(!error.contains("not valid"));
    assert!(app.chatgpt.auth_json.is_empty());
    assert_eq!(app.session.doc.plain_text(StoryRef::Body), "Hello world");
}

#[test]
fn settings_popup_accepts_multiline_paste_without_editing_document_and_clears_on_close() {
    use egui_kittest::kittest::Queryable;
    let mut app = app();
    app.chatgpt.open = true;
    app.chatgpt.settings_open = true;
    app.chatgpt.advanced_auth = true;
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut WordApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(4);
    harness.get_by_label("Protected credential file JSON").focus();
    harness.run_steps(2);
    harness.event(egui::Event::Paste("{\n  \"example\": \"test-placeholder\"\n}".into()));
    harness.run_steps(2);
    assert!(serde_json::from_str::<Value>(&harness.state().chatgpt.auth_json).is_ok());
    assert_eq!(harness.state().session.doc.plain_text(StoryRef::Body), "Hello world");
    harness.get_by_label("Close").click();
    harness.run_steps(2);
    assert!(!harness.state().chatgpt.settings_open);
    assert!(harness.state().chatgpt.auth_json.is_empty());
}

#[test]
fn native_login_notification_opens_only_the_trusted_browser_url_and_cancel_resets_it() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.chatgpt.logging_in = true;
    assert!(app.chatgpt.notification(&ctx, "account/login/url", &json!({"url":"https://evil.test/"})).is_err());
    app.chatgpt.notification(&ctx, "account/login/url", &json!({"url":"https://auth.openai.com/api/accounts/authorize?state=test"})).unwrap();
    assert!(app.chatgpt.login_url.is_some());
    app.execute("ui.chatgpt.cancelLogin", json!({})).unwrap();
    assert!(!app.chatgpt.logging_in);
    assert!(app.chatgpt.login_url.is_none());
    app.chatgpt.notification(&ctx, "account/login/url", &json!({"url":"https://auth.openai.com/api/accounts/authorize"})).unwrap();
    assert!(app.chatgpt.login_url.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn browser_login_result_connects_automatically_and_preserves_document() {
    let mut app = app();
    let ctx = egui::Context::default();
    let (client, outgoing) = desktop::Client::test_pair();
    app.chatgpt.client = Some(client);
    app.chatgpt.logging_in = true;
    app.chatgpt.settings_open = true;
    app.chatgpt.request(&ctx, "account/login/start", json!({"newAccount": false}), Request::Login).unwrap();
    let request = outgoing.try_recv().unwrap();
    receive(&mut app, &ctx, json!({"id":request["id"],"result":{
        "account":{"type":"chatgpt","email":"ChatGPT account"},"models":[{"slug":"account-model","display_name":"Account model"}],"model":"account-model"
    }})).unwrap();
    assert!(app.chatgpt.ready);
    assert!(app.chatgpt.account.is_some());
    assert!(!app.chatgpt.logging_in);
    assert!(!app.chatgpt.settings_open);
    assert_eq!(app.chatgpt.model, "account-model");
    assert!(app.chatgpt.auth_json.is_empty());
    assert_eq!(app.session.doc.plain_text(StoryRef::Body), "Hello world");
}

#[test]
fn text_references_capture_navigate_relocate_and_reject_changed_text() {
    let mut app = app();
    assert!(app.execute("ui.chatgpt.attachSelection", json!({})).is_err());
    app.execute("select.text", json!({"text": "world"})).unwrap();
    let revision = app.session.rev();
    app.execute("ui.chatgpt.attachSelection", json!({})).unwrap();
    app.execute("ui.chatgpt.attachSelection", json!({})).unwrap();
    assert_eq!(app.chatgpt.references.len(), 1, "duplicate attachments are ignored");
    assert!(app.chatgpt.open && app.chatgpt.focus_composer);
    assert_eq!(app.session.rev(), revision);
    assert_eq!(app.chatgpt.references[0].text, "world");
    assert!(!app.chatgpt.references[0].label().contains("world"));
    app.execute("caret.docStart", json!({})).unwrap();
    app.execute("ui.chatgpt.showReference", json!({"id": 1})).unwrap();
    assert_eq!(app.session.selected_text(), "world");
    assert!(app.canvas.scroll_to_caret);
    assert_eq!(app.session.rev(), revision);
    app.execute("caret.docStart", json!({})).unwrap();
    app.execute("text.insert", json!({"text": "Intro: "})).unwrap();
    app.execute("ui.chatgpt.showReference", json!({"id": 1})).unwrap();
    assert_eq!(app.session.selected_text(), "world");
    assert_eq!(app.session.sel.anchor.off, 13);
    app.execute("text.insert", json!({"text": "changed"})).unwrap();
    assert!(app.execute("ui.chatgpt.showReference", json!({"id": 1})).is_err());
    assert!(app.execute("ui.chatgpt.send", json!({"text": "Rewrite it"})).is_err());
    assert_eq!(app.chatgpt.references.len(), 1);
    app.execute("ui.chatgpt.removeReference", json!({"id": 1})).unwrap();
    assert!(app.chatgpt.references.is_empty());
    assert!(app.execute("ui.chatgpt.removeReference", json!({"id": u64::MAX})).is_err());
    assert_eq!(app.session.doc.plain_text(StoryRef::Body), "Intro: Hello changed");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn sending_references_includes_hidden_context_and_preserves_navigable_history() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.execute("select.text", json!({"text": "world"})).unwrap();
    app.execute("ui.chatgpt.attachSelection", json!({})).unwrap();
    let (client, outgoing) = desktop::Client::test_pair();
    app.chatgpt.client = Some(client);
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("test@example.com".into());
    app.chatgpt.thread = Some("thread".into());
    app.execute("select.text", json!({"text": "Hello"})).unwrap();
    app.execute("ui.chatgpt.send", json!({"text": "Make this concise"})).unwrap();
    let sent = outgoing.try_recv().unwrap();
    let input = sent.pointer("/params/input/0/text").unwrap().as_str().unwrap();
    assert!(input.contains("Make this concise") && input.contains("untrusted context"));
    assert!(input.contains("world") && input.contains("\"off\":6"));
    assert!(app.chatgpt.references.is_empty());
    let message = app.chatgpt.messages.last().unwrap();
    assert_eq!(message.text, "Make this concise");
    assert_eq!(message.references[0].text, "world");
    app.execute("ui.chatgpt.showReference", json!({"id": 1})).unwrap();
    assert_eq!(app.session.selected_text(), "world");
    app.chatgpt.notification(&ctx, "turn/completed", &json!({"threadId": "thread", "turn": {"status": "completed"}})).unwrap();
    app.execute("file.new", json!({"force": true})).unwrap();
    poll(&mut app, &ctx);
    assert!(app.chatgpt.messages.is_empty() && app.chatgpt.references.is_empty());
    assert!(app.execute("ui.chatgpt.showReference", json!({"id": 1})).is_err());
}

#[test]
fn references_support_unicode_multiple_paragraphs_and_size_limits() {
    let mut app = WordApp::new(Session::new(Document::from_text("Before\nCafé αβ\nThird line\nAfter")), Services::default());
    app.execute("select.range", json!({"anchor": {"block": 1, "off": 0}, "focus": {"block": 2, "off": 5}})).unwrap();
    app.execute("ui.chatgpt.attachSelection", json!({})).unwrap();
    assert_eq!(app.chatgpt.references[0].text, "Café αβ\nThird");
    app.execute("caret.docStart", json!({})).unwrap();
    app.execute("text.insert", json!({"text": "Prefix "})).unwrap();
    app.execute("ui.chatgpt.showReference", json!({"id": 1})).unwrap();
    assert_eq!(app.session.selected_text(), "Café αβ\nThird");
    app.chatgpt.clear();
    assert!(app.chatgpt.references.is_empty());
    let mut big = WordApp::new(Session::new(Document::from_text(&"a".repeat(MAX_PROMPT + 1))), Services::default());
    big.execute("select.all", json!({})).unwrap();
    assert!(big.execute("ui.chatgpt.attachSelection", json!({})).is_err());
    assert!(big.chatgpt.references.is_empty());
}

#[test]
fn markdown_keeps_tables_code_and_math_but_blocks_images_and_unsafe_links() {
    let markdown = "# Heading\n\n**bold** *italic*\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```rust\nlet x = 1;\n```\n\n[Docs](https://example.com) ![private](file:///C:/secret.txt) [bad](javascript:alert)";
    let safe = markdown::safe_markdown(markdown);
    assert!(safe.contains("| A | B |") && safe.contains("```rust") && safe.contains("**bold**"));
    assert!(safe.contains("https://example.com"));
    assert!(!safe.contains("file://") && !safe.contains("javascript:"));
    assert!(safe.contains("private") && safe.contains("bad"));
    let math = markdown::normalize_math(r"Inline \(x^2\), display \[\frac{a}{b}\]. Code: `\(unchanged\)`");
    assert!(math.contains("$x^2$") && math.contains(r"$$\frac{a}{b}$$"));
    assert!(math.contains(r"`\(unchanged\)`"));
    assert_eq!(markdown::normalize_math("```tex\n\\[literal\\]\n```"), "```tex\n\\[literal\\]\n```");
}

#[test]
fn reference_links_select_text_and_hover_remove_leaves_document_untouched() {
    use egui_kittest::kittest::Queryable;
    let mut app = app();
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("test@example.com".into());
    app.execute("select.text", json!({"text": "world"})).unwrap();
    app.execute("ui.chatgpt.attachSelection", json!({})).unwrap();
    app.execute("select.collapse", json!({})).unwrap();
    let label = app.chatgpt.references[0].label();
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut WordApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(4);
    harness.get_by_label(&label).click();
    harness.run_steps(3);
    assert_eq!(harness.state().session.selected_text(), "world");
    harness.get_by_label("Remove reference").click();
    harness.run_steps(3);
    assert!(harness.state().chatgpt.references.is_empty());
    assert_eq!(harness.state().session.doc.plain_text(StoryRef::Body), "Hello world");
}

#[test]
#[ignore = "manual native GPU screenshot; no account or network required"]
fn reference_and_markdown_preview() {
    use egui_kittest::kittest::Queryable;
    let mut app = WordApp::new(Session::new(wordcraft_engine::sample::sample_document()), Services::default());
    app.integrated_titlebar = true;
    app.execute("select.text", json!({"text": "The Open Studio Handbook"})).unwrap();
    app.execute("ui.chatgpt.attachSelection", json!({})).unwrap();
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("Preview account".into());
    app.chatgpt.draft = "Make the selected title more engaging".into();
    app.chatgpt.push("ChatGPT", "## Suggested structure\n\nKeep the introduction **short** and use a clear list:\n\n- Welcome new members\n- Explain *shared responsibilities*\n\n| Section | Focus |\n| --- | --- |\n| Welcome | Getting started |\n| Studio | Equipment and care |\n\n```text\nA place to make things together\n```\n\nFor a formula: \\(x^2 + y^2 = z^2\\).".into(), None);
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 1000.0)).with_pixels_per_point(1.5).wgpu().build_ui_state(
        |ui, app: &mut WordApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    harness.input_mut().max_texture_side = Some(8192);
    harness.run_steps(8);
    harness.get_by_label("Selected text 1 · 4 words").hover();
    harness.run_steps(3);
    harness.render().unwrap().save(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/chatgpt-context-preview.png")).unwrap();
    harness.state_mut().canvas.mini_dismissed = None;
    harness.run_steps(3);
    harness.render().unwrap().save(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/chatgpt-selection-toolbar.png")).unwrap();
    harness.state_mut().execute("ui.contextMenu", json!({})).unwrap();
    harness.run_steps(3);
    harness.render().unwrap().save(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/chatgpt-context-menu.png")).unwrap();
    harness.key_press(egui::Key::Escape);
    harness.state_mut().execute("ui.chatgpt.settings", json!({})).unwrap();
    harness.run_steps(3);
    harness.render().unwrap().save(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/chatgpt-search-settings.png")).unwrap();
}

#[test]
fn references_enforce_attachment_count_and_refuse_ambiguous_relocation() {
    let mut app = WordApp::new(Session::new(Document::from_text("one two three four five six seven eight nine")), Services::default());
    for word in ["one", "two", "three", "four", "five", "six", "seven", "eight"] {
        app.execute("select.text", json!({"text": word})).unwrap();
        app.execute("ui.chatgpt.attachSelection", json!({})).unwrap();
    }
    app.execute("select.text", json!({"text": "nine"})).unwrap();
    assert!(app.execute("ui.chatgpt.attachSelection", json!({})).is_err());
    assert_eq!(app.chatgpt.references.len(), 8);
    app.chatgpt.clear();
    app.execute("select.text", json!({"text": "one two three four five six seven eight nine"})).unwrap();
    app.execute("ui.chatgpt.attachSelection", json!({})).unwrap();
    app.execute("caret.docStart", json!({})).unwrap();
    app.execute("text.insert", json!({"text": "one two three four five six seven eight nine\n"})).unwrap();
    assert!(app.execute("ui.chatgpt.showReference", json!({"id": 1})).is_err());
}

#[test]
fn assistant_markdown_renders_table_cells_and_headings_instead_of_source_markers() {
    use egui_kittest::kittest::Queryable;
    let mut app = app();
    app.chatgpt.open = true;
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("test@example.com".into());
    app.chatgpt.push(
        "ChatGPT",
        "## Suggested outline\n\n**Strong text**\n\n| Section | Focus |\n|---|---|\n| Welcome | Getting started |\n\n```rust\nlet count = 1;\n```"
            .into(),
        None,
    );
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 1000.0)).build_ui_state(
        |ui, app: &mut WordApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(4);
    assert!(harness.query_by_label("Suggested outline").is_some());
    assert!(harness.query_by_label("Section").is_some());
    assert!(harness.query_by_label("Getting started").is_some());
    assert!(harness.query_by_label("## Suggested outline").is_none());
    assert!(harness.query_by_label("| Section | Focus |").is_none());
}

#[test]
fn web_search_is_enabled_by_default_persisted_and_can_be_disabled_in_settings() {
    use egui_kittest::kittest::Queryable;
    let initial: crate::UiState = serde_json::from_str("{}").unwrap();
    assert!(initial.chatgpt_web_search);
    let mut app = app();
    app.execute("ui.chatgpt.settings", json!({})).unwrap();
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 1000.0)).build_ui_state(
        |ui, app: &mut WordApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(4);
    harness.get_by_label("Enable web search").click();
    harness.run_steps(3);
    assert!(!harness.state().ui.chatgpt_web_search);
    let saved = serde_json::to_string(&harness.state().ui).unwrap();
    let restored: crate::UiState = serde_json::from_str(&saved).unwrap();
    assert!(!restored.chatgpt_web_search);
    assert!(harness.state_mut().execute("ui.chatgpt.webSearch", json!({"value": "yes"})).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn next_turn_receives_the_saved_search_setting_and_file_authority_uses_only_user_chat() {
    let mut app = app();
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("reference.txt");
    std::fs::write(&file, "Separate reference content").unwrap();
    let path = file.to_str().unwrap();
    assert!(execute_tool(&mut app, "wordcraft_read_file", &json!({"path": path})).is_err());
    app.chatgpt.push("ChatGPT", format!("Read `{path}`"), None);
    assert!(execute_tool(&mut app, "wordcraft_read_file", &json!({"path": path})).is_err());
    app.chatgpt.push("You", format!("Please summarize `{path}`"), None);
    let authorization = execute_tool(&mut app, "wordcraft_read_file", &json!({"path": path})).unwrap();
    let revision = app.session.rev();
    let document = app.session.document_id();
    let content = files::read(&authorization, &json!({}), &std::sync::atomic::AtomicBool::new(false)).unwrap();
    assert_eq!(content["text"], "Separate reference content");
    assert_eq!(app.session.document_id(), document);
    assert_eq!(app.session.rev(), revision);
    assert_eq!(app.session.doc.plain_text(StoryRef::Body), "Hello world");
    let (client, outgoing) = desktop::Client::test_pair();
    app.chatgpt.client = Some(client);
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("test@example.com".into());
    app.chatgpt.thread = Some("thread".into());
    app.execute("ui.chatgpt.webSearch", json!({"value": false})).unwrap();
    app.execute("ui.chatgpt.send", json!({"text": "Summarize the reference"})).unwrap();
    assert_eq!(outgoing.try_recv().unwrap()["params"]["webSearch"], false);
    app.chatgpt.clear();
    assert!(execute_tool(&mut app, "wordcraft_read_file", &json!({"path": path})).is_err());
}

#[test]
fn whole_document_inspection_cannot_bypass_chat_context_limits() {
    let mut app = WordApp::new(Session::new(Document::from_text(&"secret body ".repeat(100000))), Services::default());
    for tool in ["wordcraft_inspect", "wordcraft_execute"] {
        let result = execute_tool(&mut app, tool, &json!({"command":"document.inspect"})).unwrap();
        assert!(result.to_string().len() < 4000);
        assert!(!result.to_string().contains("secret body"));
    }
    let result = execute_tool(&mut app, "wordcraft_document", &json!({"mode":"read","path":[0]})).unwrap();
    assert!(result["next"].is_object());
    assert!(result["text"].as_str().unwrap().len() <= 2048);
    let nested = tool_result(Ok(json!({"blocks":[{"nested":"x".repeat(100000)}]})));
    assert_eq!(nested["success"], true);
    assert!(nested.to_string().len() < 1000);
}

#[test]
fn streamed_reply_keeps_sidebar_header_and_composer_on_screen() {
    use egui_kittest::kittest::Queryable;
    let mut app = app();
    app.chatgpt.open = true;
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("test@example.com".into());
    app.chatgpt.busy = true;
    app.chatgpt.thread = Some("t".into());
    app.chatgpt.document = Some(app.session.document_id());
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut WordApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    for text in [
        "A short answer.".to_owned(),
        format!("\n\n```text\n{}\n```", "unbroken_text_".repeat(200)),
        format!("\n\n| {} |\n| {} |\n| {} |", vec!["Column"; 32].join(" | "), vec!["---"; 32].join(" | "), vec!["Content"; 32].join(" | ")),
    ] {
        harness
            .state_mut()
            .chatgpt
            .notification(&egui::Context::default(), "item/agentMessage/delta", &json!({"threadId":"t","itemId":"reply","delta":text}))
            .unwrap();
        harness.run_steps(5);
        assert!(harness.state().chatgpt.open);
        for label in ["Chat options", "Message ChatGPT"] {
            let rect = harness.get_by_label(label).rect();
            assert!(rect.min.x >= 0.0 && rect.max.x <= 1440.0 && rect.min.y >= 0.0 && rect.max.y <= 900.0, "{label}: {rect:?}");
        }
    }
}

#[test]
fn active_chat_survives_agent_focus_mode_and_can_be_reopened() {
    use egui_kittest::kittest::Queryable;
    let mut app = app();
    app.chatgpt.open = true;
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("test@example.com".into());
    app.chatgpt.busy = true;
    app.chatgpt.thread = Some("t".into());
    app.chatgpt.turn = Some("turn".into());
    app.chatgpt.document = Some(app.session.document_id());
    tool_call(
        &mut app,
        &json!({"threadId":"t","turnId":"turn","tool":"wordcraft_execute", "arguments":{"command":"view.focus","params":{"value":true}}}),
    )
    .unwrap();
    assert!(app.session.view.focus_mode);
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut WordApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    assert!(harness.query_by_label("Chat options").is_some(), "Focus mode must keep the active chat reachable");
    harness
        .state_mut()
        .chatgpt
        .notification(
            &egui::Context::default(),
            "item/agentMessage/delta",
            &json!({"threadId":"t","itemId":"reply","delta":"The document says hello."}),
        )
        .unwrap();
    harness.run_steps(3);
    assert!(harness.query_by_label("Message ChatGPT").is_some());
    harness.state_mut().execute("ui.chatgpt.toggle", json!({"value":false})).unwrap();
    harness.run_steps(2);
    assert!(harness.query_by_label("Chat options").is_none());
    harness.get_by_role_and_label(egui::accesskit::Role::Button, "ChatGPT").click();
    harness.run_steps(3);
    assert!(!harness.state().session.view.focus_mode);
    assert!(harness.query_by_label("Chat options").is_some());
    assert!(harness.state().chatgpt.messages.iter().any(|m| m.text == "The document says hello."));
}

#[test]
#[ignore = "manual native GPU screenshot; no account or network required"]
fn sidebar_streaming_layout_preview() {
    let mut app = app();
    app.integrated_titlebar = true;
    app.chatgpt.open = true;
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("Preview account".into());
    app.chatgpt.busy = true;
    app.chatgpt.activity = "Reading document…".into();
    let citation_args = json!({"documentId":app.session.document_id(),"revision":app.session.rev(),
        "selection":{"anchor":{"story":"body","path":[0],"off":0},"focus":{"story":"body","path":[0],"off":5}},
        "text":"Hello","label":"Open cited passage"});
    let citation = links::create(&mut app, &citation_args).unwrap();
    app.chatgpt.push(
        "ChatGPT",
        format!(
            "## Document answer\n\nA wide table stays inside the chat history.\n\n{}\n\n| {} |\n| {} |\n| {} |",
            citation["markdown"].as_str().unwrap(),
            vec!["Column"; 24].join(" | "),
            vec!["---"; 24].join(" | "),
            vec!["Document context"; 24].join(" | ")
        ),
        None,
    );
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
        |ui, app: &mut WordApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    harness.render().unwrap().save(root.join("chatgpt-sidebar-wide-reply.png")).unwrap();
    harness.state_mut().session.run("view.focus", &json!({"value":true})).unwrap();
    harness.run_steps(4);
    harness.render().unwrap().save(root.join("chatgpt-sidebar-focus-reply.png")).unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn assistant_can_create_verified_document_links_through_its_tool_protocol() {
    let mut app = app();
    let ctx = egui::Context::default();
    let (client, outgoing) = desktop::Client::test_pair();
    app.chatgpt.client = Some(client);
    app.chatgpt.ready = true;
    app.chatgpt.account = Some("test@example.com".into());
    app.chatgpt.submit(&ctx, "Explain this passage with a clickable source", app.session.document_id()).unwrap();
    let start = outgoing.try_recv().unwrap();
    assert!(start["params"]["dynamicTools"].as_array().unwrap().iter().any(|tool| tool["name"] == "wordcraft_reference"));
    receive(&mut app, &ctx, json!({"id":start["id"],"result":{"thread":{"id":"thread"}}})).unwrap();
    let turn = outgoing.try_recv().unwrap();
    receive(&mut app, &ctx, json!({"id":turn["id"],"result":{"turn":{"id":"turn"}}})).unwrap();
    let selection = app.session.sel.clone();
    let revision = app.session.rev();
    let arguments = json!({"documentId":app.session.document_id(),"revision":revision,"selection":{
        "anchor":{"story":"body","path":[0],"off":6},"focus":{"story":"body","path":[0],"off":11}},"text":"world","label":"Source passage"});
    receive(
        &mut app,
        &ctx,
        json!({"id":"cite-1","method":"item/tool/call","params":{
        "threadId":"thread","turnId":"turn","tool":"wordcraft_reference","arguments":arguments}}),
    )
    .unwrap();
    let reply = outgoing.try_recv().unwrap();
    assert_eq!(reply["result"]["success"], true);
    let citation: Value = serde_json::from_str(reply["result"]["contentItems"][0]["text"].as_str().unwrap()).unwrap();
    assert!(citation["markdown"].as_str().unwrap().starts_with("[Source passage](wordcraft://document/"));
    assert_eq!(app.session.sel, selection);
    assert_eq!(app.session.rev(), revision);
    receive(
        &mut app,
        &ctx,
        json!({"method":"item/completed","params":{"threadId":"thread","item":{
        "id":"answer","type":"agentMessage","text":format!("See {}.",citation["markdown"].as_str().unwrap())}}}),
    )
    .unwrap();
    app.execute("ui.chatgpt.showLink", json!({"url":citation["url"]})).unwrap();
    assert_eq!(app.session.selected_text(), "world");
}

#[test]
fn markdown_preserves_only_strict_document_destinations_with_normal_web_links() {
    let url = "wordcraft://document/0123456789abcdef0123456789abcdef";
    let source = format!(
        "| Passage | Source |\n| --- | --- |\n| Quote | [See section]({url}) |\n\n[reference][passage]\n\n[passage]: {url}\n\n[Website](https://example.com)"
    );
    let safe = markdown::safe_markdown(&source);
    assert!(safe.contains(url));
    assert!(safe.contains("https://example.com"));
    for destination in ["wordcraft://document/1", "wordcraft://execute/text.delete", "file:///private", "javascript:alert(1)"] {
        let source = format!("[Passage](<{destination}>)");
        let safe = markdown::safe_markdown(&source);
        assert!(!safe.contains(destination), "{safe}");
        assert!(safe.contains("Passage"));
    }
}
