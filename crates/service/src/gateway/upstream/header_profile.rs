#[path = "headers/mod.rs"]
mod headers_impl;

pub(crate) use headers_impl::{
    build_codex_compact_upstream_headers, build_codex_upstream_headers,
    derive_sticky_conversation_id_from_headers,
    derive_sticky_conversation_id_from_headers_with_remote, derive_sticky_session_id_from_headers,
    derive_sticky_session_id_from_headers_with_remote, CodexCompactUpstreamHeaderInput,
    CodexUpstreamHeaderInput, CODEX_CLIENT_VERSION,
};
