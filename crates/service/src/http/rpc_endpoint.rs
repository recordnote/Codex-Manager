use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response as AxumResponse};
use tiny_http::Request;
use tiny_http::Response;
use url::Url;

fn rpc_response_failed(resp: &codexmanager_core::rpc::types::JsonRpcResponse) -> bool {
    if resp.result.get("error").is_some() {
        return true;
    }
    matches!(
        resp.result.get("ok").and_then(|value| value.as_bool()),
        Some(false)
    )
}

fn get_header_value<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().trim())
        .filter(|value| !value.is_empty())
}

fn is_json_content_type(request: &Request) -> bool {
    get_header_value(request, "Content-Type")
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
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

fn notification_method(value: &serde_json::Value) -> Option<&str> {
    value
        .as_object()
        .and_then(|object| object.get("method"))
        .and_then(|method| method.as_str())
        .map(str::trim)
        .filter(|method| !method.is_empty())
}

fn handle_rpc_payload(
    payload: serde_json::Value,
    connection_id: Option<String>,
) -> (u16, String, bool) {
    let ctx = crate::rpc_dispatch::RpcRequestContext { connection_id };

    let is_notification = payload
        .as_object()
        .map(|object| !object.contains_key("id"))
        .unwrap_or(false);
    if is_notification {
        let method = notification_method(&payload)
            .unwrap_or_default()
            .to_string();
        let params = payload
            .as_object()
            .and_then(|object| object.get("params"))
            .cloned();
        let response_body = match crate::rpc_dispatch::handle_notification_with_context(
            method.as_str(),
            params,
            &ctx,
        ) {
            Ok(()) => "{}".to_string(),
            Err(err) => serde_json::json!({ "error": err }).to_string(),
        };
        let success = response_body == "{}";
        return (200, response_body, success);
    }

    let req: codexmanager_core::rpc::types::JsonRpcRequest = match serde_json::from_value(payload) {
        Ok(v) => v,
        Err(_) => return (400, "{}".to_string(), false),
    };
    let resp = crate::handle_request_with_context(req, &ctx);
    let success = !rpc_response_failed(&resp);
    let json = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string());
    (200, json, success)
}

fn validate_axum_headers(headers: &HeaderMap) -> Option<AxumResponse> {
    match headers
        .get("X-CodexManager-Rpc-Token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(token) => {
            if !crate::rpc_auth_token_matches(token) {
                return Some((StatusCode::UNAUTHORIZED, "{}").into_response());
            }
        }
        None => return Some((StatusCode::UNAUTHORIZED, "{}").into_response()),
    }

    if let Some(fetch_site) = headers
        .get("Sec-Fetch-Site")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
    {
        if fetch_site.eq_ignore_ascii_case("cross-site") {
            return Some((StatusCode::FORBIDDEN, "{}").into_response());
        }
    }
    if let Some(origin) = headers
        .get("Origin")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
    {
        if !is_loopback_origin(origin) {
            return Some((StatusCode::FORBIDDEN, "{}").into_response());
        }
    }

    None
}

pub(crate) async fn handle_rpc_http(headers: HeaderMap, body: String) -> AxumResponse {
    let mut rpc_metrics_guard = crate::gateway::begin_rpc_request();
    if let Some(response) = validate_axum_headers(&headers) {
        return response;
    }
    if body.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "{}").into_response();
    }
    let payload: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return (StatusCode::BAD_REQUEST, "{}").into_response(),
    };
    let connection_id = headers
        .get(crate::rpc_transport::RPC_CONNECTION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let (status, response_body, success) = handle_rpc_payload(payload, connection_id);
    if success {
        rpc_metrics_guard.mark_success();
    }
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        response_body,
    )
        .into_response()
}

pub fn handle_rpc(mut request: Request) {
    let mut rpc_metrics_guard = crate::gateway::begin_rpc_request();
    if request.method().as_str() != "POST" {
        let _ = request.respond(Response::from_string("{}").with_status_code(405));
        return;
    }
    if !is_json_content_type(&request) {
        let _ = request.respond(Response::from_string("{}").with_status_code(415));
        return;
    }

    match get_header_value(&request, "X-CodexManager-Rpc-Token") {
        Some(token) => {
            if !crate::rpc_auth_token_matches(token) {
                let _ = request.respond(Response::from_string("{}").with_status_code(401));
                return;
            }
        }
        None => {
            let _ = request.respond(Response::from_string("{}").with_status_code(401));
            return;
        }
    }

    if let Some(fetch_site) = get_header_value(&request, "Sec-Fetch-Site") {
        if fetch_site.eq_ignore_ascii_case("cross-site") {
            let _ = request.respond(Response::from_string("{}").with_status_code(403));
            return;
        }
    }
    if let Some(origin) = get_header_value(&request, "Origin") {
        if !is_loopback_origin(origin) {
            let _ = request.respond(Response::from_string("{}").with_status_code(403));
            return;
        }
    }

    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        let _ = request.respond(Response::from_string("{}").with_status_code(400));
        return;
    }
    if body.trim().is_empty() {
        let _ = request.respond(Response::from_string("{}").with_status_code(400));
        return;
    }

    let payload: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            let _ = request.respond(Response::from_string("{}").with_status_code(400));
            return;
        }
    };
    let connection_id = get_header_value(&request, crate::rpc_transport::RPC_CONNECTION_ID_HEADER)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let (status, response_body, success) = handle_rpc_payload(payload, connection_id);
    if success {
        rpc_metrics_guard.mark_success();
    }
    let _ = request.respond(Response::from_string(response_body).with_status_code(status));
}
