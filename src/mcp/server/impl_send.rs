//! Write admission stays above the client seam; failures preserve delivery truth.
use super::McpServer;
use crate::config::OutputFormat;
use crate::mcp::tools::send_budget;
use crate::mcp::tools::types::sending::{SendMediaRequest, SendMessageRequest, SendReceipt};
use crate::rate_limiter::RateLimiterTrait;
use crate::sending::SendingRuntimeConfig;
use crate::telegram::TelegramClientTrait;
use crate::telegram::sending::{SendDeadline, SendStage};
use crate::telegram::types::sending::{
    PreparedMediaSend, RecipientSpec, SendFailure, SendFailureKind, SendOptions, SentMessage,
    UsernameNamespace,
};
use rmcp::model::{CallToolResult, JsonObject};
use std::sync::Arc;

pub(super) fn send_name(name: &str) -> bool {
    matches!(name, "send_message" | "send_media")
}
impl<T: TelegramClientTrait + 'static, R: RateLimiterTrait + 'static> McpServer<T, R> {
    /// Apply outgoing policy; disabled/text-only settings create no storage.
    ///
    /// # Errors
    /// Rejects legacy-enabled writes. Configured media storage is initialized separately.
    pub async fn with_sending(mut self, config: SendingRuntimeConfig) -> Result<Self, SendFailure> {
        if config.settings.enabled
            && (config.output_format != OutputFormat::JsonV1
                || self.output_format != OutputFormat::JsonV1)
        {
            return Err(SendFailure::not_sent(SendFailureKind::InvalidInput, None));
        }
        self.outgoing_store = if config.settings.enabled && !config.settings.upload_roots.is_empty()
        {
            Some(crate::sending::staging::StagingStore::try_new(&config).await?)
        } else {
            None
        };
        self.sending_config = Some(Arc::new(config));
        self.refresh_tool_router();
        Ok(self)
    }
    pub(super) fn sending_enabled(&self) -> bool {
        self.output_format == OutputFormat::JsonV1
            && self
                .sending_config
                .as_ref()
                .is_some_and(|c| c.settings.enabled)
    }
    pub(super) fn cached_sending_status(
        &self,
    ) -> crate::mcp::tools::types::responses::SendingStatus {
        let defaults = crate::config::SendingConfig::default();
        let policy = self.sending_config.as_deref();
        let settings = policy.map_or(&defaults, |c| &c.settings);
        crate::mcp::tools::types::responses::SendingStatus {
            enabled: self.sending_enabled(),
            media_enabled: self.sending_enabled() && self.outgoing_store.is_some(),
            max_image_bytes: settings.max_image_bytes,
            max_media_bytes: settings.max_media_bytes,
            max_concurrent_media: settings.max_concurrent_media,
            max_staging_bytes: settings.max_staging_bytes,
            send_secs: policy
                .map_or_else(crate::config::defaults::default_send_secs, |c| c.send_secs),
            upload_send_secs: policy
                .map_or_else(crate::config::defaults::default_upload_send_secs, |c| {
                    c.upload_send_secs
                }),
            send_message_cost: policy
                .map_or_else(crate::config::defaults::default_send_message_cost, |c| {
                    c.send_message_cost
                }),
            send_media_cost: policy
                .map_or_else(crate::config::defaults::default_send_media_cost, |c| {
                    c.send_media_cost
                }),
        }
    }
    pub(super) fn validate_sending_mode(&self) -> Result<(), SendFailure> {
        if self
            .sending_config
            .as_ref()
            .is_some_and(|c| c.settings.enabled)
            && self.output_format != OutputFormat::JsonV1
        {
            return Err(SendFailure::not_sent(SendFailureKind::InvalidInput, None));
        }
        Ok(())
    }
    pub(super) fn refresh_tool_router(&mut self) {
        self.tool_router = Self::tool_router();
        if !self.sending_enabled() {
            self.tool_router.map.retain(|name, _| !send_name(name));
        }
        if self.output_format == OutputFormat::Legacy {
            for route in self.tool_router.map.values_mut() {
                route.attr.output_schema = None;
            }
        }
    }
    fn sending_policy(&self) -> Result<&SendingRuntimeConfig, SendFailure> {
        self.validate_sending_mode()?;
        self.sending_config
            .as_deref()
            .filter(|c| c.settings.enabled)
            .ok_or_else(|| SendFailure::not_sent(SendFailureKind::SendingDisabled, None))
    }
    pub(super) async fn send_message_data(
        &self,
        request: SendMessageRequest,
    ) -> Result<SendReceipt, SendFailure> {
        let policy = self.sending_policy()?;
        let deadline = SendDeadline::new(policy.send_secs)?;
        let request = request.try_into_domain()?;
        send_budget::preflight(self.response_byte_budget)?;
        deadline
            .run(SendStage::LocalPreparation, async {
                self.rate_limiter
                    .acquire(policy.send_message_cost)
                    .await
                    .map_err(admission_failure)
            })
            .await?;
        let receipt = self
            .telegram_client
            .send_message(&request, deadline)
            .await?;
        if deadline.expired() {
            return Err(SendFailure::unknown());
        }
        if receipt.content_kind != crate::telegram::types::sending::ContentKind::Text
            || !receipt_options_match(&request.options, &receipt)
        {
            return Err(SendFailure::unknown());
        }
        SendReceipt::try_from(receipt)
    }
    pub(super) async fn send_message_wire_impl(&self, arguments: JsonObject) -> CallToolResult {
        let started = std::time::Instant::now();
        let result = async {
            self.sending_policy()?;
            let request = serde_json::from_value(serde_json::Value::Object(arguments))
                .map_err(|_| SendFailure::not_sent(SendFailureKind::InvalidInput, None))?;
            self.send_message_data(request).await
        }
        .await;
        log_outcome("send_message", started, &result);
        send_budget::finish(result, self.response_byte_budget)
    }
    pub(super) async fn send_media_wire_impl(&self, arguments: JsonObject) -> CallToolResult {
        let started = std::time::Instant::now();
        let result = async {
            self.sending_policy()?;
            let request: SendMediaRequest =
                serde_json::from_value(serde_json::Value::Object(arguments))
                    .map_err(|_| SendFailure::not_sent(SendFailureKind::InvalidInput, None))?;
            self.send_media_data(request).await
        }
        .await;
        log_outcome("send_media", started, &result);
        send_budget::finish(result, self.response_byte_budget)
    }
    pub(super) async fn send_media_data(
        &self,
        request: SendMediaRequest,
    ) -> Result<SendReceipt, SendFailure> {
        let policy = self.sending_policy()?;
        let deadline = SendDeadline::new(policy.upload_send_secs)?;
        let request = request.try_into_domain()?;
        send_budget::preflight(self.response_byte_budget)?;
        let store = self
            .outgoing_store
            .as_ref()
            .ok_or_else(|| SendFailure::not_sent(SendFailureKind::UploadSourceInvalid, None))?;
        let staged = store
            .stage(&request.source, &request.media, deadline)
            .await?;
        tracing::debug!(
            tool = "send_media",
            stage = "local_preparation",
            size_bytes = staged.size_bytes(),
            "Outgoing snapshot prepared"
        );
        deadline
            .run(SendStage::LocalPreparation, async {
                self.rate_limiter
                    .acquire(policy.send_media_cost)
                    .await
                    .map_err(admission_failure)
            })
            .await?;
        let request = PreparedMediaSend {
            options: request.options,
            caption: request.caption,
            media: request.media,
            staged,
        };
        let receipt = self.telegram_client.send_media(&request, deadline).await?;
        if deadline.expired()
            || receipt.content_kind != request.media.content_kind()
            || !receipt_options_match(&request.options, &receipt)
            || !receipt.media.as_ref().is_some_and(|m| {
                m.mime_type == request.staged.mime_type()
                    && m.size_bytes == request.staged.size_bytes()
            })
        {
            return Err(SendFailure::unknown());
        }
        SendReceipt::try_from(receipt)
    }
}
fn receipt_options_match(options: &SendOptions, receipt: &SentMessage) -> bool {
    use crate::telegram::types::PeerNamespace;
    if receipt.reply_to_message_id != options.reply_to_message_id
        || receipt.topic_id != options.topic_id
    {
        return false;
    }
    match &options.recipient {
        RecipientSpec::SavedMessages => {
            receipt.saved_messages && receipt.recipient.namespace() == PeerNamespace::User
        }
        RecipientSpec::Peer(id) => !receipt.saved_messages && receipt.recipient == *id,
        RecipientSpec::Username {
            expected_namespace, ..
        } => {
            !receipt.saved_messages
                && receipt.recipient.namespace()
                    == match expected_namespace {
                        UsernameNamespace::User => PeerNamespace::User,
                        UsernameNamespace::Channel => PeerNamespace::Channel,
                    }
        }
    }
}
pub(super) fn admission_failure(error: crate::error::Error) -> SendFailure {
    match error {
        crate::error::Error::RateLimit {
            retry_after_seconds,
            ..
        } => SendFailure::not_sent(SendFailureKind::RateLimited, Some(retry_after_seconds)),
        _ => SendFailure::not_sent(SendFailureKind::Unavailable, None),
    }
}
pub(super) fn log_outcome<T>(
    tool: &'static str,
    started: std::time::Instant,
    result: &Result<T, SendFailure>,
) {
    let duration_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(_) => tracing::info!(tool, duration_ms, "Send operation completed"),
        Err(error) => tracing::warn!(
            tool,
            duration_ms,
            category = error.kind().as_str(),
            "Send operation failed"
        ),
    }
}
