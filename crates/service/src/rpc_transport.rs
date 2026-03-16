use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

pub(crate) const RPC_CONNECTION_ID_HEADER: &str = "X-CodexManager-Rpc-Connection-Id";

static RPC_CONNECTION_COUNTER: AtomicU64 = AtomicU64::new(1);

fn rpc_connections() -> &'static RwLock<BTreeMap<String, UnboundedSender<Value>>> {
    static CONNECTIONS: OnceLock<RwLock<BTreeMap<String, UnboundedSender<Value>>>> =
        OnceLock::new();
    CONNECTIONS.get_or_init(|| RwLock::new(BTreeMap::new()))
}

#[cfg(test)]
pub(crate) fn clear_connections_for_tests() {
    crate::lock_utils::write_recover(rpc_connections(), "rpc_connections").clear();
}

#[cfg(test)]
fn rpc_transport_test_lock() -> &'static std::sync::Mutex<()> {
    static TEST_LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    TEST_LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
pub(crate) fn lock_rpc_transport_tests() -> std::sync::MutexGuard<'static, ()> {
    rpc_transport_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) struct RpcConnectionStream {
    connection_id: String,
    receiver: UnboundedReceiver<Value>,
}

impl RpcConnectionStream {
    pub(crate) fn into_parts(self) -> (String, UnboundedReceiver<Value>) {
        (self.connection_id, self.receiver)
    }
}

pub(crate) fn open_connection() -> RpcConnectionStream {
    let connection_id = format!(
        "rpc-{}",
        RPC_CONNECTION_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let (sender, receiver) = unbounded_channel::<Value>();
    crate::lock_utils::write_recover(rpc_connections(), "rpc_connections")
        .insert(connection_id.clone(), sender);
    RpcConnectionStream {
        connection_id,
        receiver,
    }
}

pub(crate) fn close_connection(connection_id: &str) {
    crate::lock_utils::write_recover(rpc_connections(), "rpc_connections").remove(connection_id);
    crate::rpc_dispatch::remove_connection_session(connection_id);
    crate::thread_turn::remove_connection(connection_id);
}

pub(crate) fn connection_opened_event(connection_id: &str) -> Value {
    json!({
        "connectionId": connection_id
    })
}

#[allow(dead_code)]
pub(crate) fn send_notification_to_connection(
    connection_id: &str,
    method: &str,
    params: Value,
) -> bool {
    if !crate::rpc_dispatch::connection_accepts_notification(connection_id, method) {
        return false;
    }
    let sender = crate::lock_utils::read_recover(rpc_connections(), "rpc_connections")
        .get(connection_id)
        .cloned();
    let Some(sender) = sender else {
        return false;
    };
    let payload = json!({
        "method": method,
        "params": params,
    });
    if sender.send(payload).is_ok() {
        true
    } else {
        close_connection(connection_id);
        false
    }
}

pub(crate) fn broadcast_notification(method: &str, params: Value) -> usize {
    let targets = crate::lock_utils::read_recover(rpc_connections(), "rpc_connections")
        .iter()
        .filter(|(connection_id, _)| {
            crate::rpc_dispatch::connection_accepts_notification(connection_id, method)
        })
        .map(|(connection_id, sender)| (connection_id.clone(), sender.clone()))
        .collect::<Vec<_>>();
    let payload = json!({
        "method": method,
        "params": params,
    });
    let mut delivered = 0usize;
    let mut stale = Vec::new();
    for (connection_id, sender) in targets {
        if sender.send(payload.clone()).is_ok() {
            delivered = delivered.saturating_add(1);
        } else {
            stale.push(connection_id);
        }
    }
    for connection_id in stale {
        close_connection(connection_id.as_str());
    }
    delivered
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::rpc::types::JsonRpcRequest;
    use serde_json::json;

    fn initialize_request(id: u64) -> JsonRpcRequest {
        JsonRpcRequest {
            id,
            method: "initialize".to_string(),
            params: Some(json!({
                "capabilities": {
                    "optOutNotificationMethods": ["skills/changed"]
                }
            })),
        }
    }

    fn ready_connection(connection_id: &str) {
        let ctx = crate::rpc_dispatch::RpcRequestContext {
            connection_id: Some(connection_id.to_string()),
        };
        let _ = crate::handle_request_with_context(initialize_request(1), &ctx);
        crate::rpc_dispatch::handle_notification_with_context("initialized", None, &ctx)
            .expect("initialized notification");
    }

    #[test]
    fn broadcast_notification_requires_ready_connection() {
        let _guard = lock_rpc_transport_tests();
        clear_connections_for_tests();
        crate::rpc_dispatch::clear_connection_sessions_for_tests();
        let (connection_id, mut receiver) = open_connection().into_parts();

        assert_eq!(
            broadcast_notification("account/updated", json!({"authMode": "chatgpt"})),
            0
        );
        assert!(receiver.try_recv().is_err());

        ready_connection(connection_id.as_str());
        assert_eq!(
            broadcast_notification("account/updated", json!({"authMode": "chatgpt"})),
            1
        );
        assert_eq!(
            receiver.try_recv().expect("notification"),
            json!({
                "method": "account/updated",
                "params": { "authMode": "chatgpt" }
            })
        );

        close_connection(connection_id.as_str());
        clear_connections_for_tests();
        crate::rpc_dispatch::clear_connection_sessions_for_tests();
    }

    #[test]
    fn broadcast_notification_respects_opt_out_methods() {
        let _guard = lock_rpc_transport_tests();
        clear_connections_for_tests();
        crate::rpc_dispatch::clear_connection_sessions_for_tests();
        let (connection_id, mut receiver) = open_connection().into_parts();
        ready_connection(connection_id.as_str());

        assert_eq!(broadcast_notification("skills/changed", json!({})), 0);
        assert!(receiver.try_recv().is_err());

        close_connection(connection_id.as_str());
        clear_connections_for_tests();
        crate::rpc_dispatch::clear_connection_sessions_for_tests();
    }
}
