use super::resolve_recipient::{RecipientSelection, from_peer, resolve_with};
use crate::telegram::sending::SendDeadline;
use crate::telegram::types::sending::{
    QualifiedRecipient, RecipientSpec, SendFailureKind, UsernameNamespace,
};
use crate::telegram::types::{PeerNamespace, Username};
use crate::test_helpers::{raw_tl_channel, raw_tl_user};
use grammers_client::peer::{Peer, User};
use grammers_client::tl;

#[test]
fn sending_recipient_selection_uses_qualified_ids_and_deduplicates_dialogs() {
    for wanted in [
        PeerNamespace::User,
        PeerNamespace::Chat,
        PeerNamespace::Channel,
    ] {
        let mut selection =
            RecipientSelection::new(QualifiedRecipient::try_new(wanted, 91).expect("ID"));
        for namespace in [
            PeerNamespace::User,
            PeerNamespace::Chat,
            PeerNamespace::Channel,
            wanted,
        ] {
            selection.consider(
                QualifiedRecipient::try_new(namespace, 91).expect("ID"),
                namespace,
            );
        }
        assert_eq!(selection.finish(), Some(wanted));
    }
}

#[tokio::test]
async fn sending_recipient_user_and_saved_messages_retain_real_identity() {
    let (client, _runner, _dir) = super::stalled_client().await;
    let mut user = raw_tl_user(91, Some("private"), None, Some("alice"));
    user.is_self = true;
    let peer = Peer::User(User::from_raw(client.client(), user.into()));
    let resolved = resolve_with(
        &RecipientSpec::SavedMessages,
        SendDeadline::new(30).expect("budget"),
        from_peer(&peer),
    )
    .await
    .expect("self");
    assert_eq!(resolved.identity.id(), 91);
    assert_eq!(resolved.identity.namespace(), PeerNamespace::User);
    assert!(resolved.saved_messages);
    assert!(resolved.link_username.is_none());
}

#[tokio::test]
async fn sending_recipient_username_must_match_namespace_without_second_lookup() {
    let (client, _runner, _dir) = super::stalled_client().await;
    let mut raw = raw_tl_channel(91, "private title", Some("alice"));
    raw.date = 1_700_000_000;
    let peer = Peer::from_raw(client.client(), raw.into());
    let spec = RecipientSpec::Username {
        username: Username::new("alice").expect("name"),
        expected_namespace: UsernameNamespace::User,
    };
    let failure = resolve_with(
        &spec,
        SendDeadline::new(30).expect("budget"),
        from_peer(&peer),
    )
    .await
    .expect_err("wrong namespace");
    assert_eq!(failure.kind(), SendFailureKind::RecipientMismatch);
}

#[tokio::test]
async fn sending_recipient_rejects_deleted_migrated_and_missing_authority() {
    let (client, _runner, _dir) = super::stalled_client().await;
    let mut user = raw_tl_user(91, None, None, None);
    user.deleted = true;
    assert!(
        from_peer(&Peer::User(User::from_raw(client.client(), user.into())))
            .await
            .is_err()
    );
    let mut channel = raw_tl_channel(91, "private", None);
    channel.access_hash = None;
    channel.min = true;
    assert!(
        from_peer(&Peer::from_raw(client.client(), channel.into()))
            .await
            .is_err()
    );
    let chat: tl::enums::Chat = tl::types::Chat {
        creator: false,
        left: false,
        deactivated: false,
        call_active: false,
        call_not_empty: false,
        noforwards: false,
        id: 91,
        title: "private".into(),
        photo: tl::enums::ChatPhoto::Empty,
        participants_count: 2,
        date: 1,
        version: 1,
        migrated_to: Some(
            tl::types::InputChannel {
                channel_id: 92,
                access_hash: 739,
            }
            .into(),
        ),
        admin_rights: None,
        default_banned_rights: None,
    }
    .into();
    let failure = from_peer(&Peer::from_raw(client.client(), chat))
        .await
        .expect_err("migration");
    assert_eq!(failure.kind(), SendFailureKind::RecipientNotFound);
}

#[tokio::test]
async fn sending_recipient_inaccessible_broadcast_cannot_become_a_send_target() {
    let (client, _runner, _dir) = super::stalled_client().await;
    let raw = tl::types::ChannelForbidden {
        broadcast: true,
        monoforum: false,
        megagroup: false,
        id: 91,
        access_hash: 739,
        title: "private".into(),
        until_date: None,
    };
    let peer = Peer::from_raw(client.client(), raw.into());
    let failure = from_peer(&peer)
        .await
        .expect_err("forbidden metadata cannot authorize dispatch");
    assert_eq!(failure.kind(), SendFailureKind::RecipientNotFound);
}

#[tokio::test(start_paused = true)]
async fn sending_recipient_lookup_is_bounded_by_stage_cap_and_shared_deadline() {
    let (client, _runner, _dir) = super::stalled_client().await;
    let start = tokio::time::Instant::now();
    let spec = RecipientSpec::Username {
        username: Username::new("alice").expect("name"),
        expected_namespace: UsernameNamespace::User,
    };
    let error = client
        .resolve_recipient(&spec, SendDeadline::new(30).expect("budget"))
        .await
        .expect_err("stalled RPC");
    assert_eq!(error.kind(), SendFailureKind::Timeout);
    assert_eq!(
        tokio::time::Instant::now() - start,
        std::time::Duration::from_secs(2)
    );
}
