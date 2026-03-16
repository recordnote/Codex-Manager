use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

use super::{response, value_or_error, RpcRequestContext};

pub(super) fn try_handle(req: &JsonRpcRequest, ctx: &RpcRequestContext) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "thread/start" => {
            value_or_error(crate::thread_turn::thread_start(req.params.as_ref(), ctx))
        }
        "thread/resume" => {
            value_or_error(crate::thread_turn::thread_resume(req.params.as_ref(), ctx))
        }
        "thread/fork" => value_or_error(crate::thread_turn::thread_fork(req.params.as_ref(), ctx)),
        "thread/read" => value_or_error(crate::thread_turn::thread_read(req.params.as_ref())),
        "thread/name/set" => value_or_error(crate::thread_turn::thread_name_set(
            req.params.as_ref(),
            ctx,
        )),
        "thread/compact/start" => value_or_error(crate::thread_turn::thread_compact_start(
            req.params.as_ref(),
            ctx,
        )),
        "turn/start" => value_or_error(crate::thread_turn::turn_start(req.params.as_ref(), ctx)),
        "turn/steer" => value_or_error(crate::thread_turn::turn_steer(req.params.as_ref())),
        "turn/interrupt" => value_or_error(crate::thread_turn::turn_interrupt(req.params.as_ref())),
        _ => return None,
    };

    Some(response(req, result))
}
