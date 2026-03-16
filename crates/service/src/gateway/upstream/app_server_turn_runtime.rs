use bytes::Bytes;
use std::time::Instant;

use super::attempt_flow::candidate_flow::CandidateUpstreamDecision;
use super::attempt_flow::transport::UpstreamRequestContext;
use super::proxy_pipeline::candidate_attempt::{
    run_candidate_attempt, CandidateAttemptParams, CandidateAttemptTrace,
};
use super::proxy_pipeline::candidate_state::CandidateExecutionState;
use super::proxy_pipeline::execution_context::GatewayUpstreamExecutionContext;
use super::proxy_pipeline::request_gate::acquire_request_gate;
use super::proxy_pipeline::request_setup::prepare_request_setup;
use super::support::candidates::free_account_model_override;

const APP_SERVER_INTERNAL_KEY_ID: &str = "codex_app_server_internal";
const REQUEST_METHOD: &str = "POST";
const REQUEST_PATH: &str = "/v1/responses";

#[derive(Debug, Clone, Default)]
pub(crate) struct AppServerTurnUsage {
    pub(crate) input_tokens: Option<i64>,
    pub(crate) cached_input_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
    pub(crate) total_tokens: Option<i64>,
    pub(crate) reasoning_output_tokens: Option<i64>,
}

pub(crate) struct AppServerTurnExecution {
    response: Option<reqwest::blocking::Response>,
    inflight_guard: Option<super::super::AccountInFlightGuard>,
    request_gate_guard: Option<super::super::request_gate::RequestGateGuard>,
    trace_id: String,
    final_account_id: String,
    upstream_url: Option<String>,
    attempted_account_ids: Vec<String>,
    model_for_log: Option<String>,
    reasoning_for_log: Option<String>,
    started_at: Instant,
}

impl AppServerTurnExecution {
    pub(crate) fn take_response(&mut self) -> Result<reqwest::blocking::Response, String> {
        self.response
            .take()
            .ok_or_else(|| "upstream response already consumed".to_string())
    }

    pub(crate) fn finish(
        mut self,
        status_code: u16,
        usage: AppServerTurnUsage,
        error: Option<&str>,
        model_for_log: Option<&str>,
    ) {
        let _ = self.response.take();
        let _ = self.inflight_guard.take();
        let _ = self.request_gate_guard.take();

        let Some(storage) = crate::storage_helpers::open_storage() else {
            log::warn!(
                "event=app_server_turn_log_skipped reason=open_storage_failed trace_id={} status={}",
                self.trace_id,
                status_code
            );
            return;
        };

        super::super::request_log::write_request_log_with_attempts(
            &storage,
            super::super::request_log::RequestLogTraceContext {
                trace_id: Some(self.trace_id.as_str()),
                original_path: Some(REQUEST_PATH),
                adapted_path: Some(REQUEST_PATH),
                response_adapter: Some(super::super::ResponseAdapter::Passthrough),
            },
            Some(APP_SERVER_INTERNAL_KEY_ID),
            Some(self.final_account_id.as_str()),
            REQUEST_PATH,
            REQUEST_METHOD,
            model_for_log.or(self.model_for_log.as_deref()),
            self.reasoning_for_log.as_deref(),
            self.upstream_url.as_deref(),
            Some(status_code),
            super::super::request_log::RequestLogUsage {
                input_tokens: usage.input_tokens,
                cached_input_tokens: usage.cached_input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
                reasoning_output_tokens: usage.reasoning_output_tokens,
            },
            error,
            Some(self.started_at.elapsed().as_millis()),
            Some(self.attempted_account_ids.as_slice()),
        );
        super::super::trace_log::log_request_final(
            self.trace_id.as_str(),
            status_code,
            Some(self.final_account_id.as_str()),
            self.upstream_url.as_deref(),
            error,
            self.started_at.elapsed().as_millis(),
        );
        super::super::record_gateway_request_outcome(
            REQUEST_PATH,
            status_code,
            Some(crate::apikey_profile::PROTOCOL_OPENAI_COMPAT),
        );
    }
}

fn internal_request_context() -> UpstreamRequestContext<'static> {
    UpstreamRequestContext {
        request_path: REQUEST_PATH,
        remote_addr: None,
    }
}

fn log_internal_terminal_result(
    context: &GatewayUpstreamExecutionContext<'_>,
    final_account_id: Option<&str>,
    upstream_url: Option<&str>,
    model_for_log: Option<&str>,
    status_code: u16,
    error: &str,
    started_at: Instant,
    attempted_account_ids: &[String],
) {
    context.log_final_result_with_model(
        final_account_id,
        upstream_url,
        model_for_log,
        status_code,
        super::super::request_log::RequestLogUsage::default(),
        Some(error),
        started_at.elapsed().as_millis(),
        Some(attempted_account_ids),
    );
}

pub(crate) fn execute_app_server_turn_response_request(
    body: &Bytes,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
) -> Result<AppServerTurnExecution, String> {
    let storage = crate::storage_helpers::open_storage()
        .ok_or_else(|| "storage not initialized".to_string())?;
    let trace_id = super::super::trace_log::next_trace_id();
    let started_at = Instant::now();
    super::super::trace_log::log_request_start(
        trace_id.as_str(),
        APP_SERVER_INTERNAL_KEY_ID,
        REQUEST_METHOD,
        REQUEST_PATH,
        model_for_log,
        reasoning_for_log,
        true,
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
    );
    super::super::trace_log::log_request_body_preview(trace_id.as_str(), body.as_ref());

    let mut candidates =
        super::support::candidates::prepare_gateway_candidates(&storage, model_for_log)?;
    if candidates.is_empty() {
        let context = GatewayUpstreamExecutionContext::new(
            trace_id.as_str(),
            &storage,
            APP_SERVER_INTERNAL_KEY_ID,
            REQUEST_PATH,
            REQUEST_PATH,
            REQUEST_METHOD,
            super::super::ResponseAdapter::Passthrough,
            crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
            model_for_log,
            reasoning_for_log,
            0,
            super::super::account_max_inflight_limit(),
        );
        log_internal_terminal_result(
            &context,
            None,
            None,
            model_for_log,
            503,
            "no available account",
            started_at,
            &[],
        );
        return Err("no available account".to_string());
    }

    let incoming_headers = super::super::IncomingHeaderSnapshot::default();
    let setup = prepare_request_setup(
        REQUEST_PATH,
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        false,
        &incoming_headers,
        body,
        &mut candidates,
        APP_SERVER_INTERNAL_KEY_ID,
        model_for_log,
        trace_id.as_str(),
    );
    let context = GatewayUpstreamExecutionContext::new(
        trace_id.as_str(),
        &storage,
        APP_SERVER_INTERNAL_KEY_ID,
        REQUEST_PATH,
        REQUEST_PATH,
        REQUEST_METHOD,
        super::super::ResponseAdapter::Passthrough,
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        model_for_log,
        reasoning_for_log,
        setup.candidate_count,
        setup.account_max_inflight,
    );
    let request_deadline = super::support::deadline::request_deadline(started_at, true);
    let request_gate_guard = acquire_request_gate(
        trace_id.as_str(),
        APP_SERVER_INTERNAL_KEY_ID,
        REQUEST_PATH,
        model_for_log,
        request_deadline,
    );
    let request_ctx = internal_request_context();
    let method = reqwest::Method::POST;
    let mut state = CandidateExecutionState::default();
    let mut attempted_account_ids = Vec::new();

    for (idx, (account, mut token)) in candidates.into_iter().enumerate() {
        if super::support::deadline::is_expired(request_deadline) {
            let message = "upstream total timeout exceeded".to_string();
            log_internal_terminal_result(
                &context,
                None,
                None,
                model_for_log,
                504,
                message.as_str(),
                started_at,
                attempted_account_ids.as_slice(),
            );
            return Err(message);
        }

        let strip_session_affinity =
            state.strip_session_affinity(&account, idx, setup.anthropic_has_prompt_cache_key);
        let attempt_model_override = free_account_model_override(&storage, &account, &token);
        let attempt_model_for_log = attempt_model_override.as_deref().or(model_for_log);
        let body_for_attempt = state.body_for_attempt(
            REQUEST_PATH,
            body,
            strip_session_affinity,
            &setup,
            attempt_model_override.as_deref(),
        );
        context.log_candidate_start(&account.id, idx, strip_session_affinity);
        if let Some(skip_reason) = context.should_skip_candidate(&account.id, idx) {
            context.log_candidate_skip(&account.id, idx, skip_reason);
            continue;
        }
        attempted_account_ids.push(account.id.clone());
        super::super::trace_log::log_attempt_profile(
            trace_id.as_str(),
            &account.id,
            idx,
            setup.candidate_count,
            strip_session_affinity,
            false,
            false,
            false,
            None,
            None,
            body_for_attempt.len(),
            attempt_model_for_log,
        );

        let mut inflight_guard = Some(super::super::acquire_account_inflight(&account.id));
        let mut attempt_trace = CandidateAttemptTrace::default();
        let decision = run_candidate_attempt(CandidateAttemptParams {
            storage: &storage,
            method: &method,
            request_ctx,
            incoming_headers: &incoming_headers,
            body: &body_for_attempt,
            upstream_is_stream: true,
            path: REQUEST_PATH,
            request_deadline,
            account: &account,
            token: &mut token,
            strip_session_affinity,
            debug: false,
            allow_openai_fallback: false,
            disable_challenge_stateless_retry: false,
            has_more_candidates: context.has_more_candidates(idx),
            context: &context,
            setup: &setup,
            trace: &mut attempt_trace,
        });

        match decision {
            CandidateUpstreamDecision::Failover => {
                super::super::record_gateway_failover_attempt();
                continue;
            }
            CandidateUpstreamDecision::Terminal {
                status_code,
                message,
            } => {
                log_internal_terminal_result(
                    &context,
                    Some(account.id.as_str()),
                    attempt_trace.last_attempt_url.as_deref(),
                    attempt_model_for_log,
                    status_code,
                    message.as_str(),
                    started_at,
                    attempted_account_ids.as_slice(),
                );
                return Err(message);
            }
            CandidateUpstreamDecision::RespondUpstream(mut resp) => {
                if resp.status().as_u16() == 400 && !strip_session_affinity {
                    let retry_body = state.retry_body(
                        REQUEST_PATH,
                        body,
                        &setup,
                        attempt_model_override.as_deref(),
                    );
                    let retry_decision = run_candidate_attempt(CandidateAttemptParams {
                        storage: &storage,
                        method: &method,
                        request_ctx,
                        incoming_headers: &incoming_headers,
                        body: &retry_body,
                        upstream_is_stream: true,
                        path: REQUEST_PATH,
                        request_deadline,
                        account: &account,
                        token: &mut token,
                        strip_session_affinity: true,
                        debug: false,
                        allow_openai_fallback: false,
                        disable_challenge_stateless_retry: false,
                        has_more_candidates: context.has_more_candidates(idx),
                        context: &context,
                        setup: &setup,
                        trace: &mut attempt_trace,
                    });

                    match retry_decision {
                        CandidateUpstreamDecision::RespondUpstream(retry_resp) => {
                            resp = retry_resp;
                        }
                        CandidateUpstreamDecision::Failover => {
                            super::super::record_gateway_failover_attempt();
                            continue;
                        }
                        CandidateUpstreamDecision::Terminal {
                            status_code,
                            message,
                        } => {
                            log_internal_terminal_result(
                                &context,
                                Some(account.id.as_str()),
                                attempt_trace.last_attempt_url.as_deref(),
                                attempt_model_for_log,
                                status_code,
                                message.as_str(),
                                started_at,
                                attempted_account_ids.as_slice(),
                            );
                            return Err(message);
                        }
                    }
                }

                return Ok(AppServerTurnExecution {
                    response: Some(resp),
                    inflight_guard: inflight_guard.take(),
                    request_gate_guard,
                    trace_id,
                    final_account_id: account.id,
                    upstream_url: attempt_trace.last_attempt_url,
                    attempted_account_ids,
                    model_for_log: attempt_model_for_log.map(str::to_string),
                    reasoning_for_log: reasoning_for_log.map(str::to_string),
                    started_at,
                });
            }
        }
    }

    let message = "no available account".to_string();
    log_internal_terminal_result(
        &context,
        None,
        Some(setup.upstream_base.as_str()),
        model_for_log,
        503,
        message.as_str(),
        started_at,
        attempted_account_ids.as_slice(),
    );
    Err(message)
}
