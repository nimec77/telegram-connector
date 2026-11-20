//! Explicit destination policy for whole-placeholder scalar values.

use toml::de::DeValue;

#[derive(Clone, Copy)]
pub(super) enum Scalar {
    Integer,
    Float,
    Boolean,
}

impl Scalar {
    pub(super) fn accepts(self, value: &DeValue<'_>) -> bool {
        match self {
            Self::Integer => matches!(value, DeValue::Integer(_)),
            Self::Float => matches!(value, DeValue::Integer(_) | DeValue::Float(_)),
            Self::Boolean => matches!(value, DeValue::Boolean(_)),
        }
    }
}

/// Width/sign checks remain with Config's existing typed deserializer. Paths are
/// segments, not joined strings: quoted keys containing dots cannot impersonate
/// a typed destination. Array elements deliberately have no scalar destination.
pub(super) fn destination(path: &[&str]) -> Option<Scalar> {
    match path {
        ["telegram", "api_id" | "max_download_bytes"]
        | [
            "telegram",
            "timeouts",
            "send_secs" | "upload_send_secs" | "resolve_secs" | "history_secs" | "search_secs"
            | "download_secs",
        ]
        | [
            "rate_limiting",
            "send_message_cost"
            | "send_media_cost"
            | "max_tokens"
            | "media_download_cost"
            | "transcription_cost",
        ]
        | [
            "transcription",
            "default_timeout_seconds" | "max_timeout_seconds",
        ]
        | ["logging", "max_log_days"]
        | ["search", "deadline_seconds"]
        | [
            "server",
            "shutdown_timeout_seconds" | "max_request_bytes" | "max_in_flight_requests",
        ]
        | [
            "sending",
            "max_image_bytes" | "max_media_bytes" | "max_staging_bytes" | "max_concurrent_media",
        ]
        | [
            "limits",
            "response_byte_budget" | "media_batch_max_total_bytes",
        ]
        | [
            "observability",
            "slow_write_threshold_ms" | "response_buffer_size" | "max_buffered_payload_bytes",
        ]
        | [
            "media_artifacts",
            "ttl_seconds" | "max_total_bytes" | "max_entries",
        ] => Some(Scalar::Integer),
        ["rate_limiting", "refill_rate"] => Some(Scalar::Float),
        ["sending", "enabled"]
        | ["logging", "file_enabled"]
        | ["media_artifacts", "enabled" | "file_export_enabled"] => Some(Scalar::Boolean),
        _ => None,
    }
}
