use serde_json::json;

pub(crate) fn notify_account_login_completed(
    login_id: Option<&str>,
    success: bool,
    error: Option<&str>,
) {
    let _ = crate::rpc_transport::broadcast_notification(
        "account/login/completed",
        json!({
            "loginId": login_id,
            "success": success,
            "error": error,
        }),
    );
}

pub(crate) fn notify_account_updated() {
    let payload = crate::auth_account::account_updated_notification_payload();
    let _ = crate::rpc_transport::broadcast_notification("account/updated", payload);
}

pub(crate) fn notify_account_rate_limits_updated(account_id: &str) {
    let Some(payload) =
        crate::auth_account::current_rate_limits_notification_payload_for_account(account_id)
    else {
        return;
    };
    let _ = crate::rpc_transport::broadcast_notification("account/rateLimits/updated", payload);
}

pub(crate) fn notify_skills_changed() {
    let _ = crate::rpc_transport::broadcast_notification("skills/changed", json!({}));
}
