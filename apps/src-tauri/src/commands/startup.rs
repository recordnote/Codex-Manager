use crate::app_storage::apply_runtime_storage_env;
use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_startup_snapshot(
    app: tauri::AppHandle,
    addr: Option<String>,
    request_log_limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    apply_runtime_storage_env(&app);
    let params = request_log_limit.map(|value| serde_json::json!({ "requestLogLimit": value }));
    rpc_call_in_background("startup/snapshot", addr, params).await
}
