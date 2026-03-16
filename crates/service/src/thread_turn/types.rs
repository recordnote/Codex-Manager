use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadWire {
    pub(crate) id: String,
    pub(crate) preview: String,
    pub(crate) ephemeral: bool,
    pub(crate) model_provider: String,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) status: ThreadStatusWire,
    pub(crate) path: Option<String>,
    pub(crate) cwd: String,
    pub(crate) cli_version: String,
    pub(crate) source: SessionSourceWire,
    pub(crate) agent_nickname: Option<String>,
    pub(crate) agent_role: Option<String>,
    pub(crate) git_info: Option<Value>,
    pub(crate) name: Option<String>,
    pub(crate) turns: Vec<TurnWire>,
}

#[derive(Debug, Clone)]
pub(crate) struct ThreadSessionConfig {
    pub(crate) model: String,
    pub(crate) model_provider: String,
    pub(crate) service_tier: Option<String>,
    pub(crate) cwd: String,
    pub(crate) approval_policy: String,
    pub(crate) approvals_reviewer: Option<Value>,
    pub(crate) sandbox: Value,
    pub(crate) reasoning_effort: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub(crate) enum ThreadStatusWire {
    NotLoaded,
    Idle,
    SystemError,
    Active {
        active_flags: Vec<ThreadActiveFlagWire>,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ThreadActiveFlagWire {
    WaitingOnApproval,
    WaitingOnUserInput,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SessionSourceWire {
    AppServer,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnWire {
    pub(crate) id: String,
    pub(crate) items: Vec<Value>,
    pub(crate) status: TurnStatusWire,
    pub(crate) error: Option<TurnErrorWire>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TurnStatusWire {
    Completed,
    Interrupted,
    Failed,
    InProgress,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnErrorWire {
    pub(crate) message: String,
    pub(crate) codex_error_info: Option<Value>,
    pub(crate) additional_details: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TokenUsageBreakdownWire {
    pub(crate) total_tokens: i64,
    pub(crate) input_tokens: i64,
    pub(crate) cached_input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) reasoning_output_tokens: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadTokenUsageWire {
    pub(crate) total: TokenUsageBreakdownWire,
    pub(crate) last: TokenUsageBreakdownWire,
    pub(crate) model_context_window: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ThreadStartParams {
    pub(crate) model: Option<String>,
    pub(crate) service_tier: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) approval_policy: Option<String>,
    pub(crate) approvals_reviewer: Option<Value>,
    pub(crate) sandbox: Option<Value>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) ephemeral: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ThreadResumeParams {
    pub(crate) thread_id: String,
    pub(crate) model: Option<String>,
    pub(crate) service_tier: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) approval_policy: Option<String>,
    pub(crate) approvals_reviewer: Option<Value>,
    pub(crate) sandbox: Option<Value>,
    pub(crate) reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ThreadForkParams {
    pub(crate) thread_id: String,
    pub(crate) model: Option<String>,
    pub(crate) service_tier: Option<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) approval_policy: Option<String>,
    pub(crate) approvals_reviewer: Option<Value>,
    pub(crate) sandbox: Option<Value>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) ephemeral: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ThreadReadParams {
    pub(crate) thread_id: String,
    pub(crate) include_turns: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ThreadNameSetParams {
    pub(crate) thread_id: String,
    pub(crate) name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ThreadCompactStartParams {
    pub(crate) thread_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TurnStartParams {
    pub(crate) thread_id: String,
    pub(crate) input: Vec<Value>,
    pub(crate) model: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TurnInterruptParams {
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TurnSteerParams {
    pub(crate) thread_id: String,
    pub(crate) expected_turn_id: String,
    pub(crate) input: Vec<Value>,
}
