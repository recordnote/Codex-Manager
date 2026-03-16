use crate::rpc_transport::RPC_CONNECTION_ID_HEADER;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream;
use serde_json::Value;
use std::convert::Infallible;
use std::time::Duration;
use url::Url;

struct RpcConnectionStreamGuard {
    connection_id: String,
}

impl Drop for RpcConnectionStreamGuard {
    fn drop(&mut self) {
        crate::rpc_transport::close_connection(self.connection_id.as_str());
    }
}

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

fn event_from_value(value: Value, event_name: Option<&str>) -> Result<Event, Infallible> {
    let json = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    let event = match event_name {
        Some(name) => Event::default().event(name).data(json),
        None => Event::default().data(json),
    };
    Ok(event)
}

pub(crate) async fn handle_rpc_events(headers: HeaderMap) -> Response {
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

    let connection = crate::rpc_transport::open_connection();
    let (connection_id, receiver) = connection.into_parts();
    let connection_header_value = HeaderValue::from_str(connection_id.as_str())
        .unwrap_or_else(|_| HeaderValue::from_static("invalid-connection-id"));
    let initial_event = crate::rpc_transport::connection_opened_event(connection_id.as_str());
    let guard = RpcConnectionStreamGuard {
        connection_id: connection_id.clone(),
    };
    let stream = stream::unfold(
        (receiver, Some(initial_event), guard),
        |(mut receiver, mut pending, guard)| async move {
            if let Some(initial) = pending.take() {
                return Some((
                    event_from_value(initial, Some("connection")),
                    (receiver, pending, guard),
                ));
            }
            match receiver.recv().await {
                Some(value) => Some((event_from_value(value, None), (receiver, pending, guard))),
                None => None,
            }
        },
    );

    let sse = Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive"),
    );

    let mut response = sse.into_response();
    response
        .headers_mut()
        .insert(RPC_CONNECTION_ID_HEADER, connection_header_value);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::CONTENT_TYPE;

    fn header_map(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(
                axum::http::header::HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                HeaderValue::from_str(value).expect("header value"),
            );
        }
        headers
    }

    fn run_async<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(future)
    }

    #[test]
    fn rpc_events_requires_token() {
        let _guard = crate::rpc_transport::lock_rpc_transport_tests();
        let response = run_async(handle_rpc_events(HeaderMap::new()));
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn rpc_events_rejects_cross_site_origin() {
        let _guard = crate::rpc_transport::lock_rpc_transport_tests();
        let token = crate::rpc_auth_token().to_string();
        let headers = header_map(&[
            ("X-CodexManager-Rpc-Token", token.as_str()),
            ("Origin", "https://evil.example"),
            ("Sec-Fetch-Site", "cross-site"),
        ]);
        let response = run_async(handle_rpc_events(headers));
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn rpc_events_accepts_loopback_origin_and_sets_connection_header() {
        let _guard = crate::rpc_transport::lock_rpc_transport_tests();
        crate::rpc_transport::clear_connections_for_tests();
        crate::rpc_dispatch::clear_connection_sessions_for_tests();

        let token = crate::rpc_auth_token().to_string();
        let headers = header_map(&[
            ("X-CodexManager-Rpc-Token", token.as_str()),
            ("Origin", "http://localhost:3005"),
            ("Sec-Fetch-Site", "same-site"),
        ]);
        let response = run_async(handle_rpc_events(headers));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
        assert!(response.headers().contains_key(RPC_CONNECTION_ID_HEADER));

        drop(response);
        crate::rpc_transport::clear_connections_for_tests();
        crate::rpc_dispatch::clear_connection_sessions_for_tests();
    }
}
