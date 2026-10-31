//! Channel-reference -> peer resolution (the consolidated AD-1 resolver).
//!
//! Unit of `client` (LM-2).

use super::TelegramClient;
use crate::error::Error;
use crate::telegram::converters::{channel_identity, peer_namespace};
use crate::telegram::timeout::with_timeout;
use crate::telegram::types::{ChannelIdentity, PeerNamespace, Username};

/// Convert a resolved peer into the `PeerRef` that grammers RPC calls take.
///
/// grammers 0.10 split two failure modes that used to be one `Option`; keep
/// them distinguishable in logs: `Err` is a session/storage error, `Ok(None)`
/// means the peer is not in the session cache (no usable access hash).
pub(super) async fn peer_to_ref(
    peer: &grammers_client::peer::Peer,
) -> Result<grammers_session::types::PeerRef, Error> {
    peer.to_ref()
        .await
        .map_err(|e| Error::TelegramApi(format!("Failed to convert peer to PeerRef: {e}")))?
        .ok_or_else(|| {
            Error::TelegramApi("Peer has no PeerRef (not in the session cache)".to_string())
        })
}

/// Reject an empty channel identifier before it reaches peer resolution.
///
/// Shared client-wide so every entry point reports the same error for the
/// same caller mistake.
pub(super) fn validate_channel_identifier(identifier: &str) -> Result<(), Error> {
    if identifier.is_empty() {
        return Err(Error::InvalidInput(
            "Channel identifier cannot be empty".to_string(),
        ));
    }
    Ok(())
}

impl TelegramClient {
    /// Canonical channel-reference resolver: a numeric ref is found by walking the
    /// dialog list; anything else is treated as a username (leading `@` stripped).
    /// Both forms return a typed not-found error when nothing matches. This is the
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
                Error::ChannelNotFound(channel_ref.to_string())
            })
        } else {
            // Username — resolve directly.
            let peer = self
                .resolve_username_peer(channel_ref)
                .await?
                .ok_or_else(|| {
                    tracing::warn!(channel_ref = %channel_ref, "Username not found");
                    Error::ChannelNotFound(channel_ref.to_string())
                })?;
            if peer_namespace(&peer) == PeerNamespace::User {
                return Err(Error::UnsupportedPeer);
            }
            Ok(peer)
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
    /// bounded by one resolve timeout. The whole traversal is needed to detect
    /// distinct eligible peer namespaces with the same bare ID. Duplicate typed
    /// identities are harmless; user peers are ineligible. `Ok(None)` means no
    /// eligible dialog matched. Shared
    /// by [`Self::resolve_peer`] and `get_recent_messages`'s dialog fallback.
    pub(super) async fn find_dialog_peer(
        &self,
        id: i64,
    ) -> Result<Option<grammers_client::peer::Peer>, Error> {
        with_timeout("iter_dialogs", self.timeouts.resolve_secs, async {
            let mut dialogs = self.client.iter_dialogs();
            let mut selected = NumericSelection::new(id);
            while let Some(dialog) = dialogs.next().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to iterate dialogs");
                Error::TelegramApi(format!("Failed to iterate dialogs: {}", e))
            })? {
                let peer = dialog.peer();
                selected.consider(peer_namespace(peer), peer.id().bare_id(), || peer.clone())?;
            }
            Ok(selected.finish())
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

/// A bounded dialog traversal must finish before a provisional numeric match is trusted.
pub(super) struct NumericSelection<T> {
    id: i64,
    selected: Option<(PeerNamespace, T)>,
}
impl<T> NumericSelection<T> {
    pub(super) fn new(id: i64) -> Self {
        Self { id, selected: None }
    }
    pub(super) fn consider(
        &mut self,
        namespace: PeerNamespace,
        id: Option<i64>,
        value: impl FnOnce() -> T,
    ) -> Result<(), Error> {
        if namespace == PeerNamespace::User || id != Some(self.id) {
            return Ok(());
        }
        match &self.selected {
            Some((existing, _)) if *existing != namespace => {
                Err(Error::AmbiguousChannel(self.id.to_string()))
            }
            Some(_) => Ok(()),
            None => {
                self.selected = Some((namespace, value()));
                Ok(())
            }
        }
    }
    pub(super) fn finish(self) -> Option<T> {
        self.selected.map(|(_, value)| value)
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    #[test]
    fn numeric_resolution_ignores_users_and_deduplicates_typed_ids() {
        let mut selection = NumericSelection::new(123);
        selection
            .consider(PeerNamespace::User, Some(123), || "user")
            .expect("skip");
        selection
            .consider(PeerNamespace::Channel, Some(124), || "other")
            .expect("skip");
        selection
            .consider(PeerNamespace::Channel, Some(123), || "channel")
            .expect("first");
        selection
            .consider(PeerNamespace::Channel, Some(123), || "duplicate")
            .expect("same peer");
        assert_eq!(selection.finish(), Some("channel"));
    }
    #[test]
    fn numeric_resolution_rejects_ambiguous_orderings() {
        for order in [
            [PeerNamespace::Chat, PeerNamespace::Channel],
            [PeerNamespace::Channel, PeerNamespace::Chat],
        ] {
            let mut selection = NumericSelection::new(123);
            selection
                .consider(order[0], Some(123), || ())
                .expect("provisional");
            assert!(matches!(
                selection.consider(order[1], Some(123), || ()),
                Err(Error::AmbiguousChannel(_))
            ));
        }
    }
}
