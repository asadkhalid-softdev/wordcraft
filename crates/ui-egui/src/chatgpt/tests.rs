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
fn stale_stopped_and_cross_document_tool_calls_cannot_edit() {
    let mut app = app();
    app.chatgpt.busy = true;
    app.chatgpt.thread = Some("thread".into());
    app.chatgpt.turn = Some("turn".into());
    app.chatgpt.document = Some(app.session.document_id());
    let mut call = json!({"threadId": "thread", "turnId": "turn", "tool": "wordcraft_execute", "arguments": {"command": "text.insert", "params": {"text": "safe"}}});
    app.chatgpt.stopping = true;
    assert!(tool_call(&mut app, &call).is_err());
    app.chatgpt.stopping = false;
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
    use egui_kittest::kittest::Queryable;
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
    // Disconnect the fake transport before Clear; this test must never launch a real client.
    harness.state_mut().chatgpt.ready = false;
    harness.get_by_label("Clear conversation").click();
    harness.run_steps(2);
    assert!(harness.state().chatgpt.messages.is_empty());
    assert_eq!(harness.state().session.doc.plain_text(StoryRef::Body), "Hello world");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn protocol_round_trip_from_account_to_edit_and_stop() {
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
    assert_eq!(start["params"]["dynamicTools"].as_array().unwrap().len(), 3);
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
    app.chatgpt.stop(&ctx).unwrap();
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
