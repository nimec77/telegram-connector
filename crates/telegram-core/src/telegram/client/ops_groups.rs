//! Authoritative membership reads for basic groups and supergroups.

use super::TelegramClient;
use super::stream_source::stream_rpc_error;
use crate::error::Error;
use crate::telegram::timeout::with_timeout;
use grammers_client::{InvocationError, tl};
use grammers_session::Session;
use grammers_session::types::PeerId;

#[derive(Clone, Copy)]
enum GroupKind {
    Basic,
    Supergroup,
}

impl TelegramClient {
    /// Check current membership for a Telegram basic group.
    ///
    /// # Errors
    /// Fails for an invalid ID, an unavailable session, or a Telegram timeout.
    /// The caller owns rate-limit and admission policy.
    pub async fn joined_basic_group(&self, id: i64) -> Result<bool, Error> {
        PeerId::chat(id).ok_or_else(invalid_group)?;
        with_timeout("joined_basic_group", self.timeouts.resolve_secs, async {
            #[cfg(any(test, feature = "test-support"))]
            if let Some(fixture) = &self.metadata_fixture {
                return Ok(confirmed(&fixture.dialogs, id, GroupKind::Basic));
            }
            let result = self
                .client
                .invoke(&tl::functions::messages::GetChats { id: vec![id] })
                .await;
            Ok(group_rpc(result)?
                .is_some_and(|chats| confirmed(chat_rows(&chats), id, GroupKind::Basic)))
        })
        .await
    }

    /// Check current membership for a Telegram supergroup.
    ///
    /// # Errors
    /// Fails for an invalid ID, an unavailable session, or a Telegram timeout.
    /// The caller owns rate-limit and admission policy.
    pub async fn joined_supergroup(&self, id: i64) -> Result<bool, Error> {
        let peer_id = PeerId::channel(id).ok_or_else(invalid_group)?;
        with_timeout("joined_supergroup", self.timeouts.resolve_secs, async {
            #[cfg(any(test, feature = "test-support"))]
            if let Some(fixture) = &self.metadata_fixture {
                return Ok(confirmed(&fixture.dialogs, id, GroupKind::Supergroup));
            }
            let Some(channel) = self.group_input_channel(peer_id).await? else {
                return Ok(false);
            };
            let result = self
                .client
                .invoke(&tl::functions::channels::GetChannels { id: vec![channel] })
                .await;
            Ok(group_rpc(result)?
                .is_some_and(|chats| confirmed(chat_rows(&chats), id, GroupKind::Supergroup)))
        })
        .await
    }

    pub(crate) async fn joined_supergroup_pts(&self, id: i64) -> Result<Option<i32>, Error> {
        let peer_id = PeerId::channel(id).ok_or_else(invalid_group)?;
        with_timeout("joined_supergroup_pts", self.timeouts.resolve_secs, async {
            let Some(channel) = self.group_input_channel(peer_id).await? else {
                return Ok(None);
            };
            let result = self
                .client
                .invoke(&tl::functions::channels::GetFullChannel { channel })
                .await;
            let Some(tl::enums::messages::ChatFull::Full(result)) = group_rpc(result)? else {
                return Ok(None);
            };
            if !confirmed(&result.chats, id, GroupKind::Supergroup) {
                return Ok(None);
            }
            match result.full_chat {
                tl::enums::ChatFull::ChannelFull(channel) if channel.id == id => {
                    Ok(Some(channel.pts))
                }
                _ => Err(Error::TelegramApi("Group checkpoint unavailable".into())),
            }
        })
        .await
    }

    async fn group_input_channel(
        &self,
        peer: PeerId,
    ) -> Result<Option<tl::enums::InputChannel>, Error> {
        if let Some(reference) = self
            .session
            .peer(peer)
            .await
            .map_err(|_| Error::TelegramApi("Group peer unavailable".into()))?
            .and_then(|entry| entry.auth())
        {
            return Ok(Some(
                grammers_session::types::PeerRef {
                    id: peer,
                    auth: reference,
                }
                .into(),
            ));
        }
        let mut dialogs = self.client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await.map_err(stream_rpc_error)? {
            if dialog.peer().id() == peer {
                return dialog
                    .peer()
                    .to_ref()
                    .await
                    .map(|reference| reference.map(Into::into))
                    .map_err(|_| Error::TelegramApi("Group peer unavailable".into()));
            }
        }
        Ok(None)
    }
}

fn chat_rows(chats: &tl::enums::messages::Chats) -> &[tl::enums::Chat] {
    match chats {
        tl::enums::messages::Chats::Chats(result) => &result.chats,
        tl::enums::messages::Chats::Slice(result) => &result.chats,
    }
}

fn confirmed(chats: &[tl::enums::Chat], id: i64, kind: GroupKind) -> bool {
    let mut evidence = chats.iter().filter_map(|chat| match (kind, chat) {
        (GroupKind::Basic, tl::enums::Chat::Chat(raw)) if raw.id == id => {
            Some(!raw.left && !raw.deactivated && raw.migrated_to.is_none())
        }
        (GroupKind::Basic, tl::enums::Chat::Empty(raw)) if raw.id == id => Some(false),
        (GroupKind::Basic, tl::enums::Chat::Forbidden(raw)) if raw.id == id => Some(false),
        (GroupKind::Supergroup, tl::enums::Chat::Channel(raw)) if raw.id == id => {
            Some(raw.megagroup && !raw.broadcast && !raw.left && !raw.min && !raw.monoforum)
        }
        (GroupKind::Supergroup, tl::enums::Chat::ChannelForbidden(raw)) if raw.id == id => {
            Some(false)
        }
        (GroupKind::Supergroup, tl::enums::Chat::Community(raw)) if raw.id == id => Some(false),
        (GroupKind::Supergroup, tl::enums::Chat::CommunityForbidden(raw)) if raw.id == id => {
            Some(false)
        }
        _ => None,
    });
    evidence.next() == Some(true) && evidence.all(|joined| joined)
}

fn group_rpc<T>(response: Result<T, InvocationError>) -> Result<Option<T>, Error> {
    match response {
        Ok(value) => Ok(Some(value)),
        Err(error)
            if error.is("CHANNEL_PRIVATE")
                || error.is("CHANNEL_INVALID")
                || error.is("CHAT_ID_INVALID")
                || error.is("USER_NOT_PARTICIPANT") =>
        {
            Ok(None)
        }
        Err(error) => Err(stream_rpc_error(error)),
    }
}

fn invalid_group() -> Error {
    Error::InvalidInput("Invalid Telegram group ID".into())
}

#[cfg(test)]
mod tests {
    use super::tl;

    #[tokio::test]
    async fn group_membership_keeps_colliding_ids_separate_and_rejects_left_groups() {
        use crate::telegram::client::client_fixture::stalled_client;
        use crate::telegram::client::metadata_fixture::MetadataFixture;

        let (mut client, _runner, _root) = stalled_client().await;
        let basic = tl::types::Chat {
            creator: false,
            left: false,
            deactivated: false,
            call_active: false,
            call_not_empty: false,
            noforwards: false,
            id: 42,
            title: "Basic group".into(),
            photo: tl::enums::ChatPhoto::Empty,
            participants_count: 2,
            date: 0,
            version: 1,
            migrated_to: None,
            admin_rights: None,
            default_banned_rights: None,
        };
        let mut supergroup = crate::test_helpers::raw_tl_channel(42, "Supergroup", None);
        supergroup.broadcast = false;
        supergroup.megagroup = true;
        supergroup.left = true;
        client.set_metadata_fixture(MetadataFixture {
            dialogs: vec![basic.clone().into(), supergroup.clone().into()],
            ..MetadataFixture::default()
        });
        assert!(
            client
                .joined_basic_group(42)
                .await
                .expect("joined basic group")
        );
        assert!(!client.joined_supergroup(42).await.expect("left supergroup"));

        supergroup.left = false;
        client.set_metadata_fixture(MetadataFixture {
            dialogs: vec![
                tl::types::Chat {
                    left: true,
                    ..basic
                }
                .into(),
                supergroup.into(),
            ],
            ..MetadataFixture::default()
        });
        assert!(
            !client
                .joined_basic_group(42)
                .await
                .expect("left basic group")
        );
        assert!(
            client
                .joined_supergroup(42)
                .await
                .expect("joined supergroup")
        );
    }
}
