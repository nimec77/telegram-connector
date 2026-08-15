//! Channel-reference -> peer resolution (the consolidated AD-1 resolver).
//!
//! Unit of `client` (LM-2).

use super::*;

impl TelegramClient {
    /// Canonical channel-reference resolver: a numeric ref is found by walking the
    /// dialog list; anything else is treated as a username (leading `@` stripped).
    /// Both forms hard-error with `InvalidInput` when nothing matches. This is the
    /// single entry point — `get_channel_info`, `get_message_by_id`,
    /// `download_message_media`, and `transcribe_audio` all go through it (AD-1).
    pub(super) async fn resolve_peer(
        &self,
        channel_ref: &str,
    ) -> Result<grammers_client::peer::Peer, Error> {
        if let Ok(id) = channel_ref.parse::<i64>() {
            // Numeric ID — search through dialogs.
            self.find_dialog_peer(id).await?.ok_or_else(|| {
                tracing::warn!(id, "Channel not found in dialogs by ID");
                Error::InvalidInput(format!("Channel not found: {}", channel_ref))
            })
        } else {
            // Username — resolve directly.
            self.resolve_username_peer(channel_ref)
                .await?
                .ok_or_else(|| {
                    tracing::warn!(channel_ref = %channel_ref, "Username not found");
                    Error::InvalidInput(format!("Channel not found: {}", channel_ref))
                })
        }
    }

    /// Resolve a username to a peer via `resolve_username`, bounded by the resolve
    /// timeout (a leading `@` is stripped). `Ok(None)` means the username does not
    /// exist; `Err` is an RPC/timeout failure. `Ok(None)` is also returned before
    /// any RPC is made when the username locally fails [`Username::is_valid_shape`]
    /// — a malformed username cannot exist, so the local shape-reject path and the
    /// "no such username" RPC outcome are indistinguishable to callers by design.
    /// Shared by [`Self::resolve_peer`] (which hard-errors on `None`) and
    /// `get_recent_messages` (which falls back to a dialog walk on `None`/`Err`).
    pub(super) async fn resolve_username_peer(
        &self,
        channel_ref: &str,
    ) -> Result<Option<grammers_client::peer::Peer>, Error> {
        let username = channel_ref.strip_prefix('@').unwrap_or(channel_ref);
        // Reject malformed usernames locally — Ok(None) means "cannot exist",
        // which callers already turn into the clean not-found error or their
        // dialog-walk fallback, without spending a resolve RPC (D8).
        if !Username::is_valid_shape(username) {
            tracing::warn!(username = %username, "Rejected malformed username without RPC");
            return Ok(None);
        }
        with_timeout("resolve_username", self.timeouts.resolve_secs, async {
            self.client.resolve_username(username).await.map_err(|e| {
                tracing::error!(username = %username, error = %e, "Failed to resolve username");
                Error::TelegramApi(format!("Failed to resolve username: {}", e))
            })
        })
        .await
    }

    /// Find a subscribed peer by its bare numeric ID by walking the dialog list,
    /// bounded by the resolve timeout. `Ok(None)` means no dialog matched. Shared
    /// by [`Self::resolve_peer`] and `get_recent_messages`'s dialog fallback.
    pub(super) async fn find_dialog_peer(
        &self,
        id: i64,
    ) -> Result<Option<grammers_client::peer::Peer>, Error> {
        with_timeout("iter_dialogs", self.timeouts.resolve_secs, async {
            let mut dialogs = self.client.iter_dialogs();
            while let Some(dialog) = dialogs.next().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to iterate dialogs");
                Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
            })? {
                if dialog.peer().id().bare_id() == Some(id) {
                    return Ok(Some(dialog.peer().clone()));
                }
            }
            Ok(None)
        })
        .await
    }

    /// Trait backing for `resolve_channel_identity` (work-order B2): one
    /// resolve, then sentinel-free identity extraction.
    pub(super) async fn resolve_channel_identity_impl(
        &self,
        channel_ref: &str,
    ) -> Result<ChannelIdentity, Error> {
        validate_channel_identifier(channel_ref)?;
        let peer = self.resolve_peer(channel_ref).await?;
        channel_identity(&peer).ok_or_else(|| {
            Error::TelegramApi(format!(
                "Failed to read channel identity for {}",
                channel_ref
            ))
        })
    }
}

/// The username to attempt for a history `channel_identifier`, or `None` when the
/// identifier (after stripping a leading `@`) is purely numeric. Preserves the
/// original `get_recent_messages` rule of resolving only provably-non-numeric
/// usernames before falling back to a dialog walk by numeric id (AD-1).
pub(crate) fn username_to_resolve(identifier: &str) -> Option<&str> {
    let username = identifier.strip_prefix('@').unwrap_or(identifier);
    if username.chars().all(|c| c.is_ascii_digit()) {
        None
    } else {
        Some(identifier)
    }
}
