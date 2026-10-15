//! Background direct Responses transport, using the reference integration's OAuth JSON.
//! The small internal event protocol keeps document actions on the UI thread.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::time::Duration;

use super::{auth, inference};
use serde_json::{Value, json};

pub(super) struct Client {
    outgoing: SyncSender<Value>,
    pub incoming: Receiver<Result<Value, String>>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Client {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
impl Client {
    #[cfg(test)]
    pub(super) fn test_pair() -> (Self, Receiver<Value>) {
        let (outgoing, writes) = mpsc::sync_channel(32);
        let (_events, incoming) = mpsc::sync_channel(64);
        (Self { outgoing, incoming, cancelled: Arc::new(AtomicBool::new(false)) }, writes)
    }
    pub fn start(model: &str, ctx: egui::Context) -> Result<Self, String> {
        let model = model.to_owned();
        let (outgoing, writes) = mpsc::sync_channel(32);
        let (events, incoming) = mpsc::sync_channel(64);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stopped = cancelled.clone();
        std::thread::Builder::new()
            .name("wordcraft-chatgpt".into())
            .spawn(move || {
                let worker = Worker {
                    http: auth::http(),
                    events,
                    writes,
                    ctx,
                    stopped,
                    model,
                    tools: json!([]),
                    history: Vec::new(),
                    serial: 0,
                    web_search: true,
                };
                worker.run();
            })
            .map_err(|_| "Could not start the ChatGPT connection")?;
        Ok(Self { outgoing, incoming, cancelled })
    }
    pub fn send(&self, value: Value) -> Result<(), String> {
        self.outgoing.try_send(value).map_err(|_| "ChatGPT connection is unavailable or busy. Reconnect and try again.".into())
    }
}
struct Worker {
    http: Result<reqwest::blocking::Client, String>,
    events: SyncSender<Result<Value, String>>,
    writes: Receiver<Value>,
    ctx: egui::Context,
    stopped: Arc<AtomicBool>,
    model: String,
    tools: Value,
    history: Vec<Value>,
    serial: u64,
    web_search: bool,
}
impl Worker {
    fn emit(&self, value: Value) -> Result<(), String> {
        if self.stopped.load(Ordering::Relaxed) {
            return Err("Reply stopped".into());
        }
        self.events.send(Ok(value)).map_err(|_| "ChatGPT sidebar disconnected")?;
        self.ctx.request_repaint();
        Ok(())
    }
    fn notice(&self, method: &str, params: Value) -> Result<(), String> {
        self.emit(json!({"method": method, "params": params}))
    }
    fn recv(&self) -> Result<Value, String> {
        loop {
            if self.stopped.load(Ordering::Relaxed) {
                return Err("Reply stopped".into());
            }
            match self.writes.recv_timeout(Duration::from_millis(50)) {
                Ok(v) => return Ok(v),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err("ChatGPT sidebar disconnected".into()),
            }
        }
    }
    fn run(mut self) {
        while let Ok(value) = self.recv() {
            if value.get("method").is_none() {
                continue;
            }
            let id = value.get("id").cloned();
            let result = self.handle(&value);
            if let Some(id) = id.filter(|_| result.is_err() || v_method(&value) != "turn/start") {
                let response = match result {
                    Ok(result) => json!({"id": id, "result": result}),
                    Err(error) => json!({"id": id, "error": {"message": error}}),
                };
                if self.emit(response).is_err() {
                    break;
                }
            }
        }
    }
    fn choose_model(&mut self, account: &mut Value) {
        let models = account.get("models").and_then(Value::as_array);
        if !models.is_some_and(|models| models.iter().any(|m| m.get("slug").and_then(Value::as_str) == Some(self.model.as_str()))) {
            self.model = models.and_then(|m| m.first()).and_then(|m| m.get("slug")).and_then(Value::as_str).unwrap_or("").into();
        }
        account["model"] = json!(self.model);
    }
    fn handle(&mut self, v: &Value) -> Result<Value, String> {
        let http = self.http.as_ref().map_err(Clone::clone)?.clone();
        let p = v.get("params").cloned().unwrap_or(json!({}));
        match v.get("method").and_then(Value::as_str).unwrap_or("") {
            "initialize" | "initialized" => Ok(json!({})),
            "account/read" => {
                let mut account = auth::account(&http)?;
                self.choose_model(&mut account);
                Ok(account)
            }
            "account/import" => {
                let mut raw = zeroize::Zeroizing::new(p.get("json").and_then(Value::as_str).ok_or("Missing credential JSON")?.to_owned());
                let result = auth::import(&http, &raw);
                use zeroize::Zeroize;
                raw.zeroize();
                let mut account = result?;
                self.choose_model(&mut account);
                Ok(account)
            }
            "account/login/start" => {
                let new_account = p.get("newAccount").and_then(Value::as_bool).unwrap_or(false);
                if !new_account
                    && let Ok(mut account) = auth::account(&http)
                    && !account["account"].is_null()
                {
                    self.choose_model(&mut account);
                    return Ok(account);
                }
                let mut account = auth::login::connect(&http, p.get("newAccount").and_then(Value::as_bool).unwrap_or(false), &self.stopped, |url| {
                    self.notice("account/login/url", json!({"url": url.as_str()}))
                })?;
                self.choose_model(&mut account);
                Ok(account)
            }
            "account/logout" => auth::logout(&http),
            "thread/start" => {
                self.history.clear();
                self.tools = inference::tools(p.get("dynamicTools").unwrap_or(&json!([])))?;
                Ok(json!({"thread": {"id": "document-chat"}}))
            }
            "turn/start" => {
                self.web_search = p.get("webSearch").and_then(Value::as_bool).unwrap_or(true);
                let prompt = p.pointer("/input/0/text").and_then(Value::as_str).ok_or("Missing chat prompt")?;
                self.serial = self.serial.checked_add(1).ok_or("Reconnect to start a new chat")?;
                let turn = format!("turn-{}", self.serial);
                self.emit(json!({"id": v.get("id"), "result": {"turn": {"id": turn}}}))?;
                // Announce ownership before any queued tool request can reach the UI.
                self.notice("turn/started", json!({"threadId": "document-chat", "turn": {"id": turn}}))?;
                self.history.push(json!({"role": "user", "content": prompt}));
                let result = self.turn(&http, &turn).map_err(|error| {
                    self.history.clear();
                    format!("{error} Conversation context was cleared; the next message starts a new conversation.")
                });
                self.notice(
                    "turn/completed",
                    json!({"threadId": "document-chat", "turn": {"id": turn, "error": result.err().map(|message| json!({"message": message}))}}),
                )?;
                Ok(json!({"turn": {"id": turn}}))
            }
            _ => Err("Unsupported ChatGPT operation".into()),
        }
    }
    fn turn(&mut self, http: &reqwest::blocking::Client, turn: &str) -> Result<(), String> {
        let mut calls = 0usize;
        for round in 0..16 {
            if self.stopped.load(Ordering::Relaxed) {
                return Err("Reply stopped".into());
            }
            let token = auth::access(http)?.ok_or("Import ChatGPT credentials in ChatGPT settings first")?;
            let output = inference::response(
                http,
                &token,
                &self.model,
                &self.history,
                inference::ToolOptions { definitions: &self.tools, web_search: self.web_search },
                &self.stopped,
                |event| {
                    if event.get("type").and_then(Value::as_str) == Some("response.output_text.delta") {
                        self.notice(
                            "item/agentMessage/delta",
                            json!({"threadId": "document-chat", "itemId": format!("{turn}-{round}"), "delta": event.get("delta")}),
                        )?;
                    } else if let Some(kind) = event.get("type").and_then(Value::as_str) {
                        self.notice(kind, json!({"threadId": "document-chat"}))?;
                    }
                    Ok(())
                },
            )?;
            let mut text = String::new();
            for item in &output {
                if item.get("type").and_then(Value::as_str) == Some("message") {
                    for part in item.get("content").and_then(Value::as_array).into_iter().flatten() {
                        if part.get("type").and_then(Value::as_str) == Some("output_text") {
                            super::append_bounded(&mut text, &super::citations::text(part), super::MAX_TEXT);
                        }
                    }
                }
            }
            if !text.is_empty() {
                self.notice(
                    "item/completed",
                    json!({"threadId": "document-chat", "item": {"type": "agentMessage", "id": format!("{turn}-{round}"), "text": text}}),
                )?;
            }
            let function_calls: Vec<_> =
                output.iter().filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call")).cloned().collect();
            self.history.extend(output);
            if function_calls.is_empty() {
                return Ok(());
            }
            for call in function_calls {
                calls = calls.saturating_add(1);
                if calls > 32 {
                    return Err("This reply reached its document-action limit. Review the document before continuing.".into());
                }
                let name = call.get("name").and_then(Value::as_str).ok_or("Invalid document tool name")?;
                let call_id = call.get("call_id").and_then(Value::as_str).ok_or("Invalid document tool call")?;
                let args: Value = serde_json::from_str(call.get("arguments").and_then(Value::as_str).ok_or("Invalid document tool arguments")?)
                    .map_err(|_| "Invalid document tool arguments")?;
                let id = format!("{turn}-{round}-{calls}");
                self.emit(json!({"id": id, "method": "item/tool/call", "params": {
                    "threadId": "document-chat", "turnId": turn, "tool": name, "arguments": args
                }}))?;
                let result = self.recv()?;
                if result.get("id").and_then(Value::as_str) != Some(id.as_str()) || result.get("result").is_none() {
                    return Err("Document tool connection was interrupted".into());
                }
                let mut result = result.get("result").cloned().ok_or("Missing document tool result")?;
                if name == "wordcraft_read_file" && result.get("success").and_then(Value::as_bool) == Some(true) {
                    let read = result
                        .pointer("/contentItems/0/text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "Invalid file authorization".to_owned())
                        .and_then(|text| serde_json::from_str::<Value>(text).map_err(|_| "Invalid file authorization".to_owned()))
                        .and_then(|authorization| super::files::read(&authorization, &args, &self.stopped));
                    // Keep actionable sanitized errors from the reader, but never print file contents.
                    result = super::tool_result(read);
                    self.notice("item/file/completed", json!({"threadId": "document-chat", "success": result.get("success")}))?;
                }
                self.history.push(json!({"type": "function_call_output", "call_id": call_id, "output": result.to_string()}));
            }
        }
        Err("This reply reached its action-round limit. Review the document before continuing.".into())
    }
}

fn v_method(v: &Value) -> &str {
    v.get("method").and_then(Value::as_str).unwrap_or("")
}
