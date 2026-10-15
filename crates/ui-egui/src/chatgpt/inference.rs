//! Bounded Responses SSE. Tool calls are provisional until response.completed.
use super::{INSTRUCTIONS, auth};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_RESPONSE: usize = 2_000_000;
const MAX_INPUT: usize = 256_000;
pub(super) const CONTEXT_LIMIT: &str = "ChatGPT context limit reached";

pub(super) fn tools(dynamic: &Value) -> Result<Value, String> {
    let items = dynamic.as_array().ok_or("Invalid document tools")?;
    let functions: Vec<Value> = items
        .iter()
        .take(8)
        .map(|tool| {
            json!({
                "type": "function", "name": tool.get("name"), "description": tool.get("description"),
                "parameters": tool.get("inputSchema"), "strict": false
            })
        })
        .collect();
    Ok(
        json!([{"type": "namespace", "name": "wordcraft", "description": "WordCraft document commands and user-referenced file reads", "tools": functions}]),
    )
}

fn payload(model: &str, history: &[Value], tools: &Value, web_search: bool) -> Result<Value, String> {
    let mut tools = tools.as_array().ok_or("Invalid document tools")?.clone();
    if web_search {
        tools.push(json!({"type": "web_search"}));
    }
    Ok(json!({"model": model, "instructions": INSTRUCTIONS, "input": history, "tools": tools,
        "store": false, "stream": true, "parallel_tool_calls": false, "include": ["reasoning.encrypted_content"]}))
}

pub(super) fn input_size(model: &str, history: &[Value], tools: &Value, web_search: bool) -> Result<usize, String> {
    serde_json::to_vec(&payload(model, history, tools, web_search)?).map(|b| b.len()).map_err(|_| "Could not encode conversation".into())
}

pub(super) struct ToolOptions<'a> {
    pub definitions: &'a Value,
    pub web_search: bool,
}

pub(super) fn response(
    http: &reqwest::blocking::Client,
    token: &str,
    model: &str,
    history: &[Value],
    options: ToolOptions<'_>,
    stopped: &AtomicBool,
    delta: impl FnMut(&Value) -> Result<(), String>,
) -> Result<Vec<Value>, String> {
    let payload = payload(model, history, options.definitions, options.web_search)?;
    let bytes = serde_json::to_vec(&payload).map_err(|_| "Could not encode the conversation")?;
    if bytes.len() > MAX_INPUT {
        return Err(
            "This message exceeds the request budget. Send a shorter message or attach a smaller excerpt; the conversation is preserved.".into()
        );
    }
    let result = http
        .post(format!("{}/responses", auth::API))
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body(bytes)
        .send()
        .map_err(|_| "Could not reach ChatGPT. Check the document before retrying; earlier actions may have completed.")?;
    if result.status().as_u16() != 200 {
        let status = result.status().as_u16();
        // Read only a bounded error body and expose a stable classification, never server text.
        if status == 400 || status == 413 {
            let mut bytes = Vec::new();
            use std::io::Read;
            let _ = result.take(8192).read_to_end(&mut bytes);
            if let Ok(error) = serde_json::from_slice::<Value>(&bytes)
                && matches!(
                    error.pointer("/error/code").and_then(Value::as_str),
                    Some("context_length_exceeded" | "context_window_exceeded" | "input_too_long")
                )
            {
                return Err(CONTEXT_LIMIT.into());
            }
        }
        let error = auth::status_error(status);
        return Err(if options.web_search && status == 400 {
            format!("{error} Try disabling web search in ChatGPT settings or selecting another model.")
        } else {
            error
        });
    }
    stream(BufReader::new(result), stopped, delta)
}

fn stream(mut reader: impl BufRead, stopped: &AtomicBool, mut delta: impl FnMut(&Value) -> Result<(), String>) -> Result<Vec<Value>, String> {
    let mut total = 0usize;
    let mut data = String::new();
    let mut provisional = BTreeMap::new();
    loop {
        if stopped.load(Ordering::Relaxed) {
            return Err("Reply stopped".into());
        }
        let line =
            read_line(&mut reader, &mut total)?.ok_or("ChatGPT disconnected before completing the reply. Review the document before retrying.")?;
        if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim_start_matches(' '));
            continue;
        }
        if !line.is_empty() || data.is_empty() {
            continue;
        }
        let raw = std::mem::take(&mut data);
        if raw == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(&raw).map_err(|_| "ChatGPT returned an invalid event")?;
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "response.output_text.delta"
            | "response.web_search_call.in_progress"
            | "response.web_search_call.searching"
            | "response.web_search_call.completed" => delta(&event)?,
            "response.output_item.done" => {
                let index = event.get("output_index").and_then(Value::as_u64).filter(|n| *n < 128).ok_or("Invalid streamed output index")?;
                let item = event
                    .get("item")
                    .filter(|i| i.is_object() && i.get("type").and_then(Value::as_str).is_some())
                    .ok_or("Invalid streamed output item")?;
                if provisional.get(&index).is_some_and(|existing| existing != item) {
                    return Err("Conflicting streamed output items".into());
                }
                provisional.insert(index, item.clone());
            }
            "response.completed" => {
                let response = event.get("response").ok_or("Invalid completed response")?;
                if response.get("status").and_then(Value::as_str) != Some("completed") {
                    return Err("ChatGPT did not complete the reply".into());
                }
                let output = response.get("output").and_then(Value::as_array).ok_or("Invalid completed output")?;
                if output.len() > 128 {
                    return Err("Too many response items".into());
                }
                let items = if output.is_empty() && !provisional.is_empty() {
                    if provisional.keys().copied().ne(0..provisional.len() as u64) {
                        return Err("ChatGPT returned incomplete streamed output".into());
                    }
                    provisional.into_values().collect()
                } else {
                    output.clone()
                };
                if items.iter().any(|item| {
                    !item.is_object()
                        || !matches!(item.get("type").and_then(Value::as_str), Some("message" | "reasoning" | "function_call" | "web_search_call"))
                }) {
                    return Err("ChatGPT returned an unsupported output item".into());
                }
                return Ok(items);
            }
            "error" | "response.failed" | "response.incomplete" => return Err(stream_error(&event)),
            _ => {}
        }
    }
}

fn stream_error(event: &Value) -> String {
    let code = event
        .pointer("/response/error/code")
        .or_else(|| event.pointer("/error/code"))
        .or_else(|| event.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("");
    match code {
        "context_length_exceeded" | "context_window_exceeded" | "input_too_long" => CONTEXT_LIMIT,
        "subscription_sharing_usage_limit_exceeded" | "rate_limit_exceeded" | "insufficient_quota" => {
            "ChatGPT usage limit reached. Review ChatGPT Settings → Usage."
        }
        "subscription_sharing_user_not_eligible" | "chatpass_v2_scope_not_authorized" => {
            "This account or workspace does not allow ChatGPT plan access."
        }
        "model_not_found" | "unsupported_model" => "The selected model is unavailable. Choose another account model in ChatGPT settings.",
        _ => "ChatGPT did not complete the reply. Review the document before retrying; earlier actions may have completed.",
    }
    .into()
}

fn read_line(reader: &mut impl BufRead, total: &mut usize) -> Result<Option<String>, String> {
    let mut line = Vec::new();
    loop {
        let chunk = reader.fill_buf().map_err(|_| "ChatGPT response was interrupted")?;
        if chunk.is_empty() {
            return if line.is_empty() { Ok(None) } else { Err("ChatGPT returned an unterminated stream".into()) };
        }
        let count = chunk.iter().position(|b| *b == b'\n').map(|n| n + 1).unwrap_or(chunk.len());
        if count > MAX_RESPONSE.saturating_sub(*total) {
            return Err("ChatGPT response exceeded the size limit".into());
        }
        if let Some(part) = chunk.get(..count) {
            line.extend_from_slice(part);
        }
        *total = total.saturating_add(count);
        let ended = line.last() == Some(&b'\n');
        reader.consume(count);
        if ended {
            let line = String::from_utf8(line).map_err(|_| "ChatGPT returned invalid UTF-8")?;
            return Ok(Some(line.trim_end_matches(['\r', '\n']).into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(events: &[Value]) -> Result<Vec<Value>, String> {
        let text: String = events.iter().map(|event| format!("data: {event}\n\n")).collect();
        stream(text.as_bytes(), &AtomicBool::new(false), |_| Ok(()))
    }
    #[test]
    fn context_rejection_is_classified_without_exposing_provider_text() {
        let event = json!({"type":"response.failed","response":{"error":{"code":"context_length_exceeded","message":"private echoed input"}}});
        assert_eq!(parse(&[event]).unwrap_err(), CONTEXT_LIMIT);
    }
    #[test]
    fn completed_stream_required_before_document_actions() {
        let item = json!({"type": "function_call", "name": "wordcraft_execute", "call_id": "c", "arguments": "{}"});
        let provisional = json!({"type": "response.output_item.done", "output_index": 0, "item": item});
        assert!(parse(std::slice::from_ref(&provisional)).is_err());
        let failed = json!({"type": "response.failed", "response": {"error": {"code": "subscription_sharing_usage_limit_exceeded", "message": "sensitive provider text"}}});
        let error = parse(&[provisional.clone(), failed]).unwrap_err();
        assert!(error.contains("usage limit"));
        assert!(!error.contains("sensitive"));
        let completed = json!({"type": "response.completed", "response": {"status": "completed", "output": []}});
        assert_eq!(parse(&[provisional.clone(), completed.clone()]).unwrap(), vec![item]);
        let mut gap = provisional;
        gap["output_index"] = json!(1);
        assert!(parse(&[gap, completed]).is_err());
    }
    #[test]
    fn stream_bounds_allocations_handles_chunk_boundaries_and_stop() {
        let text = b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\r\n\r\n";
        assert!(stream(BufReader::with_capacity(1, &text[..]), &AtomicBool::new(false), |_| Ok(())).is_ok());
        assert!(stream(&vec![b'x'; MAX_RESPONSE + 1][..], &AtomicBool::new(false), |_| Ok(())).is_err());
        assert!(stream(&text[..], &AtomicBool::new(true), |_| Ok(())).is_err());
        assert!(stream(&b"data: invalid\n\n"[..], &AtomicBool::new(false), |_| Ok(())).is_err());
        assert!(stream(&b"data: \xff\n\n"[..], &AtomicBool::new(false), |_| Ok(())).is_err());
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    #[test]
    fn search_toggle_controls_payload_and_preserves_all_namespaced_functions() {
        let functions = tools(&super::super::tool_definitions()).unwrap();
        let enabled = payload("account-model", &[], &functions, true).unwrap();
        assert_eq!(enabled["tools"][0]["type"], "namespace");
        assert_eq!(enabled["tools"][0]["tools"].as_array().unwrap().len(), 5);
        assert_eq!(enabled["tools"][1]["type"], "web_search");
        assert_eq!(enabled["store"], false);
        assert_eq!(enabled["stream"], true);
        let disabled = payload("account-model", &[], &functions, false).unwrap();
        assert_eq!(disabled["tools"].as_array().unwrap().len(), 1);
        assert_eq!(disabled["model"], "account-model");
    }

    #[test]
    fn completed_search_items_survive_stream_and_search_progress_is_forwarded() {
        let search = json!({"type": "web_search_call", "id": "search", "status": "completed", "action": {"type": "search", "query": "test"}});
        let events = [
            json!({"type": "response.web_search_call.searching"}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": [search.clone()]}}),
        ];
        let data: String = events.iter().map(|event| format!("data: {event}\n\n")).collect();
        let mut progress = false;
        let output = stream(data.as_bytes(), &AtomicBool::new(false), |event| {
            progress = event["type"] == "response.web_search_call.searching";
            Ok(())
        })
        .unwrap();
        assert!(progress);
        assert_eq!(output, vec![search]);
    }
}
