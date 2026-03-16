mod control;
mod store;
mod types;

use crate::rpc_dispatch::RpcRequestContext;
use bytes::Bytes;
use codexmanager_core::storage::now_ts;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;
use types::{
    SessionSourceWire, ThreadCompactStartParams, ThreadForkParams, ThreadNameSetParams,
    ThreadReadParams, ThreadResumeParams, ThreadSessionConfig, ThreadStartParams, ThreadStatusWire,
    ThreadTokenUsageWire, ThreadWire, TokenUsageBreakdownWire, TurnErrorWire, TurnInterruptParams,
    TurnStartParams, TurnStatusWire, TurnSteerParams, TurnWire,
};

static THREAD_ID_SEQ: AtomicU64 = AtomicU64::new(1);
static TURN_ID_SEQ: AtomicU64 = AtomicU64::new(1);
static ITEM_ID_SEQ: AtomicU64 = AtomicU64::new(1);
const TURN_RUNTIME_POLL_INTERVAL: Duration = Duration::from_millis(100);

fn next_thread_id() -> String {
    format!("thr_{}", THREAD_ID_SEQ.fetch_add(1, Ordering::Relaxed))
}

fn next_turn_id() -> String {
    format!("turn_{}", TURN_ID_SEQ.fetch_add(1, Ordering::Relaxed))
}

fn next_item_id() -> String {
    format!("item_{}", ITEM_ID_SEQ.fetch_add(1, Ordering::Relaxed))
}

fn parse_params<T: serde::de::DeserializeOwned + Default>(
    params: Option<&Value>,
) -> Result<T, String> {
    match params {
        Some(value) => serde_json::from_value::<T>(value.clone())
            .map_err(|err| format!("invalid request payload: {err}")),
        None => Ok(T::default()),
    }
}

fn default_cwd() -> String {
    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .to_string_lossy()
        .into_owned()
}

fn default_model() -> String {
    "gpt-5.4".to_string()
}

fn build_thread_session_config(
    model: Option<String>,
    service_tier: Option<String>,
    cwd: Option<String>,
    approval_policy: Option<String>,
    approvals_reviewer: Option<Value>,
    sandbox: Option<Value>,
    reasoning_effort: Option<String>,
) -> ThreadSessionConfig {
    let cwd = cwd.unwrap_or_else(default_cwd);
    ThreadSessionConfig {
        model: model.unwrap_or_else(default_model),
        model_provider: "openai".to_string(),
        service_tier,
        cwd: cwd.clone(),
        approval_policy: approval_policy.unwrap_or_else(|| "unlessTrusted".to_string()),
        approvals_reviewer,
        sandbox: sandbox.unwrap_or_else(|| json!({ "type": "dangerFullAccess" })),
        reasoning_effort,
    }
}

fn extract_preview(input: &[Value]) -> String {
    input
        .iter()
        .find_map(|item| {
            item.as_object()
                .filter(|object| {
                    object
                        .get("type")
                        .and_then(|value| value.as_str())
                        .is_some_and(|value| value == "text")
                })
                .and_then(|object| object.get("text"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
        })
        .unwrap_or_default()
}

fn thread_for_wire(thread: &store::StoredThread, include_turns: bool) -> ThreadWire {
    let mut wire = thread.wire.clone();
    wire.turns = if include_turns {
        thread
            .turn_order
            .iter()
            .filter_map(|turn_id| thread.turns.get(turn_id).map(|turn| turn.wire.clone()))
            .collect()
    } else {
        Vec::new()
    };
    wire
}

fn thread_response_payload(thread: &store::StoredThread, include_turns: bool) -> Value {
    json!({
        "thread": thread_for_wire(thread, include_turns),
        "model": thread.session.model,
        "modelProvider": thread.session.model_provider,
        "serviceTier": thread.session.service_tier,
        "cwd": thread.session.cwd,
        "approvalPolicy": thread.session.approval_policy,
        "approvalsReviewer": thread.session.approvals_reviewer,
        "sandbox": thread.session.sandbox,
        "reasoningEffort": thread.session.reasoning_effort,
    })
}

fn turn_for_notification(turn: &TurnWire) -> TurnWire {
    let mut wire = turn.clone();
    wire.items.clear();
    wire
}

fn user_message_item(input: Vec<Value>) -> Value {
    json!({
        "type": "userMessage",
        "id": next_item_id(),
        "content": input,
    })
}

fn agent_message_item(item_id: &str, text: &str) -> Value {
    json!({
        "type": "agentMessage",
        "id": item_id,
        "text": text,
        "phase": Value::Null,
    })
}

fn context_compaction_item(item_id: &str) -> Value {
    json!({
        "type": "contextCompaction",
        "id": item_id,
    })
}

fn usage_breakdown_from_app_usage(
    usage: &crate::gateway::AppServerTurnUsage,
) -> TokenUsageBreakdownWire {
    TokenUsageBreakdownWire {
        total_tokens: usage.total_tokens.unwrap_or_default().max(0),
        input_tokens: usage.input_tokens.unwrap_or_default().max(0),
        cached_input_tokens: usage.cached_input_tokens.unwrap_or_default().max(0),
        output_tokens: usage.output_tokens.unwrap_or_default().max(0),
        reasoning_output_tokens: usage.reasoning_output_tokens.unwrap_or_default().max(0),
    }
}

fn add_usage_breakdown(total: &mut TokenUsageBreakdownWire, last: TokenUsageBreakdownWire) {
    total.total_tokens = total.total_tokens.saturating_add(last.total_tokens);
    total.input_tokens = total.input_tokens.saturating_add(last.input_tokens);
    total.cached_input_tokens = total
        .cached_input_tokens
        .saturating_add(last.cached_input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(last.output_tokens);
    total.reasoning_output_tokens = total
        .reasoning_output_tokens
        .saturating_add(last.reasoning_output_tokens);
}

fn append_turn_input_content(content: &mut Vec<Value>, item: &Value) {
    if let Some(text) = item
        .as_object()
        .and_then(|object| object.get("text"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        content.push(json!({
            "type": "input_text",
            "text": text,
        }));
        return;
    }

    if let Some(text) = item
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        content.push(json!({
            "type": "input_text",
            "text": text,
        }));
    }
}

fn build_turn_request_body(
    model: &str,
    input: &[Value],
    service_tier: Option<&str>,
    reasoning_effort: Option<&str>,
) -> Result<Bytes, String> {
    let mut content = Vec::new();
    for item in input {
        append_turn_input_content(&mut content, item);
    }
    if content.is_empty() {
        return Err("turn input does not contain supported text content".to_string());
    }

    let mut object = serde_json::Map::new();
    object.insert("model".to_string(), Value::String(model.to_string()));
    object.insert("instructions".to_string(), Value::String(String::new()));
    object.insert(
        "input".to_string(),
        Value::Array(vec![json!({
            "role": "user",
            "content": content,
        })]),
    );
    object.insert("stream".to_string(), Value::Bool(true));
    object.insert("store".to_string(), Value::Bool(false));
    if let Some(service_tier) = service_tier
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert(
            "service_tier".to_string(),
            Value::String(service_tier.to_string()),
        );
    }
    if let Some(reasoning_effort) = reasoning_effort
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert(
            "reasoning".to_string(),
            json!({
                "effort": reasoning_effort,
            }),
        );
    }
    serde_json::to_vec(&Value::Object(object))
        .map(Bytes::from)
        .map_err(|err| format!("serialize turn request body failed: {err}"))
}

fn extract_sse_frame_payload(lines: &[String]) -> Option<String> {
    let mut data_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some(rest) = trimmed.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }
    if !data_lines.is_empty() {
        return Some(data_lines.join("\n"));
    }

    let mut raw_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with(':')
            || trimmed.starts_with("event:")
            || trimmed.starts_with("id:")
            || trimmed.starts_with("retry:")
        {
            continue;
        }
        raw_lines.push(trimmed.to_string());
    }
    if raw_lines.is_empty() {
        None
    } else {
        Some(raw_lines.join("\n"))
    }
}

fn extract_json_error_message(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    error
                        .as_str()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_string)
                })
        })
        .or_else(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
}

fn merge_usage_from_usage_value(target: &mut crate::gateway::AppServerTurnUsage, value: &Value) {
    let Some(usage) = value.as_object() else {
        return;
    };
    if let Some(input_tokens) = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| usage.get("prompt_tokens").and_then(Value::as_i64))
    {
        target.input_tokens = Some(input_tokens);
    }
    if let Some(output_tokens) = usage
        .get("output_tokens")
        .and_then(Value::as_i64)
        .or_else(|| usage.get("completion_tokens").and_then(Value::as_i64))
    {
        target.output_tokens = Some(output_tokens);
    }
    if let Some(total_tokens) = usage.get("total_tokens").and_then(Value::as_i64) {
        target.total_tokens = Some(total_tokens);
    }
    if let Some(cached_input_tokens) = usage
        .get("input_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("prompt_tokens_details")
                .and_then(Value::as_object)
                .and_then(|details| details.get("cached_tokens"))
                .and_then(Value::as_i64)
        })
    {
        target.cached_input_tokens = Some(cached_input_tokens);
    }
    if let Some(reasoning_output_tokens) = usage
        .get("output_tokens_details")
        .and_then(Value::as_object)
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("completion_tokens_details")
                .and_then(Value::as_object)
                .and_then(|details| details.get("reasoning_tokens"))
                .and_then(Value::as_i64)
        })
    {
        target.reasoning_output_tokens = Some(reasoning_output_tokens);
    }
}

fn collect_response_text(value: &Value, output: &mut String) {
    match value {
        Value::String(text) => output.push_str(text),
        Value::Array(items) => {
            for item in items {
                collect_response_text(item, output);
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("output_text").and_then(Value::as_str) {
                output.push_str(text);
            }
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                output.push_str(text);
            }
            if let Some(delta) = map.get("delta") {
                collect_response_text(delta, output);
            }
            if let Some(content) = map.get("content") {
                collect_response_text(content, output);
            }
            if let Some(output_field) = map.get("output") {
                collect_response_text(output_field, output);
            }
            if let Some(response) = map.get("response") {
                collect_response_text(response, output);
            }
        }
        _ => {}
    }
}

fn update_turn_agent_text(
    thread_id: &str,
    turn_id: &str,
    item_id: &str,
    delta: &str,
) -> Option<Value> {
    let mut threads = crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
    let thread = threads.get_mut(thread_id)?;
    let turn = thread.turns.get_mut(turn_id)?;
    let mut item_started = None;
    if let Some(existing) = turn
        .wire
        .items
        .iter_mut()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(item_id))
    {
        if let Some(object) = existing.as_object_mut() {
            let mut text = object
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            text.push_str(delta);
            object.insert("text".to_string(), Value::String(text));
        }
    } else {
        let item = agent_message_item(item_id, "");
        turn.wire.items.push(item.clone());
        item_started = Some(item);
        if let Some(existing) = turn
            .wire
            .items
            .iter_mut()
            .find(|item| item.get("id").and_then(Value::as_str) == Some(item_id))
        {
            if let Some(object) = existing.as_object_mut() {
                object.insert("text".to_string(), Value::String(delta.to_string()));
            }
        }
    }
    thread.wire.updated_at = now_ts();
    item_started
}

fn read_turn_agent_item(thread_id: &str, turn_id: &str, item_id: &str) -> Option<Value> {
    let threads = crate::lock_utils::read_recover(store::read_store(), "thread_turn_store");
    threads
        .get(thread_id)
        .and_then(|thread| thread.turns.get(turn_id))
        .and_then(|turn| {
            turn.wire
                .items
                .iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(item_id))
                .cloned()
        })
}

enum TurnStreamPumpItem {
    Frame(Vec<String>),
    Eof,
    Error(String),
}

struct TurnSseFramePump {
    rx: Receiver<TurnStreamPumpItem>,
}

impl TurnSseFramePump {
    fn new(upstream: reqwest::blocking::Response) -> Self {
        let (tx, rx) = mpsc::sync_channel::<TurnStreamPumpItem>(32);
        thread::spawn(move || {
            let mut reader = BufReader::new(upstream);
            let mut pending_frame_lines = Vec::new();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        if !pending_frame_lines.is_empty()
                            && tx
                                .send(TurnStreamPumpItem::Frame(pending_frame_lines))
                                .is_err()
                        {
                            return;
                        }
                        let _ = tx.send(TurnStreamPumpItem::Eof);
                        return;
                    }
                    Ok(_) => {
                        let is_blank = line == "\n" || line == "\r\n";
                        pending_frame_lines.push(line);
                        if is_blank {
                            let frame = std::mem::take(&mut pending_frame_lines);
                            if tx.send(TurnStreamPumpItem::Frame(frame)).is_err() {
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(TurnStreamPumpItem::Error(err.to_string()));
                        return;
                    }
                }
            }
        });
        Self { rx }
    }

    fn recv_timeout(&self, timeout: Duration) -> Result<TurnStreamPumpItem, RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
}

fn subscriber_set(thread_id: &str) -> BTreeSet<String> {
    crate::lock_utils::read_recover(store::read_store(), "thread_turn_store")
        .get(thread_id)
        .map(|thread| thread.subscribers.clone())
        .unwrap_or_default()
}

fn send_to_subscribers(thread_id: &str, method: &str, params: Value) {
    let subscribers = subscriber_set(thread_id);
    if subscribers.is_empty() {
        return;
    }

    let mut stale = Vec::new();
    for connection_id in subscribers {
        if !crate::rpc_transport::send_notification_to_connection(
            connection_id.as_str(),
            method,
            params.clone(),
        ) {
            stale.push(connection_id);
        }
    }

    if stale.is_empty() {
        return;
    }

    let mut threads = crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
    if let Some(thread) = threads.get_mut(thread_id) {
        for connection_id in stale {
            thread.subscribers.remove(connection_id.as_str());
        }
    }
}

fn broadcast_thread_status(thread_id: &str, status: ThreadStatusWire) {
    send_to_subscribers(
        thread_id,
        "thread/status/changed",
        json!({
            "threadId": thread_id,
            "status": status,
        }),
    );
}

fn emit_thread_token_usage_updated(
    thread_id: &str,
    turn_id: &str,
    usage: &crate::gateway::AppServerTurnUsage,
) {
    let token_usage = {
        let mut threads =
            crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
        let Some(thread) = threads.get_mut(thread_id) else {
            return;
        };
        let last = usage_breakdown_from_app_usage(usage);
        add_usage_breakdown(&mut thread.token_usage.total, last);
        thread.token_usage.last = last;
        thread.token_usage.clone()
    };

    send_to_subscribers(
        thread_id,
        "thread/tokenUsage/updated",
        json!({
            "threadId": thread_id,
            "turnId": turn_id,
            "tokenUsage": token_usage,
        }),
    );
}

fn finalize_turn(
    thread_id: &str,
    turn_id: &str,
    status: TurnStatusWire,
    error: Option<TurnErrorWire>,
) -> bool {
    let turn_notification = {
        let mut threads =
            crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
        let Some(thread) = threads.get_mut(thread_id) else {
            return false;
        };
        let Some(turn) = thread.turns.get_mut(turn_id) else {
            return false;
        };
        if turn.wire.status != TurnStatusWire::InProgress {
            return false;
        }

        turn.wire.status = status;
        turn.wire.error = error;
        if thread.active_turn_id.as_deref() == Some(turn_id) {
            thread.active_turn_id = None;
        }
        thread.wire.status = ThreadStatusWire::Idle;
        thread.wire.updated_at = now_ts();
        turn_for_notification(&turn.wire)
    };

    control::clear_turn(turn_id);
    send_to_subscribers(
        thread_id,
        "turn/completed",
        json!({
            "threadId": thread_id,
            "turn": turn_notification,
        }),
    );
    broadcast_thread_status(thread_id, ThreadStatusWire::Idle);
    true
}

fn build_turn_failed_error(message: String, details: Option<String>) -> TurnErrorWire {
    TurnErrorWire {
        message,
        codex_error_info: None,
        additional_details: details,
    }
}

fn stream_agent_response_to_notifications(
    thread_id: &str,
    turn_id: &str,
    mut execution: crate::gateway::AppServerTurnExecution,
) -> Result<(crate::gateway::AppServerTurnUsage, Option<String>), TurnErrorWire> {
    let pump = TurnSseFramePump::new(
        execution
            .take_response()
            .map_err(|err| build_turn_failed_error(err, None))?,
    );
    let agent_item_id = next_item_id();
    let mut usage = crate::gateway::AppServerTurnUsage::default();
    let mut agent_started = false;
    let mut response_model = None;

    loop {
        if control::turn_control(turn_id)
            .map(|control| control.cancelled())
            .unwrap_or(false)
        {
            if agent_started {
                if let Some(item) = read_turn_agent_item(thread_id, turn_id, agent_item_id.as_str())
                {
                    send_to_subscribers(
                        thread_id,
                        "item/completed",
                        json!({
                            "threadId": thread_id,
                            "turnId": turn_id,
                            "item": item,
                        }),
                    );
                }
            }
            execution.finish(
                499,
                usage,
                Some("turn interrupted"),
                response_model.as_deref(),
            );
            return Err(build_turn_failed_error(
                "turn interrupted".to_string(),
                None,
            ));
        }

        match pump.recv_timeout(TURN_RUNTIME_POLL_INTERVAL) {
            Ok(TurnStreamPumpItem::Frame(lines)) => {
                let Some(payload) = extract_sse_frame_payload(lines.as_slice()) else {
                    continue;
                };
                if payload.trim() == "[DONE]" {
                    continue;
                }
                let value = serde_json::from_str::<Value>(payload.as_str()).map_err(|err| {
                    build_turn_failed_error(
                        "invalid upstream stream payload".to_string(),
                        Some(err.to_string()),
                    )
                })?;
                if let Some(message) = extract_json_error_message(&value) {
                    execution.finish(
                        502,
                        usage,
                        Some(message.as_str()),
                        response_model.as_deref(),
                    );
                    return Err(build_turn_failed_error(message, None));
                }
                if let Some(response_usage) = value
                    .get("response")
                    .and_then(|response| response.get("usage"))
                    .or_else(|| value.get("usage"))
                {
                    merge_usage_from_usage_value(&mut usage, response_usage);
                    emit_thread_token_usage_updated(thread_id, turn_id, &usage);
                }
                if response_model.is_none() {
                    response_model = value
                        .get("response")
                        .and_then(|response| response.get("model"))
                        .or_else(|| value.get("model"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }

                let event_type = value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                match event_type {
                    "response.output_text.delta" => {
                        let delta = value
                            .get("delta")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if delta.is_empty() {
                            continue;
                        }
                        if let Some(item) = update_turn_agent_text(
                            thread_id,
                            turn_id,
                            agent_item_id.as_str(),
                            delta,
                        ) {
                            send_to_subscribers(
                                thread_id,
                                "item/started",
                                json!({
                                    "threadId": thread_id,
                                    "turnId": turn_id,
                                    "item": item,
                                }),
                            );
                            agent_started = true;
                        }
                        send_to_subscribers(
                            thread_id,
                            "item/agentMessage/delta",
                            json!({
                                "threadId": thread_id,
                                "turnId": turn_id,
                                "itemId": agent_item_id,
                                "delta": delta,
                            }),
                        );
                    }
                    "response.completed" | "response.done" => {
                        if !agent_started {
                            let mut text = String::new();
                            if let Some(response) = value.get("response") {
                                collect_response_text(response, &mut text);
                            }
                            if !text.trim().is_empty() {
                                if let Some(item) = update_turn_agent_text(
                                    thread_id,
                                    turn_id,
                                    agent_item_id.as_str(),
                                    text.as_str(),
                                ) {
                                    send_to_subscribers(
                                        thread_id,
                                        "item/started",
                                        json!({
                                            "threadId": thread_id,
                                            "turnId": turn_id,
                                            "item": item,
                                        }),
                                    );
                                    agent_started = true;
                                }
                            }
                        }
                        break;
                    }
                    "response.output_text.done" => {}
                    "response.failed"
                    | "response.error"
                    | "response.cancelled"
                    | "response.canceled"
                    | "response.incomplete" => {
                        let message = extract_json_error_message(&value)
                            .unwrap_or_else(|| event_type.to_string());
                        if agent_started {
                            if let Some(item) =
                                read_turn_agent_item(thread_id, turn_id, agent_item_id.as_str())
                            {
                                send_to_subscribers(
                                    thread_id,
                                    "item/completed",
                                    json!({
                                        "threadId": thread_id,
                                        "turnId": turn_id,
                                        "item": item,
                                    }),
                                );
                            }
                        }
                        execution.finish(
                            502,
                            usage,
                            Some(message.as_str()),
                            response_model.as_deref(),
                        );
                        return Err(build_turn_failed_error(message, None));
                    }
                    _ => {}
                }
            }
            Ok(TurnStreamPumpItem::Eof) => {
                let message = "上游流中途中断（未正常结束）".to_string();
                if agent_started {
                    if let Some(item) =
                        read_turn_agent_item(thread_id, turn_id, agent_item_id.as_str())
                    {
                        send_to_subscribers(
                            thread_id,
                            "item/completed",
                            json!({
                                "threadId": thread_id,
                                "turnId": turn_id,
                                "item": item,
                            }),
                        );
                    }
                }
                execution.finish(
                    502,
                    usage,
                    Some(message.as_str()),
                    response_model.as_deref(),
                );
                return Err(build_turn_failed_error(message, None));
            }
            Ok(TurnStreamPumpItem::Error(err)) => {
                let message = format!("上游流读取失败：{err}");
                if agent_started {
                    if let Some(item) =
                        read_turn_agent_item(thread_id, turn_id, agent_item_id.as_str())
                    {
                        send_to_subscribers(
                            thread_id,
                            "item/completed",
                            json!({
                                "threadId": thread_id,
                                "turnId": turn_id,
                                "item": item,
                            }),
                        );
                    }
                }
                execution.finish(
                    502,
                    usage,
                    Some(message.as_str()),
                    response_model.as_deref(),
                );
                return Err(build_turn_failed_error(message, None));
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                let message = "上游流读取失败（连接中断）".to_string();
                if agent_started {
                    if let Some(item) =
                        read_turn_agent_item(thread_id, turn_id, agent_item_id.as_str())
                    {
                        send_to_subscribers(
                            thread_id,
                            "item/completed",
                            json!({
                                "threadId": thread_id,
                                "turnId": turn_id,
                                "item": item,
                            }),
                        );
                    }
                }
                execution.finish(
                    502,
                    usage,
                    Some(message.as_str()),
                    response_model.as_deref(),
                );
                return Err(build_turn_failed_error(message, None));
            }
        }
    }

    if agent_started {
        if let Some(item) = read_turn_agent_item(thread_id, turn_id, agent_item_id.as_str()) {
            send_to_subscribers(
                thread_id,
                "item/completed",
                json!({
                    "threadId": thread_id,
                    "turnId": turn_id,
                    "item": item,
                }),
            );
        }
    }
    execution.finish(200, usage.clone(), None, response_model.as_deref());
    Ok((usage, response_model))
}

fn spawn_turn_runtime(
    thread_id: String,
    turn_id: String,
    user_item: Value,
    input: Vec<Value>,
    model: String,
    service_tier: Option<String>,
    reasoning_effort: Option<String>,
) {
    thread::spawn(move || {
        send_to_subscribers(
            thread_id.as_str(),
            "turn/started",
            json!({
                "threadId": thread_id,
                "turn": {
                    "id": turn_id,
                    "items": [],
                    "status": "inProgress",
                    "error": Value::Null,
                }
            }),
        );
        send_to_subscribers(
            thread_id.as_str(),
            "item/started",
            json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "item": user_item,
            }),
        );
        send_to_subscribers(
            thread_id.as_str(),
            "item/completed",
            json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "item": user_item,
            }),
        );
        if control::turn_control(turn_id.as_str())
            .map(|control| control.cancelled())
            .unwrap_or(false)
        {
            let _ = finalize_turn(
                thread_id.as_str(),
                turn_id.as_str(),
                TurnStatusWire::Interrupted,
                None,
            );
            return;
        }

        let body = match build_turn_request_body(
            model.as_str(),
            input.as_slice(),
            service_tier.as_deref(),
            reasoning_effort.as_deref(),
        ) {
            Ok(body) => body,
            Err(err) => {
                let _ = finalize_turn(
                    thread_id.as_str(),
                    turn_id.as_str(),
                    TurnStatusWire::Failed,
                    Some(build_turn_failed_error(err, None)),
                );
                return;
            }
        };

        let execution = match crate::gateway::execute_app_server_turn_response_request(
            &body,
            Some(model.as_str()),
            reasoning_effort.as_deref(),
        ) {
            Ok(execution) => execution,
            Err(err) => {
                let _ = finalize_turn(
                    thread_id.as_str(),
                    turn_id.as_str(),
                    TurnStatusWire::Failed,
                    Some(build_turn_failed_error(err, None)),
                );
                return;
            }
        };

        match stream_agent_response_to_notifications(
            thread_id.as_str(),
            turn_id.as_str(),
            execution,
        ) {
            Ok(_) => {
                let _ = finalize_turn(
                    thread_id.as_str(),
                    turn_id.as_str(),
                    TurnStatusWire::Completed,
                    None,
                );
            }
            Err(err) if err.message == "turn interrupted" => {
                let _ = finalize_turn(
                    thread_id.as_str(),
                    turn_id.as_str(),
                    TurnStatusWire::Interrupted,
                    None,
                );
            }
            Err(err) => {
                let _ = finalize_turn(
                    thread_id.as_str(),
                    turn_id.as_str(),
                    TurnStatusWire::Failed,
                    Some(err),
                );
            }
        }
    });
}

fn spawn_compaction_runtime(thread_id: String, turn_id: String, item: Value) {
    thread::spawn(move || {
        send_to_subscribers(
            thread_id.as_str(),
            "turn/started",
            json!({
                "threadId": thread_id,
                "turn": {
                    "id": turn_id,
                    "items": [],
                    "status": "inProgress",
                    "error": Value::Null,
                }
            }),
        );
        send_to_subscribers(
            thread_id.as_str(),
            "item/started",
            json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "item": item,
            }),
        );
        thread::sleep(Duration::from_millis(80));
        send_to_subscribers(
            thread_id.as_str(),
            "item/completed",
            json!({
                "threadId": thread_id,
                "turnId": turn_id,
                "item": item,
            }),
        );
        let _ = finalize_turn(
            thread_id.as_str(),
            turn_id.as_str(),
            TurnStatusWire::Completed,
            None,
        );
    });
}

pub(crate) fn remove_connection(connection_id: &str) {
    if connection_id.trim().is_empty() {
        return;
    }
    let mut threads = crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
    for thread in threads.values_mut() {
        thread.subscribers.remove(connection_id);
    }
}

pub(crate) fn thread_start(
    params: Option<&Value>,
    ctx: &RpcRequestContext,
) -> Result<Value, String> {
    let params = parse_params::<ThreadStartParams>(params)?;
    let now = now_ts();
    let thread_id = next_thread_id();
    let thread = ThreadWire {
        id: thread_id.clone(),
        preview: String::new(),
        ephemeral: params.ephemeral,
        model_provider: "openai".to_string(),
        created_at: now,
        updated_at: now,
        status: ThreadStatusWire::Idle,
        path: None,
        cwd: params.cwd.unwrap_or_else(default_cwd),
        cli_version: codexmanager_core::core_version().to_string(),
        source: SessionSourceWire::AppServer,
        agent_nickname: None,
        agent_role: None,
        git_info: None,
        name: None,
        turns: Vec::new(),
    };

    let response_thread = {
        let mut threads =
            crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
        let session = build_thread_session_config(
            params.model,
            params.service_tier,
            Some(thread.cwd.clone()),
            params.approval_policy,
            params.approvals_reviewer,
            params.sandbox,
            params.reasoning_effort,
        );
        let mut stored = store::StoredThread {
            wire: thread,
            session,
            token_usage: ThreadTokenUsageWire::default(),
            subscribers: BTreeSet::new(),
            turns: Default::default(),
            turn_order: Vec::new(),
            active_turn_id: None,
        };
        if let Some(connection_id) = ctx.connection_id.as_deref() {
            stored.subscribers.insert(connection_id.to_string());
        }
        let response_thread = thread_response_payload(&stored, false);
        threads.insert(thread_id.clone(), stored);
        response_thread
    };

    send_to_subscribers(
        thread_id.as_str(),
        "thread/started",
        json!({
            "thread": response_thread["thread"].clone(),
        }),
    );

    Ok(response_thread)
}

pub(crate) fn thread_resume(
    params: Option<&Value>,
    ctx: &RpcRequestContext,
) -> Result<Value, String> {
    let params = parse_params::<ThreadResumeParams>(params)?;
    if params.thread_id.trim().is_empty() {
        return Err("threadId is required".to_string());
    }
    let thread = {
        let mut threads =
            crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
        let Some(thread) = threads.get_mut(params.thread_id.as_str()) else {
            return Err("thread not found".to_string());
        };
        if let Some(connection_id) = ctx.connection_id.as_deref() {
            thread.subscribers.insert(connection_id.to_string());
        }
        if let Some(model) = params.model {
            thread.session.model = model;
            thread.session.model_provider = "openai".to_string();
        }
        if let Some(service_tier) = params.service_tier {
            thread.session.service_tier = Some(service_tier);
        }
        if let Some(cwd_value) = params.cwd {
            let cwd: String = cwd_value;
            thread.session.cwd = cwd.clone();
            thread.wire.cwd = cwd;
        }
        if let Some(approval_policy) = params.approval_policy {
            thread.session.approval_policy = approval_policy;
        }
        if let Some(approvals_reviewer) = params.approvals_reviewer {
            thread.session.approvals_reviewer = Some(approvals_reviewer);
        }
        if let Some(sandbox) = params.sandbox {
            thread.session.sandbox = sandbox;
        }
        if let Some(reasoning_effort) = params.reasoning_effort {
            thread.session.reasoning_effort = Some(reasoning_effort);
        }
        thread_response_payload(thread, true)
    };
    Ok(thread)
}

pub(crate) fn thread_fork(
    params: Option<&Value>,
    ctx: &RpcRequestContext,
) -> Result<Value, String> {
    let params = parse_params::<ThreadForkParams>(params)?;
    if params.thread_id.trim().is_empty() {
        return Err("threadId is required".to_string());
    }

    let (new_thread_id, response_thread, started_thread) = {
        let now = now_ts();
        let mut threads =
            crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
        let Some(source) = threads.get(params.thread_id.as_str()).cloned() else {
            return Err("thread not found".to_string());
        };
        let new_thread_id = next_thread_id();
        let mut cloned = source.clone();
        cloned.wire.id = new_thread_id.clone();
        cloned.wire.created_at = now;
        cloned.wire.updated_at = now;
        cloned.wire.ephemeral = params.ephemeral;
        cloned.wire.status = ThreadStatusWire::Idle;
        cloned.wire.turns.clear();
        cloned.token_usage = source.token_usage.clone();
        cloned.active_turn_id = None;
        if let Some(model) = params.model {
            cloned.session.model = model;
            cloned.session.model_provider = "openai".to_string();
        }
        if let Some(service_tier) = params.service_tier {
            cloned.session.service_tier = Some(service_tier);
        }
        if let Some(cwd_value) = params.cwd {
            let cwd: String = cwd_value;
            cloned.session.cwd = cwd.clone();
            cloned.wire.cwd = cwd;
        }
        if let Some(approval_policy) = params.approval_policy {
            cloned.session.approval_policy = approval_policy;
        }
        if let Some(approvals_reviewer) = params.approvals_reviewer {
            cloned.session.approvals_reviewer = Some(approvals_reviewer);
        }
        if let Some(sandbox) = params.sandbox {
            cloned.session.sandbox = sandbox;
        }
        if let Some(reasoning_effort) = params.reasoning_effort {
            cloned.session.reasoning_effort = Some(reasoning_effort);
        }
        if let Some(connection_id) = ctx.connection_id.as_deref() {
            cloned.subscribers.insert(connection_id.to_string());
        }
        let response_thread = thread_response_payload(&cloned, true);
        let started_thread = thread_for_wire(&cloned, false);
        threads.insert(new_thread_id.clone(), cloned);
        (new_thread_id, response_thread, started_thread)
    };

    send_to_subscribers(
        new_thread_id.as_str(),
        "thread/started",
        json!({
            "thread": started_thread,
        }),
    );

    Ok(response_thread)
}

pub(crate) fn thread_read(params: Option<&Value>) -> Result<Value, String> {
    let params = parse_params::<ThreadReadParams>(params)?;
    if params.thread_id.trim().is_empty() {
        return Err("threadId is required".to_string());
    }
    let threads = crate::lock_utils::read_recover(store::read_store(), "thread_turn_store");
    let Some(thread) = threads.get(params.thread_id.as_str()) else {
        return Err("thread not found".to_string());
    };
    Ok(json!({
        "thread": thread_for_wire(thread, params.include_turns),
    }))
}

pub(crate) fn thread_name_set(
    params: Option<&Value>,
    _ctx: &RpcRequestContext,
) -> Result<Value, String> {
    let params = parse_params::<ThreadNameSetParams>(params)?;
    if params.thread_id.trim().is_empty() {
        return Err("threadId is required".to_string());
    }
    {
        let mut threads =
            crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
        let Some(thread) = threads.get_mut(params.thread_id.as_str()) else {
            return Err("thread not found".to_string());
        };
        thread.wire.name = params.name.clone();
        thread.wire.updated_at = now_ts();
    }

    send_to_subscribers(
        params.thread_id.as_str(),
        "thread/name/updated",
        json!({
            "threadId": params.thread_id,
            "threadName": params.name,
        }),
    );
    Ok(json!({}))
}

pub(crate) fn thread_compact_start(
    params: Option<&Value>,
    ctx: &RpcRequestContext,
) -> Result<Value, String> {
    let params = parse_params::<ThreadCompactStartParams>(params)?;
    if params.thread_id.trim().is_empty() {
        return Err("threadId is required".to_string());
    }

    let turn_id = next_turn_id();
    let compaction_item_id = next_item_id();
    let compaction_item = context_compaction_item(compaction_item_id.as_str());

    {
        let mut threads =
            crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
        let Some(thread) = threads.get_mut(params.thread_id.as_str()) else {
            return Err("thread not found".to_string());
        };
        if thread.active_turn_id.is_some() {
            return Err("thread already has an active turn".to_string());
        }
        if let Some(connection_id) = ctx.connection_id.as_deref() {
            thread.subscribers.insert(connection_id.to_string());
        }
        thread.wire.updated_at = now_ts();
        thread.wire.status = ThreadStatusWire::Active {
            active_flags: Vec::new(),
        };
        thread.active_turn_id = Some(turn_id.clone());
        thread.turn_order.push(turn_id.clone());
        thread.turns.insert(
            turn_id.clone(),
            store::StoredTurn {
                wire: TurnWire {
                    id: turn_id.clone(),
                    items: vec![compaction_item.clone()],
                    status: TurnStatusWire::InProgress,
                    error: None,
                },
            },
        );
    }

    let _ = control::register_turn(turn_id.clone());
    broadcast_thread_status(
        params.thread_id.as_str(),
        ThreadStatusWire::Active {
            active_flags: Vec::new(),
        },
    );
    spawn_compaction_runtime(params.thread_id, turn_id, compaction_item);
    Ok(json!({}))
}

pub(crate) fn turn_start(params: Option<&Value>, ctx: &RpcRequestContext) -> Result<Value, String> {
    let params = parse_params::<TurnStartParams>(params)?;
    if params.thread_id.trim().is_empty() {
        return Err("threadId is required".to_string());
    }
    if params.input.is_empty() {
        return Err("input is required".to_string());
    }

    let turn_id = next_turn_id();
    let user_item = user_message_item(params.input.clone());
    let turn_input = params.input.clone();
    let (turn_model, turn_service_tier, turn_reasoning_effort) = {
        let mut threads =
            crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
        let Some(thread) = threads.get_mut(params.thread_id.as_str()) else {
            return Err("thread not found".to_string());
        };
        if thread.active_turn_id.is_some() {
            return Err("thread already has an active turn".to_string());
        }
        if let Some(connection_id) = ctx.connection_id.as_deref() {
            thread.subscribers.insert(connection_id.to_string());
        }
        if thread.wire.preview.is_empty() {
            thread.wire.preview = extract_preview(&params.input);
        }
        if let Some(model) = params
            .model
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            thread.session.model = model.to_string();
            thread.wire.model_provider = if model.starts_with("gpt-") {
                "openai".to_string()
            } else {
                "unknown".to_string()
            };
            thread.session.model_provider = thread.wire.model_provider.clone();
        }
        thread.wire.updated_at = now_ts();
        thread.wire.status = ThreadStatusWire::Active {
            active_flags: Vec::new(),
        };
        thread.active_turn_id = Some(turn_id.clone());
        thread.turn_order.push(turn_id.clone());
        thread.turns.insert(
            turn_id.clone(),
            store::StoredTurn {
                wire: TurnWire {
                    id: turn_id.clone(),
                    items: vec![user_item.clone()],
                    status: TurnStatusWire::InProgress,
                    error: None,
                },
            },
        );
        (
            thread.session.model.clone(),
            thread.session.service_tier.clone(),
            thread.session.reasoning_effort.clone(),
        )
    };

    let _ = control::register_turn(turn_id.clone());
    broadcast_thread_status(
        params.thread_id.as_str(),
        ThreadStatusWire::Active {
            active_flags: Vec::new(),
        },
    );
    spawn_turn_runtime(
        params.thread_id.clone(),
        turn_id.clone(),
        user_item,
        turn_input,
        turn_model,
        turn_service_tier,
        turn_reasoning_effort,
    );

    Ok(json!({
        "turn": {
            "id": turn_id,
            "items": [],
            "status": "inProgress",
            "error": Value::Null,
        }
    }))
}

pub(crate) fn turn_steer(params: Option<&Value>) -> Result<Value, String> {
    let params = parse_params::<TurnSteerParams>(params)?;
    if params.thread_id.trim().is_empty() || params.expected_turn_id.trim().is_empty() {
        return Err("threadId and expectedTurnId are required".to_string());
    }
    if params.input.is_empty() {
        return Err("input is required".to_string());
    }
    let user_item = user_message_item(params.input.clone());
    {
        let mut threads =
            crate::lock_utils::write_recover(store::read_store(), "thread_turn_store");
        let Some(thread) = threads.get_mut(params.thread_id.as_str()) else {
            return Err("thread not found".to_string());
        };
        if thread.active_turn_id.as_deref() != Some(params.expected_turn_id.as_str()) {
            return Err("expectedTurnId does not match the active turn".to_string());
        }
        let Some(turn) = thread.turns.get_mut(params.expected_turn_id.as_str()) else {
            return Err("turn not found".to_string());
        };
        if turn.wire.status != TurnStatusWire::InProgress {
            return Err("turn is not in progress".to_string());
        }
        turn.wire.items.push(user_item.clone());
        thread.wire.updated_at = now_ts();
    }

    send_to_subscribers(
        params.thread_id.as_str(),
        "item/started",
        json!({
            "threadId": params.thread_id,
            "turnId": params.expected_turn_id,
            "item": user_item,
        }),
    );
    send_to_subscribers(
        params.thread_id.as_str(),
        "item/completed",
        json!({
            "threadId": params.thread_id,
            "turnId": params.expected_turn_id,
            "item": user_item,
        }),
    );
    Ok(json!({ "turnId": params.expected_turn_id }))
}

pub(crate) fn turn_interrupt(params: Option<&Value>) -> Result<Value, String> {
    let params = parse_params::<TurnInterruptParams>(params)?;
    if params.thread_id.trim().is_empty() || params.turn_id.trim().is_empty() {
        return Err("threadId and turnId are required".to_string());
    }

    let active_turn_matches = {
        let threads = crate::lock_utils::read_recover(store::read_store(), "thread_turn_store");
        let Some(thread) = threads.get(params.thread_id.as_str()) else {
            return Err("thread not found".to_string());
        };
        thread.active_turn_id.as_deref() == Some(params.turn_id.as_str())
    };
    if !active_turn_matches {
        return Err("turn is not active".to_string());
    }

    if let Some(control) = control::turn_control(params.turn_id.as_str()) {
        control.request_cancel();
    }
    Ok(json!({}))
}

pub(crate) fn clear_for_tests() {
    store::clear_for_tests();
    control::clear_for_tests();
}
