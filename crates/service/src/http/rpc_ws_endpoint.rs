use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc::unbounded_channel;
use url::Url;

fn get_header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.as_str().eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn is_loopback_origin(origin: &str) -> bool {
    let Ok(url) = Url::parse(origin) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

fn notification_method(value: &Value) -> Option<&str> {
    value
        .as_object()
        .and_then(|object| object.get("method"))
        .and_then(|method| method.as_str())
        .map(str::trim)
        .filter(|method| !method.is_empty())
}

fn payload_is_notification(value: &Value) -> bool {
    value
        .as_object()
        .map(|object| !object.contains_key("id"))
        .unwrap_or(false)
}

fn text_message(value: Value) -> Message {
    Message::Text(
        serde_json::to_string(&value)
            .unwrap_or_else(|_| "{}".to_string())
            .into(),
    )
}

async fn run_rpc_socket(socket: WebSocket) {
    let (connection_id, mut notification_rx) = crate::rpc_transport::open_connection().into_parts();
    let (mut writer, mut reader) = socket.split();
    let (outbound_tx, mut outbound_rx) = unbounded_channel::<Message>();

    let writer_task = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            if writer.send(message).await.is_err() {
                break;
            }
        }
    });

    let notification_forwarder = {
        let outbound_tx = outbound_tx.clone();
        tokio::spawn(async move {
            while let Some(payload) = notification_rx.recv().await {
                if outbound_tx.send(text_message(payload)).is_err() {
                    break;
                }
            }
        })
    };

    while let Some(message_result) = reader.next().await {
        let Ok(message) = message_result else {
            break;
        };
        match message {
            Message::Text(text) => {
                let payload = match serde_json::from_str::<Value>(&text) {
                    Ok(value) => value,
                    Err(err) => {
                        let _ = outbound_tx.send(text_message(serde_json::json!({
                            "error": format!("invalid json payload: {err}")
                        })));
                        continue;
                    }
                };
                let ctx = crate::rpc_dispatch::RpcRequestContext {
                    connection_id: Some(connection_id.clone()),
                };
                if payload_is_notification(&payload) {
                    if let Some(method) = notification_method(&payload) {
                        let params = payload
                            .as_object()
                            .and_then(|object| object.get("params"))
                            .cloned();
                        let _ = crate::rpc_dispatch::handle_notification_with_context(
                            method, params, &ctx,
                        );
                    }
                    continue;
                }
                let response = match serde_json::from_value::<
                    codexmanager_core::rpc::types::JsonRpcRequest,
                >(payload)
                {
                    Ok(request) => {
                        serde_json::to_value(crate::handle_request_with_context(request, &ctx))
                            .unwrap_or_else(
                                |_| serde_json::json!({ "error": "serialize response failed" }),
                            )
                    }
                    Err(err) => serde_json::json!({
                        "error": format!("invalid rpc request: {err}")
                    }),
                };
                let _ = outbound_tx.send(text_message(response));
            }
            Message::Binary(bytes) => {
                let payload = match String::from_utf8(bytes.to_vec()) {
                    Ok(text) => text,
                    Err(err) => {
                        let _ = outbound_tx.send(text_message(serde_json::json!({
                            "error": format!("invalid utf8 payload: {err}")
                        })));
                        continue;
                    }
                };
                let _ = outbound_tx.send(Message::Text(payload.into()));
            }
            Message::Ping(payload) => {
                let _ = outbound_tx.send(Message::Pong(payload));
            }
            Message::Close(_) => break,
            Message::Pong(_) => {}
        }
    }

    crate::rpc_transport::close_connection(connection_id.as_str());
    notification_forwarder.abort();
    drop(outbound_tx);
    writer_task.abort();
}

pub(crate) async fn handle_rpc_websocket(headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    match get_header_value(&headers, "X-CodexManager-Rpc-Token") {
        Some(token) => {
            if !crate::rpc_auth_token_matches(token) {
                return (StatusCode::UNAUTHORIZED, "{}").into_response();
            }
        }
        None => return (StatusCode::UNAUTHORIZED, "{}").into_response(),
    }

    if let Some(fetch_site) = get_header_value(&headers, "Sec-Fetch-Site") {
        if fetch_site.eq_ignore_ascii_case("cross-site") {
            return (StatusCode::FORBIDDEN, "{}").into_response();
        }
    }
    if let Some(origin) = get_header_value(&headers, "Origin") {
        if !is_loopback_origin(origin) {
            return (StatusCode::FORBIDDEN, "{}").into_response();
        }
    }

    ws.on_upgrade(run_rpc_socket).into_response()
}
