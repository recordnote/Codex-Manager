use super::{
    build_backend_base_url, build_front_proxy_app, build_local_backend_client, proxy_handler,
    ProxyState,
};
use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{Request as HttpRequest, StatusCode};
use codexmanager_core::storage::{now_ts, Account, Storage, Token, UsageSnapshotRecord};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex, MutexGuard};
use std::thread;
use tiny_http::{Header, Response, Server, StatusCode as TinyStatusCode};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

struct GatewayRuntimeSettingGuard {
    free_account_max_model: String,
    request_compression_enabled: bool,
}

impl GatewayRuntimeSettingGuard {
    fn capture() -> Self {
        Self {
            free_account_max_model: crate::gateway::current_free_account_max_model(),
            request_compression_enabled: crate::gateway::request_compression_enabled(),
        }
    }
}

impl Drop for GatewayRuntimeSettingGuard {
    fn drop(&mut self) {
        let _ = crate::gateway::set_free_account_max_model(self.free_account_max_model.as_str());
        let _ = crate::gateway::set_request_compression_enabled(self.request_compression_enabled);
    }
}

struct ReloadRuntimeConfigGuard;

impl Drop for ReloadRuntimeConfigGuard {
    fn drop(&mut self) {
        crate::gateway::reload_runtime_config_from_env();
    }
}

static HTTP_PROXY_RUNTIME_TEST_ENV_LOCK: Mutex<()> = Mutex::new(());
static HTTP_PROXY_RUNTIME_TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn lock_http_proxy_runtime_test_env() -> MutexGuard<'static, ()> {
    HTTP_PROXY_RUNTIME_TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn new_http_proxy_runtime_test_dir(prefix: &str) -> PathBuf {
    let seq = HTTP_PROXY_RUNTIME_TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("{prefix}-{}-{seq}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

#[derive(Debug)]
struct CapturedUpstreamRequest {
    path: String,
    body: String,
}

fn start_mock_sse_upstream_once(
    response_body: &str,
) -> (
    String,
    mpsc::Receiver<CapturedUpstreamRequest>,
    thread::JoinHandle<()>,
) {
    let server = Server::http("127.0.0.1:0").expect("start mock upstream server");
    let addr = format!(
        "http://{}/chatgpt.com/backend-api/codex",
        server.server_addr()
    );
    let response_body = response_body.to_string();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let mut request = server.recv().expect("receive upstream request");
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .expect("read upstream body");
        tx.send(CapturedUpstreamRequest {
            path: request.url().to_string(),
            body,
        })
        .expect("send upstream request");
        let response = Response::from_string(response_body)
            .with_status_code(TinyStatusCode(200))
            .with_header(
                Header::from_bytes("Content-Type", "text/event-stream")
                    .expect("content-type header"),
            );
        request.respond(response).expect("respond upstream request");
    });
    (addr, rx, handle)
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

#[test]
fn backend_base_url_uses_http_scheme() {
    assert_eq!(
        build_backend_base_url("127.0.0.1:18080"),
        "http://127.0.0.1:18080"
    );
}

#[test]
fn local_backend_client_builds_without_system_proxy() {
    build_local_backend_client().expect("local backend client");
}

#[test]
fn request_without_content_length_over_limit_returns_413() {
    let _guard = EnvGuard::set("CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES", "8");
    crate::gateway::reload_runtime_config_from_env();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let request = HttpRequest::builder()
        .method("POST")
        .uri("/rpc")
        .body(Body::from(vec![b'x'; 9]))
        .expect("request");

    let response = runtime.block_on(proxy_handler(State(state), request));
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = runtime
        .block_on(to_bytes(response.into_body(), usize::MAX))
        .expect("read body");
    let text = String::from_utf8(body.to_vec()).expect("utf8");
    assert!(text.contains("request body too large: content-length>8"));
}

#[test]
fn backend_send_failure_returns_502() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let request = HttpRequest::builder()
        .method("GET")
        .uri("/rpc")
        .body(Body::empty())
        .expect("request");

    let response = runtime.block_on(proxy_handler(State(state), request));
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let error_code = response
        .headers()
        .get(crate::error_codes::ERROR_CODE_HEADER_NAME)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = runtime
        .block_on(to_bytes(response.into_body(), usize::MAX))
        .expect("read body");
    let text = String::from_utf8(body.to_vec()).expect("utf8");
    assert_eq!(error_code.as_deref(), Some("backend_proxy_error"));
    assert!(
        text.contains("backend proxy error:"),
        "unexpected body: {text}"
    );
}

#[test]
fn rpc_post_roundtrip_uses_front_handler_and_returns_initialize() {
    let _guard = crate::rpc_transport::lock_rpc_transport_tests();
    crate::rpc_transport::clear_connections_for_tests();
    crate::rpc_dispatch::clear_connection_sessions_for_tests();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let state = ProxyState {
            backend_base_url: "http://127.0.0.1:1".to_string(),
            client: Client::new(),
        };
        let app = build_front_proxy_app(state);
        let token = crate::rpc_auth_token().to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve app");
        });

        let response = Client::new()
            .post(format!("http://{addr}/rpc"))
            .header("Content-Type", "application/json")
            .header("X-CodexManager-Rpc-Token", token)
            .json(&serde_json::json!({
                "id": 1,
                "method": "initialize",
                "params": {}
            }))
            .send()
            .await
            .expect("rpc response");
        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value = response.json().await.expect("initialize response json");
        assert_eq!(
            payload
                .get("result")
                .and_then(|value| value.get("server_name"))
                .and_then(|value| value.as_str()),
            Some("codexmanager-service")
        );

        server.abort();
    });
}

#[test]
fn rpc_websocket_roundtrip_supports_requests_and_notifications() {
    let _guard = crate::rpc_transport::lock_rpc_transport_tests();
    crate::rpc_transport::clear_connections_for_tests();
    crate::rpc_dispatch::clear_connection_sessions_for_tests();
    crate::thread_turn::clear_for_tests();
    crate::gateway::reload_runtime_config_from_env();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let state = ProxyState {
            backend_base_url: "http://127.0.0.1:1".to_string(),
            client: Client::new(),
        };
        let app = build_front_proxy_app(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve app");
        });

        let token = crate::rpc_auth_token().to_string();
        let mut request = format!("ws://{addr}/rpc")
            .into_client_request()
            .expect("client request");
        request.headers_mut().insert(
            "X-CodexManager-Rpc-Token",
            token.parse().expect("rpc token header"),
        );
        request.headers_mut().insert(
            "Origin",
            "http://localhost:3005".parse().expect("origin header"),
        );
        request.headers_mut().insert(
            "Sec-Fetch-Site",
            "same-site".parse().expect("fetch site header"),
        );

        let (mut socket, _) = connect_async(request).await.expect("connect websocket");

        socket
            .send(Message::Text(
                serde_json::json!({
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "capabilities": {
                            "optOutNotificationMethods": ["account/updated"]
                        }
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send initialize");
        let initialize_message = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("initialize timeout")
            .expect("initialize frame")
            .expect("initialize message");
        let initialize_text = initialize_message.into_text().expect("initialize text");
        let initialize_value =
            serde_json::from_str::<serde_json::Value>(&initialize_text).expect("initialize json");
        assert_eq!(
            initialize_value["result"]["server_name"],
            serde_json::json!("codexmanager-service")
        );

        socket
            .send(Message::Text(
                r#"{"method":"initialized"}"#.to_string().into(),
            ))
            .await
            .expect("send initialized notification");

        socket
            .send(Message::Text(
                serde_json::json!({
                    "id": 2,
                    "method": "skills/list",
                    "params": { "cwds": [] }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send skills list");
        let skills_message = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("skills/list timeout")
            .expect("skills/list frame")
            .expect("skills/list message");
        let skills_value = serde_json::from_str::<serde_json::Value>(
            &skills_message.into_text().expect("skills list text"),
        )
        .expect("skills/list json");
        assert!(skills_value["result"]["data"].is_array());

        crate::rpc_notifications::notify_account_updated();
        crate::rpc_notifications::notify_skills_changed();

        let notification_message = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("notification timeout")
            .expect("notification frame")
            .expect("notification message");
        let notification_value = serde_json::from_str::<serde_json::Value>(
            &notification_message.into_text().expect("notification text"),
        )
        .expect("notification json");
        assert_eq!(
            notification_value["method"],
            serde_json::json!("skills/changed")
        );

        socket.close(None).await.expect("close websocket");
        server.abort();
    });

    crate::rpc_transport::clear_connections_for_tests();
    crate::rpc_dispatch::clear_connection_sessions_for_tests();
    crate::thread_turn::clear_for_tests();
}

#[test]
fn rpc_websocket_thread_turn_runtime_emits_lifecycle_notifications() {
    let _guard = crate::rpc_transport::lock_rpc_transport_tests();
    crate::rpc_transport::clear_connections_for_tests();
    crate::rpc_dispatch::clear_connection_sessions_for_tests();
    crate::thread_turn::clear_for_tests();
    crate::gateway::reload_runtime_config_from_env();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let state = ProxyState {
            backend_base_url: "http://127.0.0.1:1".to_string(),
            client: Client::new(),
        };
        let app = build_front_proxy_app(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve app");
        });

        let token = crate::rpc_auth_token().to_string();
        let mut request = format!("ws://{addr}/rpc")
            .into_client_request()
            .expect("client request");
        request.headers_mut().insert(
            "X-CodexManager-Rpc-Token",
            token.parse().expect("rpc token header"),
        );
        request.headers_mut().insert(
            "Origin",
            "http://localhost:3005".parse().expect("origin header"),
        );
        request.headers_mut().insert(
            "Sec-Fetch-Site",
            "same-site".parse().expect("fetch site header"),
        );

        let (mut socket, _) = connect_async(request).await.expect("connect websocket");

        socket
            .send(Message::Text(
                serde_json::json!({
                    "id": 1,
                    "method": "initialize",
                    "params": {}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send initialize");
        let _ = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("initialize timeout")
            .expect("initialize frame")
            .expect("initialize message");

        socket
            .send(Message::Text(
                r#"{"method":"initialized"}"#.to_string().into(),
            ))
            .await
            .expect("send initialized notification");

        socket
            .send(Message::Text(
                serde_json::json!({
                    "id": 2,
                    "method": "thread/start",
                    "params": {
                        "model": "gpt-5.4",
                        "cwd": "D:/tmp/project",
                        "ephemeral": true
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send thread/start");
        let thread_start_response = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("thread/start response timeout")
            .expect("thread/start response frame")
            .expect("thread/start response message");
        let thread_start_value = serde_json::from_str::<serde_json::Value>(
            &thread_start_response
                .into_text()
                .expect("thread/start response text"),
        )
        .expect("thread/start response json");
        let thread_id = thread_start_value["result"]["thread"]["id"]
            .as_str()
            .expect("thread id")
            .to_string();
        assert_eq!(
            thread_start_value["result"]["model"],
            serde_json::json!("gpt-5.4")
        );
        assert_eq!(
            thread_start_value["result"]["thread"]["ephemeral"],
            serde_json::json!(true)
        );

        let thread_started_notification = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("thread/started timeout")
            .expect("thread/started frame")
            .expect("thread/started message");
        let thread_started_value = serde_json::from_str::<serde_json::Value>(
            &thread_started_notification
                .into_text()
                .expect("thread/started text"),
        )
        .expect("thread/started json");
        assert_eq!(
            thread_started_value["method"],
            serde_json::json!("thread/started")
        );
        assert_eq!(
            thread_started_value["params"]["thread"]["id"],
            serde_json::json!(thread_id.clone())
        );

        socket
            .send(Message::Text(
                serde_json::json!({
                    "id": 3,
                    "method": "turn/start",
                    "params": {
                        "threadId": thread_id,
                        "input": [{ "type": "text", "text": "hello ws", "textElements": [] }]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send turn/start");
        let turn_start_response = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("turn/start response timeout")
            .expect("turn/start response frame")
            .expect("turn/start response message");
        let turn_start_value = serde_json::from_str::<serde_json::Value>(
            &turn_start_response
                .into_text()
                .expect("turn/start response text"),
        )
        .expect("turn/start response json");
        let turn_id = turn_start_value["result"]["turn"]["id"]
            .as_str()
            .expect("turn id")
            .to_string();
        assert_eq!(
            turn_start_value["result"]["turn"]["status"],
            serde_json::json!("inProgress")
        );

        let thread_status_active = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("thread/status/changed active timeout")
            .expect("thread/status/changed active frame")
            .expect("thread/status/changed active message");
        let thread_status_active_value = serde_json::from_str::<serde_json::Value>(
            &thread_status_active
                .into_text()
                .expect("thread/status/changed active text"),
        )
        .expect("thread/status/changed active json");
        assert_eq!(
            thread_status_active_value["method"],
            serde_json::json!("thread/status/changed")
        );
        assert_eq!(
            thread_status_active_value["params"]["status"]["type"],
            serde_json::json!("active")
        );

        let turn_started = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("turn/started timeout")
            .expect("turn/started frame")
            .expect("turn/started message");
        let turn_started_value = serde_json::from_str::<serde_json::Value>(
            &turn_started.into_text().expect("turn/started text"),
        )
        .expect("turn/started json");
        assert_eq!(
            turn_started_value["method"],
            serde_json::json!("turn/started")
        );
        assert_eq!(
            turn_started_value["params"]["turn"]["id"],
            serde_json::json!(turn_id.clone())
        );

        let item_started = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("item/started timeout")
            .expect("item/started frame")
            .expect("item/started message");
        let item_started_value = serde_json::from_str::<serde_json::Value>(
            &item_started.into_text().expect("item/started text"),
        )
        .expect("item/started json");
        assert_eq!(
            item_started_value["method"],
            serde_json::json!("item/started")
        );
        assert_eq!(
            item_started_value["params"]["item"]["type"],
            serde_json::json!("userMessage")
        );

        let item_completed = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("item/completed timeout")
            .expect("item/completed frame")
            .expect("item/completed message");
        let item_completed_value = serde_json::from_str::<serde_json::Value>(
            &item_completed.into_text().expect("item/completed text"),
        )
        .expect("item/completed json");
        assert_eq!(
            item_completed_value["method"],
            serde_json::json!("item/completed")
        );

        socket
            .send(Message::Text(
                serde_json::json!({
                    "id": 4,
                    "method": "turn/interrupt",
                    "params": {
                        "threadId": thread_id,
                        "turnId": turn_id
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send turn/interrupt");
        let turn_interrupt_response = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("turn/interrupt response timeout")
            .expect("turn/interrupt response frame")
            .expect("turn/interrupt response message");
        let turn_interrupt_value = serde_json::from_str::<serde_json::Value>(
            &turn_interrupt_response
                .into_text()
                .expect("turn/interrupt response text"),
        )
        .expect("turn/interrupt response json");
        assert_eq!(turn_interrupt_value["result"], serde_json::json!({}));

        let turn_completed = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("turn/completed timeout")
            .expect("turn/completed frame")
            .expect("turn/completed message");
        let turn_completed_value = serde_json::from_str::<serde_json::Value>(
            &turn_completed.into_text().expect("turn/completed text"),
        )
        .expect("turn/completed json");
        assert_eq!(
            turn_completed_value["method"],
            serde_json::json!("turn/completed")
        );
        assert_eq!(
            turn_completed_value["params"]["turn"]["status"],
            serde_json::json!("interrupted")
        );

        let thread_status_idle = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("thread/status/changed idle timeout")
            .expect("thread/status/changed idle frame")
            .expect("thread/status/changed idle message");
        let thread_status_idle_value = serde_json::from_str::<serde_json::Value>(
            &thread_status_idle
                .into_text()
                .expect("thread/status/changed idle text"),
        )
        .expect("thread/status/changed idle json");
        assert_eq!(
            thread_status_idle_value["method"],
            serde_json::json!("thread/status/changed")
        );
        assert_eq!(
            thread_status_idle_value["params"]["status"]["type"],
            serde_json::json!("idle")
        );

        socket.close(None).await.expect("close websocket");
        server.abort();
    });

    crate::rpc_transport::clear_connections_for_tests();
    crate::rpc_dispatch::clear_connection_sessions_for_tests();
    crate::thread_turn::clear_for_tests();
}

#[test]
fn rpc_websocket_thread_turn_runtime_streams_agent_deltas_and_logs_success() {
    let _transport_guard = crate::rpc_transport::lock_rpc_transport_tests();
    let _env_lock = lock_http_proxy_runtime_test_env();
    let _runtime_reload_guard = ReloadRuntimeConfigGuard;
    let _runtime_setting_guard = GatewayRuntimeSettingGuard::capture();
    crate::rpc_transport::clear_connections_for_tests();
    crate::rpc_dispatch::clear_connection_sessions_for_tests();
    crate::thread_turn::clear_for_tests();

    let dir = new_http_proxy_runtime_test_dir("rpc-thread-turn-success");
    let db_path = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let sse_body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello \"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"world\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_turn_1\",\"model\":\"gpt-5.2\",\"usage\":{\"input_tokens\":12,\"output_tokens\":4,\"total_tokens\":16},\"output\":[{\"content\":[{\"type\":\"output_text\",\"text\":\"hello world\"}]}]}}\n\n",
        "data: [DONE]\n\n"
    );
    let (upstream_base, upstream_rx, upstream_join) = start_mock_sse_upstream_once(sse_body);
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);
    crate::gateway::reload_runtime_config_from_env();
    let _ = crate::gateway::set_request_compression_enabled(false);
    let _ = crate::gateway::set_free_account_max_model("gpt-5.2");
    crate::initialize_storage_if_needed().expect("initialize storage");

    let storage = Storage::open(&db_path).expect("open storage");
    storage.init().expect("init schema");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-free".to_string(),
            label: "Free Account".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt-free-1".to_string()),
            workspace_id: Some("workspace-free-1".to_string()),
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc-free".to_string(),
            id_token: "header.payload.sig".to_string(),
            access_token: "access-token".to_string(),
            refresh_token: "refresh-token".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-free".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(20.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: None,
            credits_json: Some(r#"{"planType":"free"}"#.to_string()),
            captured_at: now,
        })
        .expect("insert usage snapshot");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let state = ProxyState {
            backend_base_url: "http://127.0.0.1:1".to_string(),
            client: Client::new(),
        };
        let app = build_front_proxy_app(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve app");
        });

        let token = crate::rpc_auth_token().to_string();
        let mut request = format!("ws://{addr}/rpc")
            .into_client_request()
            .expect("client request");
        request.headers_mut().insert(
            "X-CodexManager-Rpc-Token",
            token.parse().expect("rpc token header"),
        );
        request.headers_mut().insert(
            "Origin",
            "http://localhost:3005".parse().expect("origin header"),
        );
        request.headers_mut().insert(
            "Sec-Fetch-Site",
            "same-site".parse().expect("fetch site header"),
        );

        let (mut socket, _) = connect_async(request).await.expect("connect websocket");

        socket
            .send(Message::Text(
                serde_json::json!({
                    "id": 1,
                    "method": "initialize",
                    "params": {}
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send initialize");
        let _ = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("initialize timeout")
            .expect("initialize frame")
            .expect("initialize message");

        socket
            .send(Message::Text(
                r#"{"method":"initialized"}"#.to_string().into(),
            ))
            .await
            .expect("send initialized");

        socket
            .send(Message::Text(
                serde_json::json!({
                    "id": 2,
                    "method": "thread/start",
                    "params": {
                        "model": "gpt-5.4",
                        "cwd": "D:/tmp/project",
                        "ephemeral": true
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send thread/start");
        let thread_start_response = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("thread/start timeout")
            .expect("thread/start frame")
            .expect("thread/start message");
        let thread_start_value = serde_json::from_str::<serde_json::Value>(
            &thread_start_response
                .into_text()
                .expect("thread/start text"),
        )
        .expect("thread/start json");
        let thread_id = thread_start_value["result"]["thread"]["id"]
            .as_str()
            .expect("thread id")
            .to_string();

        let _thread_started = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("thread/started timeout")
            .expect("thread/started frame")
            .expect("thread/started message");

        socket
            .send(Message::Text(
                serde_json::json!({
                    "id": 3,
                    "method": "turn/start",
                    "params": {
                        "threadId": thread_id,
                        "input": [{ "type": "text", "text": "hello ws", "textElements": [] }]
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send turn/start");
        let turn_start_response = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("turn/start timeout")
            .expect("turn/start frame")
            .expect("turn/start message");
        let turn_start_value = serde_json::from_str::<serde_json::Value>(
            &turn_start_response.into_text().expect("turn/start text"),
        )
        .expect("turn/start json");
        let turn_id = turn_start_value["result"]["turn"]["id"]
            .as_str()
            .expect("turn id")
            .to_string();

        let expected_methods = [
            "thread/status/changed",
            "turn/started",
            "item/started",
            "item/completed",
            "item/started",
            "item/agentMessage/delta",
            "item/agentMessage/delta",
            "thread/tokenUsage/updated",
            "item/completed",
            "turn/completed",
            "thread/status/changed",
        ];
        let mut events = Vec::new();
        for _ in expected_methods {
            let message = timeout(Duration::from_secs(3), socket.next())
                .await
                .expect("event timeout")
                .expect("event frame")
                .expect("event message");
            let value = serde_json::from_str::<serde_json::Value>(
                &message.into_text().expect("event text"),
            )
            .expect("event json");
            events.push(value);
        }

        let methods = events
            .iter()
            .map(|value| value["method"].as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            expected_methods
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            events[0]["params"]["status"]["type"],
            serde_json::json!("active")
        );
        assert_eq!(
            events[1]["params"]["turn"]["id"],
            serde_json::json!(turn_id.clone())
        );
        assert_eq!(
            events[2]["params"]["item"]["type"],
            serde_json::json!("userMessage")
        );
        assert_eq!(
            events[4]["params"]["item"]["type"],
            serde_json::json!("agentMessage")
        );
        assert_eq!(events[5]["params"]["delta"], serde_json::json!("hello "));
        assert_eq!(events[6]["params"]["delta"], serde_json::json!("world"));
        assert_eq!(
            events[7]["params"]["turnId"],
            serde_json::json!(turn_id.clone())
        );
        assert_eq!(
            events[7]["params"]["tokenUsage"]["last"]["totalTokens"],
            serde_json::json!(16)
        );
        assert_eq!(
            events[7]["params"]["tokenUsage"]["last"]["inputTokens"],
            serde_json::json!(12)
        );
        assert_eq!(
            events[7]["params"]["tokenUsage"]["last"]["outputTokens"],
            serde_json::json!(4)
        );
        assert_eq!(
            events[7]["params"]["tokenUsage"]["total"]["totalTokens"],
            serde_json::json!(16)
        );
        assert_eq!(
            events[8]["params"]["item"]["text"],
            serde_json::json!("hello world")
        );
        assert_eq!(
            events[9]["params"]["turn"]["status"],
            serde_json::json!("completed")
        );
        assert_eq!(
            events[10]["params"]["status"]["type"],
            serde_json::json!("idle")
        );

        socket.close(None).await.expect("close websocket");
        server.abort();
    });

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive upstream request");
    let captured_body =
        serde_json::from_str::<serde_json::Value>(&captured.body).expect("parse upstream body");
    assert!(
        captured.path.ends_with("/responses"),
        "unexpected path: {}",
        captured.path
    );
    assert_eq!(captured_body["model"], serde_json::json!("gpt-5.2"));
    assert_eq!(captured_body["stream"], serde_json::json!(true));
    assert_eq!(
        captured_body["input"][0]["content"][0]["text"],
        serde_json::json!("hello ws")
    );

    upstream_join.join().expect("join mock upstream");

    let storage = Storage::open(&db_path).expect("reopen storage");
    let logs = storage
        .list_request_logs_paginated(None, None, 0, 10)
        .expect("list request logs");
    let latest = logs.first().expect("latest request log");
    assert_eq!(latest.request_path, "/v1/responses");
    assert_eq!(latest.account_id.as_deref(), Some("acc-free"));
    assert_eq!(latest.initial_account_id.as_deref(), Some("acc-free"));
    assert_eq!(latest.model.as_deref(), Some("gpt-5.2"));
    assert_eq!(latest.status_code, Some(200));
    assert_eq!(latest.total_tokens, Some(16));
    assert!(latest.error.as_deref().unwrap_or_default().is_empty());

    crate::rpc_transport::clear_connections_for_tests();
    crate::rpc_dispatch::clear_connection_sessions_for_tests();
    crate::thread_turn::clear_for_tests();
    let _ = fs::remove_dir_all(&dir);
}
