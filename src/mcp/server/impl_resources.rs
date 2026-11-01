//! Cache-only resources share bounded blocking admission with preview processing.
use super::McpServer;
use crate::mcp::artifacts::StoreError;
use crate::rate_limiter::RateLimiterTrait;
use crate::telegram::TelegramClientTrait;
use rmcp::model::{
    CacheScope, ErrorCode, ErrorData, ListResourceTemplatesResult, ListResourcesResult,
    ProtocolVersion, ReadResourceResult, ResourceContents,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

pub(super) const MAX_RESOURCE_BYTES: usize = 6 * 1024 * 1024;
pub(super) const RESOURCE_READ_SECONDS: u64 = 5;

fn modern(version: Option<ProtocolVersion>) -> bool {
    version.is_some_and(|version| version >= ProtocolVersion::V_2026_07_28)
}
fn resource_error(error: StoreError) -> ErrorData {
    match error {
        StoreError::InvalidUri => ErrorData::invalid_params(
            "Invalid media artifact URI",
            Some(serde_json::json!({"code":"artifact_invalid_uri"})),
        ),
        StoreError::Missing => ErrorData::new(
            ErrorCode::RESOURCE_NOT_FOUND,
            "Media artifact is missing or expired",
            Some(serde_json::json!({"code":"artifact_missing"})),
        ),
        StoreError::TooLarge => ErrorData::new(
            ErrorCode(-32003),
            "Media artifact exceeds the resource read limit",
            Some(serde_json::json!({"code":"artifact_too_large"})),
        ),
        StoreError::Unavailable => ErrorData::internal_error(
            "Media artifact storage is unavailable",
            Some(serde_json::json!({"code":"artifact_unavailable"})),
        ),
    }
}
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    pub(super) fn resources_list_for(
        &self,
        version: Option<ProtocolVersion>,
    ) -> Result<ListResourcesResult, ErrorData> {
        if self.media_artifacts.is_none() {
            return Err(ErrorData::method_not_found::<
                rmcp::model::ListResourcesRequestMethod,
            >());
        }
        let mut result = ListResourcesResult::default();
        if modern(version) {
            // Empty discovery must never extend the freshness of an existing handle.
            result.ttl_ms = Some(0);
            result.cache_scope = Some(CacheScope::Private);
        } else {
            result.result_type = None;
        }
        Ok(result)
    }
    pub(super) fn resource_templates_for(
        &self,
        version: Option<ProtocolVersion>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        if self.media_artifacts.is_none() {
            return Err(ErrorData::method_not_found::<
                rmcp::model::ListResourceTemplatesRequestMethod,
            >());
        }
        let mut result = ListResourceTemplatesResult::default();
        if modern(version) {
            // Empty discovery must never extend the freshness of an existing handle.
            result.ttl_ms = Some(0);
            result.cache_scope = Some(CacheScope::Private);
        } else {
            result.result_type = None;
        }
        Ok(result)
    }
    pub(super) async fn read_artifact_resource(
        &self,
        uri: String,
        version: Option<ProtocolVersion>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let deadline = Instant::now() + Duration::from_secs(RESOURCE_READ_SECONDS);
        let store =
            Arc::clone(self.media_artifacts.as_ref().ok_or_else(
                ErrorData::method_not_found::<rmcp::model::ReadResourceRequestMethod>,
            )?);
        let lookup = uri.clone();
        let job = crate::mcp::runtime::image_job(move || {
            use base64::Engine as _;
            let read = store.read(&lookup, MAX_RESOURCE_BYTES)?;
            let blob = base64::engine::general_purpose::STANDARD.encode(read.bytes);
            Ok::<_, StoreError>((blob, read.expires_at))
        });
        let timeout = || {
            ErrorData::new(
                ErrorCode(-32001),
                "Media artifact read timed out",
                Some(serde_json::json!({"code":"artifact_timeout"})),
            )
        };
        let (blob, expires_at) = tokio::time::timeout_at(deadline, job)
            .await
            .map_err(|_| timeout())?
            .map_err(|_| resource_error(StoreError::Unavailable))?
            .map_err(resource_error)?;
        complete_read(uri, blob, expires_at, deadline, version)
    }
}

fn complete_read(
    uri: String,
    blob: String,
    expires_at: Instant,
    deadline: Instant,
    version: Option<ProtocolVersion>,
) -> Result<ReadResourceResult, ErrorData> {
    let now = Instant::now();
    if now >= deadline {
        return Err(ErrorData::new(
            ErrorCode(-32001),
            "Media artifact read timed out",
            Some(serde_json::json!({"code":"artifact_timeout"})),
        ));
    }
    if now >= expires_at {
        return Err(resource_error(StoreError::Missing));
    }
    let mut result = ReadResourceResult::new(vec![
        ResourceContents::blob(blob, uri).with_mime_type("image/jpeg"),
    ]);
    if modern(version) {
        result.ttl_ms = Some(
            expires_at
                .duration_since(now)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        );
        result.cache_scope = Some(CacheScope::Private);
    } else {
        result.result_type = None;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn completed_read_cannot_override_exact_deadline_or_artifact_expiry() {
        let now = Instant::now();
        for (expiry, deadline, code) in [
            (now + Duration::from_secs(1), now, "artifact_timeout"),
            (now, now + Duration::from_secs(1), "artifact_missing"),
        ] {
            let error = complete_read(
                "sensitive handle".into(),
                "aGVsbG8=".into(),
                expiry,
                deadline,
                Some(ProtocolVersion::V_2026_07_28),
            )
            .expect_err("exact boundary refused");
            assert_eq!(error.data.expect("safe discriminator")["code"], code);
        }
        let result = complete_read(
            "sensitive handle".into(),
            "aGVsbG8=".into(),
            now + Duration::from_millis(19),
            now + Duration::from_secs(5),
            Some(ProtocolVersion::V_2026_07_28),
        )
        .expect("before both boundaries");
        assert_eq!(result.ttl_ms, Some(19));
        assert_eq!(result.cache_scope, Some(CacheScope::Private));
    }
}
