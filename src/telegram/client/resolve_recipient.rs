//! Write-specific resolution: one qualified identity, no read-resolver fallback or raw logging.

use super::TelegramClient;
use super::send_transport::classify_final_error;
use crate::telegram::converters::peer_namespace;
use crate::telegram::sending::{SendDeadline, SendStage};
use crate::telegram::types::sending::{
    QualifiedRecipient, RecipientSpec, SendFailure, SendFailureKind, UsernameNamespace,
};
use crate::telegram::types::{PeerNamespace, Username};
use grammers_client::peer::Peer;
use grammers_client::tl;
use grammers_mtsender::InvocationError;
use grammers_session::Session as _;
use grammers_session::types::{PeerId, PeerRef};
use std::future::Future;

pub(super) struct ResolvedRecipient {
    pub reference: PeerRef,
    pub identity: QualifiedRecipient,
    pub saved_messages: bool,
    pub forum: bool,
    pub link_username: Option<Username>,
    is_self: bool,
}

impl std::fmt::Debug for ResolvedRecipient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedRecipient")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

pub(super) struct RecipientSelection<T> {
    wanted: QualifiedRecipient,
    selected: Option<T>,
}
impl<T> RecipientSelection<T> {
    pub(super) fn new(wanted: QualifiedRecipient) -> Self {
        Self {
            wanted,
            selected: None,
        }
    }
    pub(super) fn consider(&mut self, identity: QualifiedRecipient, value: T) {
        if identity == self.wanted && self.selected.is_none() {
            self.selected = Some(value);
        }
    }
    pub(super) fn finish(self) -> Option<T> {
        self.selected
    }
}

fn missing() -> SendFailure {
    SendFailure::not_sent(SendFailureKind::RecipientNotFound, None)
}

pub(super) fn read_failure(error: InvocationError, fallback: SendFailureKind) -> SendFailure {
    let failure = classify_final_error(error);
    if failure.kind() == SendFailureKind::DeliveryUnknown {
        SendFailure::not_sent(fallback, None)
    } else {
        failure
    }
}

fn chat_forum(raw: &tl::enums::Chat) -> Result<bool, SendFailure> {
    match raw {
        tl::enums::Chat::Chat(chat) if !chat.deactivated && chat.migrated_to.is_none() => Ok(false),
        tl::enums::Chat::Channel(channel) => channel_forum(channel),
        _ => Err(missing()),
    }
}

fn channel_forum(channel: &tl::types::Channel) -> Result<bool, SendFailure> {
    // Pinned grammers converts ChannelForbidden into a Channel with date=0.
    // Such metadata cannot establish an accessible, non-forum posting target.
    if channel.monoforum || channel.date <= 0 {
        return Err(missing());
    }
    Ok(channel.forum)
}

pub(super) async fn from_peer(peer: &Peer) -> Result<ResolvedRecipient, SendFailure> {
    let identity = QualifiedRecipient::try_new(
        peer_namespace(peer),
        peer.id().bare_id().ok_or_else(missing)?,
    )?;
    let (forum, is_self) = match peer {
        Peer::User(user) => match &user.raw {
            tl::enums::User::User(raw) if !raw.deleted => (raw.bot_forum_view, raw.is_self),
            _ => return Err(missing()),
        },
        Peer::Group(group) => (chat_forum(&group.raw)?, false),
        Peer::Channel(channel) => (channel_forum(&channel.raw)?, false),
        // Communities and channel direct-message monoforums need a different
        // routing contract. Do not infer a normal posting destination for them.
        Peer::Community(_) => return Err(missing()),
    };
    let reference = peer
        .to_ref()
        .await
        .map_err(|_| SendFailure::not_sent(SendFailureKind::Unavailable, None))?
        .ok_or_else(missing)?;
    let link_username = if identity.namespace() == PeerNamespace::Channel {
        peer.username()
            .filter(|name| Username::is_valid_shape(name))
            .map(Username::new)
            .transpose()
            .map_err(|_| SendFailure::not_sent(SendFailureKind::InternalError, None))?
    } else {
        None
    };
    Ok(ResolvedRecipient {
        reference,
        identity,
        saved_messages: false,
        forum,
        link_username,
        is_self,
    })
}

pub(super) async fn resolve_with(
    spec: &RecipientSpec,
    deadline: SendDeadline,
    lookup: impl Future<Output = Result<ResolvedRecipient, SendFailure>>,
) -> Result<ResolvedRecipient, SendFailure> {
    let mut resolved = deadline.run(SendStage::Resolve, lookup).await?;
    let matches = match spec {
        RecipientSpec::SavedMessages => {
            resolved.is_self && resolved.identity.namespace() == PeerNamespace::User
        }
        RecipientSpec::Peer(expected) => resolved.identity == *expected,
        RecipientSpec::Username {
            expected_namespace, ..
        } => {
            resolved.identity.namespace()
                == match expected_namespace {
                    UsernameNamespace::User => PeerNamespace::User,
                    UsernameNamespace::Channel => PeerNamespace::Channel,
                }
        }
    };
    if !matches {
        return Err(SendFailure::not_sent(
            SendFailureKind::RecipientMismatch,
            None,
        ));
    }
    resolved.saved_messages = matches!(spec, RecipientSpec::SavedMessages);
    Ok(resolved)
}

fn session_id(identity: QualifiedRecipient) -> Result<PeerId, SendFailure> {
    match identity.namespace() {
        PeerNamespace::User => PeerId::user(identity.id()),
        PeerNamespace::Chat => PeerId::chat(identity.id()),
        PeerNamespace::Channel => PeerId::channel(identity.id()),
    }
    .ok_or_else(missing)
}

impl TelegramClient {
    pub(super) async fn resolve_recipient(
        &self,
        spec: &RecipientSpec,
        deadline: SendDeadline,
    ) -> Result<ResolvedRecipient, SendFailure> {
        let lookup = async {
            let peer = match spec {
                RecipientSpec::SavedMessages => Peer::User(
                    self.client
                        .get_me()
                        .await
                        .map_err(|e| read_failure(e, SendFailureKind::Unavailable))?,
                ),
                RecipientSpec::Username { username, .. } => {
                    if !Username::is_valid_shape(username.as_str()) {
                        return Err(SendFailure::not_sent(SendFailureKind::InvalidInput, None));
                    }
                    self.client
                        .resolve_username(username.as_str())
                        .await
                        .map_err(|e| read_failure(e, SendFailureKind::Unavailable))?
                        .ok_or_else(missing)?
                }
                RecipientSpec::Peer(identity) => {
                    let cached = self
                        .session
                        .peer_ref(session_id(*identity)?)
                        .await
                        .map_err(|_| SendFailure::not_sent(SendFailureKind::Unavailable, None))?;
                    if let Some(reference) = cached {
                        // Session references do not carry migration/forum metadata;
                        // retrieve that metadata without synthesizing an access hash.
                        self.client
                            .resolve_peer(reference)
                            .await
                            .map_err(|e| read_failure(e, SendFailureKind::RecipientNotFound))?
                    } else {
                        let mut dialogs = self.client.iter_dialogs();
                        let mut selection = RecipientSelection::new(*identity);
                        while let Some(dialog) = dialogs
                            .next()
                            .await
                            .map_err(|e| read_failure(e, SendFailureKind::Unavailable))?
                        {
                            if deadline.expired() {
                                return Err(SendFailure::not_sent(SendFailureKind::Timeout, None));
                            }
                            let peer = dialog.peer();
                            if let Some(id) = peer.id().bare_id() {
                                selection.consider(
                                    QualifiedRecipient::try_new(peer_namespace(peer), id)?,
                                    peer.clone(),
                                );
                            }
                        }
                        selection.finish().ok_or_else(missing)?
                    }
                }
            };
            from_peer(&peer).await
        };
        deadline
            .run_capped(
                SendStage::Resolve,
                self.timeouts.resolve_secs,
                resolve_with(spec, deadline, lookup),
            )
            .await
    }
}
