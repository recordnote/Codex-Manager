use tiny_http::{Header, Request};

#[path = "http_bridge/output_text.rs"]
mod output_text;

use output_text::{
    append_output_text, append_output_text_raw, collect_output_text_from_event_fields,
    collect_response_output_text, extract_error_hint_from_body, merge_usage,
    parse_usage_from_json, reload_from_env as reload_output_text_from_env, usage_has_signal,
    UpstreamResponseBridgeResult, UpstreamResponseUsage,
};
#[cfg(test)]
use output_text::{output_text_limit_bytes, OUTPUT_TEXT_TRUNCATED_MARKER};

pub(super) fn reload_from_env() {
    reload_output_text_from_env();
}

fn push_trace_id_header(headers: &mut Vec<Header>, trace_id: &str) {
    let Some(trace_id) = Some(trace_id)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    if let Ok(header) = Header::from_bytes(
        crate::error_codes::TRACE_ID_HEADER_NAME.as_bytes(),
        trace_id.as_bytes(),
    ) {
        headers.push(header);
    }
}

#[path = "http_bridge/sse_frame.rs"]
mod sse_frame;
#[path = "http_bridge/openai_stream.rs"]
mod openai_stream;
#[path = "http_bridge/sse_aggregate.rs"]
mod sse_aggregate;
#[path = "http_bridge/delivery.rs"]
mod delivery;
#[path = "http_bridge/stream_readers.rs"]
mod stream_readers;

use sse_frame::{
    extract_sse_frame_payload, inspect_sse_frame, is_response_completed_event_name,
    parse_sse_frame_json, SseTerminal,
};
#[cfg(test)]
use sse_frame::parse_usage_from_sse_frame;
use openai_stream::{
    apply_openai_stream_meta_defaults, build_chat_fallback_content_chunk,
    build_completion_fallback_text_chunk, extract_openai_completed_output_text,
    map_chunk_has_chat_text, map_chunk_has_completion_text, normalize_chat_chunk_delta_role,
    should_skip_chat_live_text_event, should_skip_completion_live_text_event,
    synthesize_chat_completion_sse_from_json, synthesize_completions_sse_from_json,
    update_openai_stream_meta, OpenAIStreamMeta,
};
use sse_aggregate::{collect_non_stream_json_from_sse_bytes, looks_like_sse_payload};
pub(super) fn respond_with_upstream(
    request: Request,
    upstream: reqwest::blocking::Response,
    inflight_guard: super::AccountInFlightGuard,
    response_adapter: super::ResponseAdapter,
    tool_name_restore_map: Option<&super::ToolNameRestoreMap>,
    is_stream: bool,
    trace_id: Option<&str>,
) -> Result<UpstreamResponseBridgeResult, String> {
    delivery::respond_with_upstream(
        request,
        upstream,
        inflight_guard,
        response_adapter,
        tool_name_restore_map,
        is_stream,
        trace_id,
    )
}
pub(super) use stream_readers::{
    AnthropicSseReader, OpenAIChatCompletionsSseReader, OpenAICompletionsSseReader,
    PassthroughSseCollector, PassthroughSseUsageReader,
};

#[cfg(test)]
#[path = "tests/http_bridge_tests.rs"]
mod tests;





